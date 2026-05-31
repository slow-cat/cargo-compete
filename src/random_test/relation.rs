//! Enforcement and generation support for persisted `ordering` / `not_equal` pairs.

use super::context::{ArrayCtx, Ctx, StrCtx};
use super::gen::{gen_int, gen_int_array};
use super::spec::ResolvedSpec;
use super::strategy::{CaseStrategy, DeterministicStrategy, RandomStrategy};
use rand::Rng;
use std::collections::HashSet;

/// Check `ordering` / `not_equal` pairs for values already rendered in this
/// scope. Array pairs are compared by flattened position; scalar-array pairs
/// compare the scalar against every known element.
pub(super) fn pairs_ok(
    spec: &ResolvedSpec,
    sc: &Ctx,
    strings: &StrCtx,
    arrays: &ArrayCtx,
    parent: Option<&HashSet<String>>,
    parent_arrays: Option<&HashSet<String>>,
) -> bool {
    let is_local = |v: &str| parent.map(|p| !p.contains(v)).unwrap_or(true);
    let is_local_array = |v: &str| parent_arrays.map(|p| !p.contains(v)).unwrap_or(true);
    let int_val = |v: &str| -> Option<i64> {
        if !is_local(v) {
            return None;
        }
        sc.get(v).copied()
    };
    let str_val = |v: &str| -> Option<&str> {
        if !is_local(v) {
            return None;
        }
        strings.get(v).map(String::as_str)
    };
    let arr_val = |v: &str| -> Option<&[i64]> {
        if !is_local_array(v) {
            return None;
        }
        arrays.get(v).map(Vec::as_slice)
    };

    for (a, b) in &spec.ordering {
        if let (Some(x), Some(y)) = (int_val(a), int_val(b)) {
            if x > y {
                return false;
            }
        }
        if let (Some(x), Some(ys)) = (int_val(a), arr_val(b)) {
            if ys.iter().any(|&y| x > y) {
                return false;
            }
        }
        if let (Some(xs), Some(y)) = (arr_val(a), int_val(b)) {
            if xs.iter().any(|&x| x > y) {
                return false;
            }
        }
        if let (Some(xs), Some(ys)) = (arr_val(a), arr_val(b)) {
            if xs.iter().zip(ys).any(|(&x, &y)| x > y) {
                return false;
            }
        }
    }
    for (a, b) in &spec.not_equal {
        if let (Some(x), Some(y)) = (int_val(a), int_val(b)) {
            if x == y {
                return false;
            }
        }
        if let (Some(x), Some(ys)) = (int_val(a), arr_val(b)) {
            if ys.contains(&x) {
                return false;
            }
        }
        if let (Some(xs), Some(y)) = (arr_val(a), int_val(b)) {
            if xs.contains(&y) {
                return false;
            }
        }
        if let (Some(xs), Some(ys)) = (arr_val(a), arr_val(b)) {
            if xs.iter().zip(ys).any(|(&x, &y)| x == y) {
                return false;
            }
        }
        if let (Some(x), Some(y)) = (str_val(a), str_val(b)) {
            if x == y {
                return false;
            }
        }
    }
    true
}

pub(super) fn narrow_scalar_bounds(
    name: &str,
    lo: i64,
    hi: i64,
    spec: &ResolvedSpec,
    ctx: &Ctx,
    array_ctx: &ArrayCtx,
) -> Option<(i64, i64)> {
    let (mut lo, mut hi) = narrow_bounds_from_scalars(name, lo, hi, spec, ctx)?;
    for (a, b) in &spec.ordering {
        if a == name {
            if let Some(bound) = array_ctx.get(b).and_then(|xs| xs.iter().min()) {
                hi = hi.min(*bound);
            }
        }
        if b == name {
            if let Some(bound) = array_ctx.get(a).and_then(|xs| xs.iter().max()) {
                lo = lo.max(*bound);
            }
        }
    }
    valid_bounds(lo, hi)
}

