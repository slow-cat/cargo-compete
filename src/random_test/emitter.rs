//! Value generation and output emission for a walked format block.

use super::budget::Budget;
use super::context::{ArrayCtx, Ctx, StrCtx};
use super::gen::{
    effective_lo_hi, gen_int, gen_int_array, gen_scalar, gen_string, Denominators, StructuralSizes,
};
use super::relation::{
    bounded_distinct_int, gen_int_array_with_positional_bounds, gen_positionally_bounded_int,
    has_any_pair_constraint, has_array_element_constraints, narrow_bounds_from_scalars,
    narrow_scalar_bounds, not_equal_forbidden_scalar, record_array_values,
};
use super::spec::{ResolvedSpec, SizeTerm, VarInfo};
use super::strategy::{CaseStrategy, DeterministicStrategy, RandomStrategy};
use crate::parse::{ArrayBlock, BoundRepr, RowsBlock, VarType};
use rand::Rng;
use std::collections::HashSet;

pub(super) struct RenderEnv<'a> {
    pub(super) spec: &'a ResolvedSpec,
    pub(super) st: &'a CaseStrategy,
    pub(super) sizes: &'a StructuralSizes,
    pub(super) denoms: &'a Denominators,
}

// ─── value helpers ────────────────────────────────────────────────────────────

// Scalars reuse the positional integer picker so enum domains, ordering,
// not_equal, and fallback strategy behaviour stay identical to constrained
// array elements. A scalar is the only element in that synthetic sequence.
const SCALAR_POSITION: usize = 0;
const SCALAR_SEQUENCE_LEN: usize = 1;

/// One integer scalar. Size variables get strategy-specific size behaviour
/// (`SmallSize(k)` → `k`, `MaxSize` → max); everything else defers to
/// [`gen_scalar`] which handles AllMax/AllMin/ZeroCorner/enum/random.
fn scalar_value(
    spec: &ResolvedSpec,
    name: &str,
    info: &VarInfo,
    sizes: &StructuralSizes,
    denoms: &Denominators,
    st: &CaseStrategy,
    rng: &mut impl Rng,
) -> i64 {
    let (lo, hi) = effective_lo_hi(name, info, sizes, denoms);
    let is_size = spec.size_vars.contains(name);
    match st {
        CaseStrategy::Random(RandomStrategy::SmallSize(k)) if is_size => (*k).max(lo).min(hi),
        CaseStrategy::Random(RandomStrategy::MaxSize) if is_size => hi,
        _ => gen_scalar(st, lo, hi, info.values.as_deref(), rng),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn constrained_scalar_value(
    spec: &ResolvedSpec,
    name: &str,
    info: &VarInfo,
    sizes: &StructuralSizes,
    denoms: &Denominators,
    st: &CaseStrategy,
    ctx: &Ctx,
    array_ctx: &ArrayCtx,
    rng: &mut impl Rng,
) -> Option<i64> {
    let (lo, hi) = effective_lo_hi(name, info, sizes, denoms);
    let (lo, hi) = narrow_scalar_bounds(name, lo, hi, spec, ctx, array_ctx)?;
    let forbidden = not_equal_forbidden_scalar(name, spec, ctx, array_ctx);
    let used = HashSet::new();
    let is_size = spec.size_vars.contains(name);
    let candidate = match st {
        CaseStrategy::Random(RandomStrategy::SmallSize(k)) if is_size => Some((*k).max(lo).min(hi)),
        CaseStrategy::Random(RandomStrategy::MaxSize) if is_size => Some(hi),
        _ => None,
    };
    if let Some(x) = candidate {
        if !forbidden.contains(&x) {
            return Some(x);
        }
        return bounded_distinct_int(st, lo, hi, &used, &forbidden, rng);
    }
    gen_positionally_bounded_int(
        st,
        SCALAR_POSITION,
        SCALAR_SEQUENCE_LEN,
        lo,
        hi,
        info.values.as_deref(),
        false,
        &used,
        &forbidden,
        false,
        rng,
    )
}

fn size_value(
    spec: &ResolvedSpec,
    name: &str,
    sizes: &StructuralSizes,
    denoms: &Denominators,
    st: &CaseStrategy,
    rng: &mut impl Rng,
) -> i64 {
    match spec.vars.get(name) {
        Some(info) => constrained_scalar_value(
            spec,
            name,
            info,
            sizes,
            denoms,
            st,
            &Ctx::new(),
            &ArrayCtx::new(),
            rng,
        )
        .unwrap_or_else(|| scalar_value(spec, name, info, sizes, denoms, st, rng)),
        None => 0,
    }
}

/// Resolve a count / size variable: reuse the context value if present
/// (seeded structural size or earlier scalar), else decide it now and cache it
/// so a later reference stays consistent.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_count(
    name: &str,
    spec: &ResolvedSpec,
    sizes: &StructuralSizes,
    denoms: &Denominators,
    st: &CaseStrategy,
    ctx: &mut Ctx,
    rng: &mut impl Rng,
) -> i64 {
    if let Some(&v) = ctx.get(name) {
        return v;
    }
    // Size fields were parsed once while resolving the persisted spec.
    let v = match spec.size_terms.get(name) {
        Some(SizeTerm::Lit(n)) => *n,
        Some(SizeTerm::Var(vn)) => size_value(spec, vn, sizes, denoms, st, rng),
        Some(SizeTerm::VarOffset(vn, off)) => {
            resolve_count(vn, spec, sizes, denoms, st, ctx, rng) + *off
        }
        None => size_value(spec, name, sizes, denoms, st, rng),
    };
    ctx.insert(name.to_string(), v);
    v
}

/// Resolve the length of one emitted Chars token (`vars[s].len`).
///
/// Pipe-wrapped names are parser-created length domains, not input size
/// variables. Each emitted string samples that domain independently; regular
/// expressions keep using the cached structural value shared by the input.
#[allow(clippy::too_many_arguments)]
fn resolve_len(
    repr: &Option<BoundRepr>,
    spec: &ResolvedSpec,
    sizes: &StructuralSizes,
    denoms: &Denominators,
    st: &CaseStrategy,
    ctx: &mut Ctx,
    rng: &mut impl Rng,
) -> usize {
    match repr {
        Some(BoundRepr::Lit(n)) => (*n).max(0) as usize,
        Some(BoundRepr::Expr(name)) if is_synthetic_chars_len(name) => {
            size_value(spec, name, sizes, denoms, st, rng).max(0) as usize
        }
        Some(BoundRepr::Expr(expr)) => match spec.size_terms.get(expr) {
            Some(SizeTerm::Lit(n)) => (*n).max(0) as usize,
            Some(SizeTerm::Var(name)) => {
                resolve_count(name, spec, sizes, denoms, st, ctx, rng).max(0) as usize
            }
            Some(SizeTerm::VarOffset(name, off)) => {
                (resolve_count(name, spec, sizes, denoms, st, ctx, rng) + *off).max(0) as usize
            }
            None => 0,
        },
        None => 0,
    }
}

fn is_synthetic_chars_len(name: &str) -> bool {
    name.len() >= 2 && name.starts_with('|') && name.ends_with('|')
}

/// Strategy-driven size value when `(lo, hi)` are already effective bounds
/// (jagged per-row length). Non-size strategies sample uniformly.
fn strat_size(st: &CaseStrategy, lo: i64, hi: i64, rng: &mut impl Rng) -> i64 {
    match st {
        CaseStrategy::Deterministic(DeterministicStrategy::AllMax) => hi,
        CaseStrategy::Deterministic(DeterministicStrategy::AllMin) => lo,
        CaseStrategy::Random(RandomStrategy::SmallSize(k)) => (*k).max(lo).min(hi),
        CaseStrategy::Random(RandomStrategy::MaxSize) => hi,
        _ => gen_int(lo, hi, rng),
    }
}

pub(super) fn gen_chars(
    info: &VarInfo,
    env: &RenderEnv<'_>,
    ctx: &mut Ctx,
    budget: &mut Budget,
    rng: &mut impl Rng,
) -> Result<String, String> {
    let len = resolve_len(&info.len, env.spec, env.sizes, env.denoms, env.st, ctx, rng);
    budget.add(len as u128)?;
    match &info.charset {
        Some(cs) if !cs.is_empty() => Ok(gen_string(env.st, cs, len, None, rng)),
        _ => Ok(String::new()),
    }
}

fn join_ints(v: &[i64]) -> String {
    let mut s = String::new();
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&x.to_string());
    }
    s
}

fn is_altmaxmin(st: &CaseStrategy) -> bool {
    matches!(st, CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin))
}