pub(super) fn narrow_bounds_from_scalars(
    name: &str,
    lo: i64,
    hi: i64,
    spec: &ResolvedSpec,
    ctx: &Ctx,
) -> Option<(i64, i64)> {
    let mut lo = lo;
    let mut hi = hi;
    for (a, b) in &spec.ordering {
        if a == name {
            if let Some(&bound) = ctx.get(b) {
                hi = hi.min(bound);
            }
        }
        if b == name {
            if let Some(&bound) = ctx.get(a) {
                lo = lo.max(bound);
            }
        }
    }
    valid_bounds(lo, hi)
}

fn narrow_element_bounds(
    name: &str,
    index: usize,
    lo: i64,
    hi: i64,
    spec: &ResolvedSpec,
    array_ctx: &ArrayCtx,
) -> Option<(i64, i64)> {
    let mut lo = lo;
    let mut hi = hi;
    for (a, b) in &spec.ordering {
        if a == name {
            if let Some(&bound) = array_ctx.get(b).and_then(|xs| xs.get(index)) {
                hi = hi.min(bound);
            }
        }
        if b == name {
            if let Some(&bound) = array_ctx.get(a).and_then(|xs| xs.get(index)) {
                lo = lo.max(bound);
            }
        }
    }
    valid_bounds(lo, hi)
}

fn valid_bounds(lo: i64, hi: i64) -> Option<(i64, i64)> {
    if lo > hi {
        None
    } else {
        Some((lo, hi))
    }
}

pub(super) fn record_array_values(array_ctx: &mut ArrayCtx, name: &str, values: &[i64]) {
    array_ctx
        .entry(name.to_string())
        .or_default()
        .extend_from_slice(values);
}

fn has_positional_array_bounds(
    name: &str,
    start: usize,
    len: usize,
    spec: &ResolvedSpec,
    array_ctx: &ArrayCtx,
) -> bool {
    spec.ordering.iter().any(|(a, b)| {
        let other = if a == name {
            Some(b)
        } else if b == name {
            Some(a)
        } else {
            None
        };
        other
            .and_then(|v| array_ctx.get(v))
            .is_some_and(|xs| start < xs.len() && start.saturating_add(len) > 0)
    })
}

pub(super) fn has_array_element_constraints(
    name: &str,
    start: usize,
    len: usize,
    spec: &ResolvedSpec,
    ctx: &Ctx,
    array_ctx: &ArrayCtx,
) -> bool {
    if has_positional_array_bounds(name, start, len, spec, array_ctx) {
        return true;
    }
    spec.not_equal.iter().any(|(a, b)| {
        let other = if a == name {
            Some(b)
        } else if b == name {
            Some(a)
        } else {
            None
        };
        let Some(other) = other else { return false };
        ctx.contains_key(other)
            || array_ctx
                .get(other)
                .is_some_and(|xs| start < xs.len() && start.saturating_add(len) > 0)
    })
}

pub(super) fn has_any_pair_constraint(name: &str, spec: &ResolvedSpec) -> bool {
    spec.ordering
        .iter()
        .chain(spec.not_equal.iter())
        .any(|(a, b)| a == name || b == name)
}

pub(super) fn not_equal_forbidden_scalar(
    name: &str,
    spec: &ResolvedSpec,
    ctx: &Ctx,
    array_ctx: &ArrayCtx,
) -> HashSet<i64> {
    let mut forbidden = HashSet::new();
    for (a, b) in &spec.not_equal {
        let other = if a == name {
            Some(b)
        } else if b == name {
            Some(a)
        } else {
            None
        };
        let Some(other) = other else { continue };
        if let Some(&x) = ctx.get(other) {
            forbidden.insert(x);
        }
        if let Some(xs) = array_ctx.get(other) {
            forbidden.extend(xs.iter().copied());
        }
    }
    forbidden
}

fn not_equal_forbidden_element(
    name: &str,
    index: usize,
    spec: &ResolvedSpec,
    ctx: &Ctx,
    array_ctx: &ArrayCtx,
) -> HashSet<i64> {
    let mut forbidden = HashSet::new();
    for (a, b) in &spec.not_equal {
        let other = if a == name {
            Some(b)
        } else if b == name {
            Some(a)
        } else {
            None
        };
        let Some(other) = other else { continue };
        if let Some(&x) = ctx.get(other) {
            forbidden.insert(x);
        }
        if let Some(&x) = array_ctx.get(other).and_then(|xs| xs.get(index)) {
            forbidden.insert(x);
        }
    }
    forbidden
}

#[allow(clippy::too_many_arguments)]
pub(super) fn gen_int_array_with_positional_bounds(
    st: &CaseStrategy,
    name: &str,
    lo: i64,
    hi: i64,
    len: usize,
    values: Option<&[i64]>,
    distinct: bool,
    spec: &ResolvedSpec,
    ctx: &Ctx,
    array_ctx: &ArrayCtx,
    start: usize,
    rng: &mut impl Rng,
) -> Option<Vec<i64>> {
    let disable_zero_corner = has_any_pair_constraint(name, spec);
    if !has_array_element_constraints(name, start, len, spec, ctx, array_ctx) {
        if disable_zero_corner && is_array_shape_strategy(st) {
            return gen_int_array(
                &CaseStrategy::Random(RandomStrategy::Random),
                lo,
                hi,
                len,
                values,
                distinct,
                rng,
            );
        }
        return gen_int_array(st, lo, hi, len, values, distinct, rng);
    }

    let mut out = Vec::with_capacity(len);
    let mut used = HashSet::new();
    for offset in 0..len {
        let index = start + offset;
        let (elo, ehi) = narrow_element_bounds(name, index, lo, hi, spec, array_ctx)?;
        let forbidden = not_equal_forbidden_element(name, index, spec, ctx, array_ctx);
        let x = gen_positionally_bounded_int(
            st,
            offset,
            len,
            elo,
            ehi,
            values,
            distinct,
            &used,
            &forbidden,
            disable_zero_corner,
            rng,
        )?;
        if distinct {
            used.insert(x);
        }
        out.push(x);
    }
    Some(out)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn gen_positionally_bounded_int(
    st: &CaseStrategy,
    index: usize,
    len: usize,
    lo: i64,
    hi: i64,
    values: Option<&[i64]>,
    distinct: bool,
    used: &HashSet<i64>,
    forbidden: &HashSet<i64>,
    disable_zero_corner: bool,
    rng: &mut impl Rng,
) -> Option<i64> {
    if let Some(vs) = values.filter(|vs| !vs.is_empty()) {
        let mut domain: Vec<i64> = vs
            .iter()
            .copied()
            .filter(|&x| {
                lo <= x && x <= hi && !forbidden.contains(&x) && (!distinct || !used.contains(&x))
            })
            .collect();
        domain.sort_unstable();
        domain.dedup();
        return choose_from_domain(st, index, len, &domain, disable_zero_corner, rng);
    }

    if !distinct {
        if matches!(
            st,
            CaseStrategy::Deterministic(DeterministicStrategy::AllMax)
        ) {
            let mut x = hi;
            loop {
                if !forbidden.contains(&x) {
                    return Some(x);
                }
                if x == lo {
                    return None;
                }
                x -= 1;
            }
        }
        if matches!(
            st,
            CaseStrategy::Deterministic(DeterministicStrategy::AllMin)
        ) {
            let mut x = lo;
            loop {
                if !forbidden.contains(&x) {
                    return Some(x);
                }
                if x == hi {
                    return None;
                }
                x += 1;
            }
        }
        let candidate = match st {
            CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin) => {
                if index % 2 == 0 {
                    hi
                } else {
                    lo
                }
            }
            CaseStrategy::Random(RandomStrategy::ArrayOneMaxRestMin) => {
                if index + 1 == len {
                    hi
                } else {
                    lo
                }
            }
            CaseStrategy::Random(RandomStrategy::ZeroCorner)
                if !disable_zero_corner && lo <= 0 && 0 <= hi =>
            {
                0
            }
            _ => gen_int(lo, hi, rng),
        };
        if !forbidden.contains(&candidate) {
            return Some(candidate);
        }
        return bounded_distinct_int(st, lo, hi, &HashSet::new(), forbidden, rng);
    }

    bounded_distinct_int(st, lo, hi, used, forbidden, rng)
}