// ─── array renderers ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn render_jagged(
    a: &ArrayBlock,
    spec: &ResolvedSpec,
    st: &CaseStrategy,
    sizes: &StructuralSizes,
    denoms: &Denominators,
    ctx: &mut Ctx,
    array_ctx: &mut ArrayCtx,
    lines: &mut Vec<String>,
    budget: &mut Budget,
    rng: &mut impl Rng,
) -> Result<bool, String> {
    let n = match &a.count {
        Some(c) => resolve_count(c, spec, sizes, denoms, st, ctx, rng),
        None => 0,
    }
    .max(0);
    let len_var = match &a.len {
        Some(l) => l,
        None => return Ok(true),
    };
    let (elo, ehi, values) = match spec.vars.get(&a.base) {
        Some(info) => {
            let (lo, hi) = effective_lo_hi(&a.base, info, sizes, denoms);
            let Some((lo, hi)) = narrow_bounds_from_scalars(&a.base, lo, hi, spec, ctx) else {
                return Ok(false);
            };
            (lo, hi, info.values.as_deref())
        }
        None => (0, 0, None),
    };
    let distinct = spec
        .vars
        .get(&a.base)
        .map(|i| i.all_distinct)
        .unwrap_or(false);
    for _ in 0..n {
        let li = match spec.vars.get(len_var) {
            Some(info) => {
                let (llo, lhi) = effective_lo_hi(len_var, info, sizes, denoms);
                strat_size(st, llo, lhi, rng)
            }
            None => 0,
        }
        .max(0);
        budget.add((li as u128).checked_add(1).ok_or_else(|| {
            "input too large: generated jagged row element count overflows 128-bit range"
                .to_string()
        })?)?;
        let start = array_ctx.get(&a.base).map_or(0, Vec::len);
        let elems = match gen_int_array_with_positional_bounds(
            st,
            &a.base,
            elo,
            ehi,
            li as usize,
            values,
            distinct,
            spec,
            ctx,
            array_ctx,
            start,
            rng,
        ) {
            Some(e) => e,
            None => return Ok(false),
        };
        let mut line = li.to_string();
        record_array_values(array_ctx, &a.base, &elems);
        for e in &elems {
            line.push(' ');
            line.push_str(&e.to_string());
        }
        lines.push(line);
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_chars_array(
    a: &ArrayBlock,
    spec: &ResolvedSpec,
    st: &CaseStrategy,
    sizes: &StructuralSizes,
    denoms: &Denominators,
    ctx: &mut Ctx,
    _str_ctx: &mut StrCtx,
    lines: &mut Vec<String>,
    budget: &mut Budget,
    rng: &mut impl Rng,
) -> Result<(), String> {
    let count = match &a.count {
        Some(c) => resolve_count(c, spec, sizes, denoms, st, ctx, rng),
        None => 1,
    }
    .max(0) as usize;
    let height = match &a.height {
        Some(h) => resolve_count(h, spec, sizes, denoms, st, ctx, rng).max(1) as usize,
        None => 1,
    };
    let total = count.saturating_mul(height);
    let info = match spec.vars.get(&a.base) {
        Some(i) => i,
        None => {
            for _ in 0..total {
                budget.add(0)?;
                lines.push(String::new());
            }
            return Ok(());
        }
    };
    let charset = info.charset.clone().unwrap_or_default();
    let phase = if is_altmaxmin(st) {
        rng.gen_range(0..2usize)
    } else {
        0
    };
    for idx in 0..total {
        let slen = resolve_len(&info.len, spec, sizes, denoms, st, ctx, rng);
        budget.add(slen as u128)?;
        if charset.is_empty() {
            lines.push(String::new());
        } else {
            lines.push(gen_string(
                st,
                &charset,
                slen,
                Some((idx, total, phase)),
                rng,
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_int_array(
    a: &ArrayBlock,
    spec: &ResolvedSpec,
    st: &CaseStrategy,
    sizes: &StructuralSizes,
    denoms: &Denominators,
    ctx: &mut Ctx,
    array_ctx: &mut ArrayCtx,
    lines: &mut Vec<String>,
    budget: &mut Budget,
    rng: &mut impl Rng,
) -> Result<bool, String> {
    let (lo, hi, values) = match spec.vars.get(&a.base) {
        Some(info) => {
            let (lo, hi) = effective_lo_hi(&a.base, info, sizes, denoms);
            let Some((lo, hi)) = narrow_bounds_from_scalars(&a.base, lo, hi, spec, ctx) else {
                return Ok(false);
            };
            (lo, hi, info.values.as_deref())
        }
        None => (0, 0, None),
    };
    let distinct = spec
        .vars
        .get(&a.base)
        .map(|i| i.all_distinct)
        .unwrap_or(false);
    let len = match &a.len {
        Some(l) => resolve_count(l, spec, sizes, denoms, st, ctx, rng),
        None => 0,
    }
    .max(0) as usize;

    let count = a
        .count
        .as_ref()
        .map(|c| resolve_count(c, spec, sizes, denoms, st, ctx, rng).max(0) as usize);
    let height = a
        .height
        .as_ref()
        .map(|h| resolve_count(h, spec, sizes, denoms, st, ctx, rng).max(0) as usize);

    let rows = match (count, height) {
        (None, None) => {
            budget.add(len as u128)?;
            let start = array_ctx.get(&a.base).map_or(0, Vec::len);
            match gen_int_array_with_positional_bounds(
                st, &a.base, lo, hi, len, values, distinct, spec, ctx, array_ctx, start, rng,
            ) {
                Some(e) => {
                    record_array_values(array_ctx, &a.base, &e);
                    lines.push(join_ints(&e));
                }
                None => return Ok(false),
            }
            return Ok(true);
        }
        (Some(c), None) => c,
        (None, Some(h)) => h,
        (Some(c), Some(h)) => c.saturating_mul(h),
    };
    budget.add((rows as u128).checked_mul(len as u128).ok_or_else(|| {
        "input too large: generated array element count overflows 128-bit range".to_string()
    })?)?;

    if is_altmaxmin(st)
        && !has_any_pair_constraint(&a.base, spec)
        && !has_array_element_constraints(
            &a.base,
            0,
            rows.saturating_mul(len),
            spec,
            ctx,
            array_ctx,
        )
    {
        let phase = rng.gen_range(0..2usize);
        let (lo, hi) = values
            .filter(|vs| !vs.is_empty())
            .map(|vs| (*vs.iter().min().unwrap(), *vs.iter().max().unwrap()))
            .unwrap_or((lo, hi));
        for r in 0..rows {
            let row: Vec<i64> = (0..len)
                .map(|c| if (r + c + phase) % 2 == 0 { hi } else { lo })
                .collect();
            record_array_values(array_ctx, &a.base, &row);
            lines.push(join_ints(&row));
        }
    } else {
        for _ in 0..rows {
            let start = array_ctx.get(&a.base).map_or(0, Vec::len);
            match gen_int_array_with_positional_bounds(
                st, &a.base, lo, hi, len, values, distinct, spec, ctx, array_ctx, start, rng,
            ) {
                Some(e) => {
                    record_array_values(array_ctx, &a.base, &e);
                    lines.push(join_ints(&e));
                }
                None => return Ok(false),
            }
        }
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_rows(
    b: &RowsBlock,
    spec: &ResolvedSpec,
    st: &CaseStrategy,
    sizes: &StructuralSizes,
    denoms: &Denominators,
    ctx: &mut Ctx,
    array_ctx: &mut ArrayCtx,
    lines: &mut Vec<String>,
    budget: &mut Budget,
    rng: &mut impl Rng,
) -> Result<bool, String> {
    let rows = resolve_count(&b.len, spec, sizes, denoms, st, ctx, rng).max(0) as usize;
    if rows == 0 {
        return Ok(true);
    }
    let altmm = is_altmaxmin(st);
    let phase = if altmm { rng.gen_range(0..2usize) } else { 0 };

    let mut cols: Vec<Vec<String>> = Vec::with_capacity(b.vars.len());
    for v in &b.vars {
        match spec.vars.get(v) {
            Some(info) if info.ty == VarType::Chars => {
                let charset = info.charset.clone().unwrap_or_default();
                let lenrepr = info.len.clone();
                let mut col = Vec::with_capacity(rows);
                for i in 0..rows {
                    let slen = resolve_len(&lenrepr, spec, sizes, denoms, st, ctx, rng);
                    budget.add(slen as u128)?;
                    if charset.is_empty() {
                        col.push(String::new());
                    } else {
                        col.push(gen_string(st, &charset, slen, Some((i, rows, phase)), rng));
                    }
                }
                cols.push(col);
            }
            Some(info) => {
                let (lo, hi) = effective_lo_hi(v, info, sizes, denoms);
                let Some((lo, hi)) = narrow_bounds_from_scalars(v, lo, hi, spec, ctx) else {
                    return Ok(false);
                };
                let start = array_ctx.get(v).map_or(0, Vec::len);
                let col: Vec<i64> = if altmm
                    && !has_any_pair_constraint(v, spec)
                    && !has_array_element_constraints(v, start, rows, spec, ctx, array_ctx)
                {
                    let (lo, hi) = info
                        .values
                        .as_ref()
                        .filter(|vs| !vs.is_empty())
                        .map(|vs| (*vs.iter().min().unwrap(), *vs.iter().max().unwrap()))
                        .unwrap_or((lo, hi));
                    (0..rows)
                        .map(|i| if (i + phase) % 2 == 0 { hi } else { lo })
                        .collect()
                } else {
                    match gen_int_array_with_positional_bounds(
                        st,
                        v,
                        lo,
                        hi,
                        rows,
                        info.values.as_deref(),
                        info.all_distinct,
                        spec,
                        ctx,
                        array_ctx,
                        start,
                        rng,
                    ) {
                        Some(e) => e,
                        None => return Ok(false),
                    }
                };
                budget.add(rows as u128)?;
                record_array_values(array_ctx, v, &col);
                cols.push(col.iter().map(|x| x.to_string()).collect());
            }
            None => cols.push(vec!["0".to_string(); rows]),
        }
    }
    for i in 0..rows {
        let line: Vec<&str> = cols.iter().map(|c| c[i].as_str()).collect();
        lines.push(line.join(" "));
    }
    Ok(true)
}