fn choose_from_domain(
    st: &CaseStrategy,
    index: usize,
    len: usize,
    domain: &[i64],
    disable_zero_corner: bool,
    rng: &mut impl Rng,
) -> Option<i64> {
    if domain.is_empty() {
        return None;
    }
    Some(match st {
        CaseStrategy::Deterministic(DeterministicStrategy::AllMax) => *domain.last().unwrap(),
        CaseStrategy::Deterministic(DeterministicStrategy::AllMin) => domain[0],
        CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin) => {
            if index % 2 == 0 {
                *domain.last().unwrap()
            } else {
                domain[0]
            }
        }
        CaseStrategy::Random(RandomStrategy::ArrayOneMaxRestMin) => {
            if index + 1 == len {
                *domain.last().unwrap()
            } else {
                domain[0]
            }
        }
        CaseStrategy::Random(RandomStrategy::ZeroCorner)
            if !disable_zero_corner && domain.binary_search(&0).is_ok() =>
        {
            0
        }
        _ => domain[rng.gen_range(0..domain.len())],
    })
}

pub(super) fn bounded_distinct_int(
    st: &CaseStrategy,
    lo: i64,
    hi: i64,
    used: &HashSet<i64>,
    forbidden: &HashSet<i64>,
    rng: &mut impl Rng,
) -> Option<i64> {
    let blocked_count = |lo: i64, hi: i64, used: &HashSet<i64>, forbidden: &HashSet<i64>| {
        used.union(forbidden)
            .filter(|&&x| lo <= x && x <= hi)
            .count()
    };
    let span = (hi as i128) - (lo as i128) + 1;
    if span <= blocked_count(lo, hi, used, forbidden) as i128 {
        return None;
    }
    let prefer_min = matches!(
        st,
        CaseStrategy::Deterministic(DeterministicStrategy::AllMin)
    );
    let prefer_max = matches!(
        st,
        CaseStrategy::Deterministic(DeterministicStrategy::AllMax)
    );
    if prefer_min || prefer_max {
        let mut x = if prefer_min { lo } else { hi };
        loop {
            if !used.contains(&x) && !forbidden.contains(&x) {
                return Some(x);
            }
            if prefer_min {
                if x == hi {
                    return None;
                }
                x += 1;
            } else {
                if x == lo {
                    return None;
                }
                x -= 1;
            }
        }
    }
    for _ in 0..1000 {
        let x = gen_int(lo, hi, rng);
        if !used.contains(&x) && !forbidden.contains(&x) {
            return Some(x);
        }
    }
    let mut x = lo;
    while x <= hi {
        if !used.contains(&x) && !forbidden.contains(&x) {
            return Some(x);
        }
        if x == i64::MAX {
            break;
        }
        x += 1;
    }
    None
}

fn is_array_shape_strategy(st: &CaseStrategy) -> bool {
    matches!(
        st,
        CaseStrategy::Random(
            RandomStrategy::ArrayMonoInc
                | RandomStrategy::ArrayMonoDec
                | RandomStrategy::ArrayAllSame
                | RandomStrategy::ArrayAltMaxMin
                | RandomStrategy::ArrayMountain
                | RandomStrategy::ArrayOneMaxRestMin
                | RandomStrategy::ArrayNarrowRange
                | RandomStrategy::ArrayPeriodic
                | RandomStrategy::ZeroCorner
        )
    )
}
