use super::parse::{BoundVal, ConstraintParsed, InputBlock, SizeRef, StringVarSpec, SumConstraint, VarBound};
use rand::{seq::{index::sample, SliceRandom}, Rng, SeedableRng};
use std::collections::{HashMap, HashSet};

// ── Case strategies ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum CaseStrategy {
    Random,
    AllMax,
    AllMin,
    SmallSize(i64),
    /// outer_var (T/Q)=1, inner_var=limit, other size vars=max, array elements/string chars=random.
    SumMaxSingle { inner_var: String, outer_var: Option<String>, limit: i64 },
    ArrayMonoInc,
    ArrayMonoDec,
    ArrayAllMax,
    ArrayAllMin,
    ArrayAllSame,
    ArrayAltMaxMin,
    ArrayMountain,
    ArrayOneMaxRestMin,
    /// Variables whose range spans zero (lo < 0 < hi) are set to 0.
    ZeroCorner,
}

fn collect_size_vars(blocks: &[InputBlock]) -> HashSet<String> {
    let mut vars = HashSet::new();
    for block in blocks {
        match block {
            InputBlock::Array1D { len, .. } => {
                if let SizeRef::Var(v) | SizeRef::VarOffset(v, _) = len { vars.insert(v.clone()); }
            }
            InputBlock::NRepeat { count, .. } | InputBlock::Vertical { count, .. } => {
                if let SizeRef::Var(v) | SizeRef::VarOffset(v, _) = count { vars.insert(v.clone()); }
            }
            InputBlock::Matrix { rows, cols, .. } => {
                if let SizeRef::Var(v) | SizeRef::VarOffset(v, _) = rows { vars.insert(v.clone()); }
                if let SizeRef::Var(v) | SizeRef::VarOffset(v, _) = cols { vars.insert(v.clone()); }
            }
            InputBlock::OuterRepeat { count, inner } => {
                if let SizeRef::Var(v) | SizeRef::VarOffset(v, _) = count { vars.insert(v.clone()); }
                vars.extend(collect_size_vars(inner));
            }
            InputBlock::TypedRepeat { count, branches } => {
                if let SizeRef::Var(v) | SizeRef::VarOffset(v, _) = count { vars.insert(v.clone()); }
                for b in branches { vars.extend(collect_size_vars(&b.inner)); }
            }
            _ => {}
        }
    }
    vars
}

fn has_array_blocks(blocks: &[InputBlock]) -> bool {
    blocks.iter().any(|b| match b {
        InputBlock::Array1D { .. } | InputBlock::NRepeat { .. } | InputBlock::Vertical { .. } | InputBlock::Matrix { .. } => true,
        InputBlock::OuterRepeat { inner, .. } => has_array_blocks(inner),
        InputBlock::TypedRepeat { branches, .. } => branches.iter().any(|b| has_array_blocks(&b.inner)),
        _ => false,
    })
}

pub(crate) fn make_strategy_list(
    blocks: &[InputBlock],
    sum_constraints: &[SumConstraint],
    count: u32,
) -> Vec<CaseStrategy> {
    let has_array = has_array_blocks(blocks);
    let n = count as usize;

    // Build corner pool
    let mut corners: Vec<CaseStrategy> = vec![
        CaseStrategy::AllMax,
        CaseStrategy::AllMin,
        CaseStrategy::SmallSize(1),
        CaseStrategy::SmallSize(2),
        CaseStrategy::SmallSize(3),
        CaseStrategy::ZeroCorner,
    ];
    let outer_var = blocks.iter().find_map(|b| {
        if let InputBlock::OuterRepeat { count: SizeRef::Var(v), .. } = b { Some(v.clone()) } else { None }
    });
    for sc in sum_constraints {
        corners.push(CaseStrategy::SumMaxSingle {
            inner_var: sc.inner_var.clone(),
            outer_var: outer_var.clone(),
            limit: sc.limit,
        });
    }
    if has_array {
        corners.extend([
            CaseStrategy::ArrayMonoInc,
            CaseStrategy::ArrayMonoDec,
            CaseStrategy::ArrayAllMax,
            CaseStrategy::ArrayAllMin,
            CaseStrategy::ArrayAllSame,
            CaseStrategy::ArrayAltMaxMin,
            CaseStrategy::ArrayMountain,
            CaseStrategy::ArrayOneMaxRestMin,
        ]);
    }

    // Initial random slots: 1 if count < 10, else 2
    let initial_random = if n < 10 { 1usize } else { 2usize }.min(n);
    let mut result: Vec<CaseStrategy> = vec![CaseStrategy::Random; initial_random];

    if n <= initial_random {
        return result;
    }

    // Shuffle corners for random ordering
    let mut rng = rand::rngs::SmallRng::from_entropy();
    corners.shuffle(&mut rng);

    // Fill remaining slots: cover all corners first, then 30% corner / 70% random
    let remaining = n - initial_random;
    let corner_count = corners.len();
    for i in 0..remaining {
        if i < corner_count {
            result.push(corners[i].clone());
        } else if rng.gen_bool(0.3) {
            let idx = rng.gen_range(0..corner_count);
            result.push(corners[idx].clone());
        } else {
            result.push(CaseStrategy::Random);
        }
    }

    result
}

// ── Random value generation ──────────────────────────────────────────────────

fn bound_lit_hi(name: &str, bounds: &HashMap<String, VarBound>) -> i64 {
    bound_lit_hi_depth(name, bounds, 0)
}

fn bound_lit_hi_depth(name: &str, bounds: &HashMap<String, VarBound>, depth: u8) -> i64 {
    if depth > 8 { return 1_000_000_000; }
    match bounds.get(name).map(|b| &b.hi) {
        Some(BoundVal::Lit(n)) => *n,
        Some(BoundVal::Var(v)) => bound_lit_hi_depth(v, bounds, depth + 1),
        Some(BoundVal::VarOffset(v, offset)) => bound_lit_hi_depth(v, bounds, depth + 1) + offset,
        Some(BoundVal::Set(vals)) => *vals.iter().max().unwrap_or(&1_000_000_000),
        _ => 1_000_000_000,
    }
}

fn resolve_bound(val: &BoundVal, ctx: &HashMap<String, i64>, bounds: &HashMap<String, VarBound>) -> i64 {
    match val {
        BoundVal::Lit(n) => *n,
        BoundVal::Var(name) => ctx.get(name).copied().unwrap_or_else(|| bound_lit_hi(name, bounds)),
        BoundVal::VarOffset(name, offset) => {
            ctx.get(name).copied().unwrap_or_else(|| bound_lit_hi(name, bounds)) + offset
        }
        BoundVal::Set(vals) => *vals.last().unwrap_or(&1_000_000_000),
    }
}

const MAX_ARRAY_SIZE: usize = 200_000;

fn resolve_size(size: &SizeRef, ctx: &HashMap<String, i64>) -> usize {
    let n = match size {
        SizeRef::Lit(n) => *n,
        SizeRef::Var(name) => ctx.get(name).copied().unwrap_or(10).max(0) as usize,
        SizeRef::VarOffset(name, offset) => {
            (ctx.get(name).copied().unwrap_or(10) + offset).max(0) as usize
        }
    };
    n.min(MAX_ARRAY_SIZE)
}

fn gen_val(var: &str, bounds: &HashMap<String, VarBound>, ctx: &HashMap<String, i64>, rng: &mut impl Rng) -> i64 {
    let default = VarBound::default();
    let bound = bounds.get(var).unwrap_or(&default);
    let lo = resolve_bound(&bound.lo, ctx, bounds);
    // For Set hi, pick from the set
    if let BoundVal::Set(vals) = &bound.hi {
        if !vals.is_empty() {
            let idx = rng.gen_range(0..vals.len());
            return vals[idx];
        }
    }
    let hi = resolve_bound(&bound.hi, ctx, bounds);
    let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
    if lo == hi { lo } else { rng.gen_range(lo..=hi) }
}

fn var_lo_hi(var: &str, bounds: &HashMap<String, VarBound>, ctx: &HashMap<String, i64>) -> (i64, i64) {
    let default = VarBound::default();
    let bound = bounds.get(var).unwrap_or(&default);
    let lo = resolve_bound(&bound.lo, ctx, bounds);
    let hi = resolve_bound(&bound.hi, ctx, bounds);
    if lo > hi { (hi, lo) } else { (lo, hi) }
}

fn gen_scalar(
    var: &str,
    bounds: &HashMap<String, VarBound>,
    ctx: &HashMap<String, i64>,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
    size_vars: &HashSet<String>,
    small_n: Option<i64>,
) -> i64 {
    if let Some(n) = small_n {
        if size_vars.contains(var) {
            return n;
        }
    }
    // For Set bounds, always sample from set regardless of strategy
    if let Some(b) = bounds.get(var) {
        if let BoundVal::Set(vals) = &b.hi {
            if !vals.is_empty() {
                return match strategy {
                    CaseStrategy::AllMax => *vals.iter().max().unwrap(),
                    CaseStrategy::AllMin => *vals.iter().min().unwrap(),
                    _ => {
                        let idx = rng.gen_range(0..vals.len());
                        vals[idx]
                    }
                };
            }
        }
    }
    // SumMaxSingle: outer_var (T/Q)=1, inner_var=limit, other size vars=max, others=random
    if let CaseStrategy::SumMaxSingle { inner_var, outer_var, limit } = strategy {
        if var == inner_var.as_str() {
            return (*limit).min(MAX_ARRAY_SIZE as i64);
        }
        if outer_var.as_deref() == Some(var) {
            return 1;
        }
        if size_vars.contains(var) {
            return var_lo_hi(var, bounds, ctx).1.min(MAX_ARRAY_SIZE as i64);
        }
        return gen_val(var, bounds, ctx, rng);
    }

    let val = match strategy {
        CaseStrategy::AllMax => var_lo_hi(var, bounds, ctx).1,
        CaseStrategy::AllMin => var_lo_hi(var, bounds, ctx).0,
        CaseStrategy::ZeroCorner => {
            let (lo, hi) = var_lo_hi(var, bounds, ctx);
            if lo < 0 && hi > 0 { 0 } else { gen_val(var, bounds, ctx, rng) }
        }
        _ => gen_val(var, bounds, ctx, rng),
    };
    // Cap size variables to MAX_ARRAY_SIZE so binaries don't try to allocate huge vecs
    if size_vars.contains(var) {
        val.min(MAX_ARRAY_SIZE as i64)
    } else {
        val
    }
}

fn gen_array_elem(
    var: &str,
    idx: usize,
    n: usize,
    bounds: &HashMap<String, VarBound>,
    ctx: &HashMap<String, i64>,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
) -> i64 {
    // For Set bounds, sample from set
    if let Some(b) = bounds.get(var) {
        if let BoundVal::Set(vals) = &b.hi {
            if !vals.is_empty() {
                return match strategy {
                    CaseStrategy::AllMax | CaseStrategy::ArrayAllMax => *vals.iter().max().unwrap(),
                    CaseStrategy::AllMin | CaseStrategy::ArrayAllMin => *vals.iter().min().unwrap(),
                    _ => {
                        let i = rng.gen_range(0..vals.len());
                        vals[i]
                    }
                };
            }
        }
    }

    let (lo, hi) = var_lo_hi(var, bounds, ctx);

    match strategy {
        CaseStrategy::AllMax | CaseStrategy::ArrayAllMax => hi,
        CaseStrategy::AllMin | CaseStrategy::ArrayAllMin => lo,
        CaseStrategy::ArrayMonoInc => {
            if n <= 1 { return lo; }
            lo + ((hi - lo) as f64 * idx as f64 / (n - 1) as f64) as i64
        }
        CaseStrategy::ArrayMonoDec => {
            if n <= 1 { return hi; }
            hi - ((hi - lo) as f64 * idx as f64 / (n - 1) as f64) as i64
        }
        CaseStrategy::ArrayAltMaxMin => {
            if idx % 2 == 0 { hi } else { lo }
        }
        CaseStrategy::ArrayMountain => {
            if n <= 1 { return (lo + hi) / 2; }
            let half = (n - 1) / 2;
            if idx <= half {
                lo + ((hi - lo) as f64 * idx as f64 / half.max(1) as f64) as i64
            } else {
                hi - ((hi - lo) as f64 * (idx - half) as f64 / (n - 1 - half).max(1) as f64) as i64
            }
        }
        CaseStrategy::ArrayOneMaxRestMin => {
            if idx == n / 2 { hi } else { lo }
        }
        CaseStrategy::ZeroCorner => {
            if lo < 0 && hi > 0 { 0 } else { rng.gen_range(lo..=hi) }
        }
        _ => gen_val(var, bounds, ctx, rng),
    }
}

fn gen_array(
    var: &str,
    n: usize,
    bounds: &HashMap<String, VarBound>,
    ctx: &HashMap<String, i64>,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
    distinct: bool,
) -> Vec<String> {
    if n == 0 {
        return vec![];
    }
    if distinct {
        return gen_distinct_array(var, n, bounds, ctx, rng, strategy);
    }
    if matches!(strategy, CaseStrategy::ArrayAllSame) {
        let val = gen_val(var, bounds, ctx, rng);
        return vec![val.to_string(); n];
    }
    (0..n)
        .map(|i| gen_array_elem(var, i, n, bounds, ctx, rng, strategy).to_string())
        .collect()
}

/// Generate n pairwise-distinct values in [lo, hi].
/// Falls back to non-distinct if the range is too small.
fn gen_distinct_array(
    var: &str,
    n: usize,
    bounds: &HashMap<String, VarBound>,
    ctx: &HashMap<String, i64>,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
) -> Vec<String> {
    let (lo, hi) = var_lo_hi(var, bounds, ctx);
    let range = (hi - lo).saturating_add(1).max(0) as usize;
    if range < n {
        // Range too small to guarantee distinct — fall back to normal generation
        return (0..n)
            .map(|i| gen_array_elem(var, i, n, bounds, ctx, rng, strategy).to_string())
            .collect();
    }
    let mut vals: Vec<i64> = match strategy {
        CaseStrategy::AllMax | CaseStrategy::ArrayAllMax => {
            // hi, hi-1, hi-2, ...
            (0..n as i64).map(|i| hi - i).collect()
        }
        CaseStrategy::AllMin | CaseStrategy::ArrayAllMin => {
            // lo, lo+1, lo+2, ...
            (0..n as i64).map(|i| lo + i).collect()
        }
        CaseStrategy::ArrayMonoInc => (0..n as i64).map(|i| lo + i).collect(),
        CaseStrategy::ArrayMonoDec => (0..n as i64).map(|i| hi - i).collect(),
        _ => {
            // Sample n distinct indices from [0, range) then map to [lo, hi]
            let indices = sample(rng, range, n);
            let mut v: Vec<i64> = indices.iter().map(|i| lo + i as i64).collect();
            // Shuffle so order isn't monotone
            use rand::seq::SliceRandom;
            v.shuffle(rng);
            v
        }
    };
    // Ensure strictly within [lo, hi] after arithmetic
    for x in &mut vals {
        *x = (*x).clamp(lo, hi);
    }
    vals.iter().map(|v| v.to_string()).collect()
}

/// Generate one string value.
/// `row_info` = `Some((row_idx, total_rows))` when called from a Vertical block
/// to produce monotone ordering across rows for Array* strategies.
fn gen_string(
    spec: &StringVarSpec,
    bounds: &HashMap<String, VarBound>,
    ctx: &HashMap<String, i64>,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
    row_info: Option<(usize, usize)>,
) -> String {
    let hi_len = resolve_bound(&spec.hi_len, ctx, bounds);
    let lo_len = resolve_bound(&spec.lo_len, ctx, bounds);
    let len = match strategy {
        CaseStrategy::AllMax | CaseStrategy::ArrayAllMax | CaseStrategy::SumMaxSingle { .. } => hi_len,
        CaseStrategy::AllMin | CaseStrategy::ArrayAllMin => lo_len,
        CaseStrategy::SmallSize(k) => (*k).clamp(lo_len, hi_len),
        _ => {
            let (lo, hi) = if lo_len > hi_len { (hi_len, lo_len) } else { (lo_len, hi_len) };
            if lo == hi { lo } else { rng.gen_range(lo..=hi) }
        }
    }.max(0) as usize;

    let chars = spec.charset.all_chars();
    if chars.is_empty() {
        return "a".repeat(len);
    }

    // Select the character (or character index) based on strategy.
    let pick_char = |i: usize| chars[i];
    match strategy {
        CaseStrategy::AllMax | CaseStrategy::ArrayAllMax => {
            pick_char(chars.len() - 1).to_string().repeat(len)
        }
        CaseStrategy::AllMin | CaseStrategy::ArrayAllMin => {
            pick_char(0).to_string().repeat(len)
        }
        CaseStrategy::ArrayMonoInc => {
            // Ascending: use char proportional to row position
            let ci = if let Some((row, total)) = row_info {
                if total <= 1 { 0 } else { row * (chars.len() - 1) / (total - 1) }
            } else { 0 };
            pick_char(ci.min(chars.len() - 1)).to_string().repeat(len)
        }
        CaseStrategy::ArrayMonoDec => {
            let ci = if let Some((row, total)) = row_info {
                if total <= 1 { chars.len() - 1 }
                else { (chars.len() - 1) - row * (chars.len() - 1) / (total - 1) }
            } else { chars.len() - 1 };
            pick_char(ci.min(chars.len() - 1)).to_string().repeat(len)
        }
        _ => (0..len).map(|_| chars[rng.gen_range(0..chars.len())]).collect(),
    }
}

/// Process a slice of blocks, appending generated lines and updating `ctx`.
fn process_blocks(
    blocks: &[InputBlock],
    bounds: &HashMap<String, VarBound>,
    string_vars: &HashMap<String, StringVarSpec>,
    size_vars: &HashSet<String>,
    all_distinct: &HashSet<String>,
    ctx: &mut HashMap<String, i64>,
    lines: &mut Vec<String>,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
    small_n: Option<i64>,
) {
    for block in blocks {
        match block {
            InputBlock::Scalars(vars) => {
                let mut parts: Vec<String> = Vec::new();
                for v in vars {
                    if let Some(spec) = string_vars.get(v.as_str()) {
                        parts.push(gen_string(spec, bounds, ctx, rng, strategy, None));
                    } else {
                        let val = gen_scalar(v, bounds, ctx, rng, strategy, size_vars, small_n);
                        ctx.insert(v.clone(), val);
                        parts.push(val.to_string());
                    }
                }
                lines.push(parts.join(" "));
            }
            InputBlock::Array1D { base, len } => {
                let n = resolve_size(len, ctx);
                if let Some(spec) = string_vars.get(base.as_str()) {
                    let vals: Vec<String> = (0..n)
                        .map(|_| gen_string(spec, bounds, ctx, rng, strategy, None))
                        .collect();
                    lines.push(vals.join(" "));
                } else {
                    let vals = gen_array(base, n, bounds, ctx, rng, strategy, all_distinct.contains(base.as_str()));
                    lines.push(vals.join(" "));
                }
            }
            InputBlock::NRepeat { cols, count } => {
                let n = resolve_size(count, ctx);
                if matches!(strategy, CaseStrategy::ArrayAllSame) {
                    let same_vals: Vec<String> = cols.iter().map(|c| {
                        if let Some(spec) = string_vars.get(c.as_str()) {
                            gen_string(spec, bounds, ctx, rng, strategy, None)
                        } else {
                            gen_val(c, bounds, ctx, rng).to_string()
                        }
                    }).collect();
                    for _ in 0..n {
                        lines.push(same_vals.join(" "));
                    }
                } else {
                    for row_idx in 0..n {
                        let vals: Vec<String> = cols.iter().map(|c| {
                            if let Some(spec) = string_vars.get(c.as_str()) {
                                gen_string(spec, bounds, ctx, rng, strategy, None)
                            } else {
                                gen_array_elem(c, row_idx, n, bounds, ctx, rng, strategy).to_string()
                            }
                        }).collect();
                        lines.push(vals.join(" "));
                    }
                }
            }
            InputBlock::Vertical { base, count, width } => {
                let n = resolve_size(count, ctx);
                if let Some(spec) = string_vars.get(base.as_str()) {
                    // If the block has an explicit width (grid row), override the spec length.
                    let effective_spec;
                    let spec = if let Some(w) = width {
                        let w_val = resolve_size(w, ctx).max(1) as i64;
                        effective_spec = super::parse::StringVarSpec {
                            charset: spec.charset.clone(),
                            lo_len: super::parse::BoundVal::Lit(w_val),
                            hi_len: super::parse::BoundVal::Lit(w_val),
                        };
                        &effective_spec
                    } else {
                        spec
                    };
                    for i in 0..n {
                        lines.push(gen_string(spec, bounds, ctx, rng, strategy, Some((i, n))));
                    }
                } else {
                    for v in gen_array(base, n, bounds, ctx, rng, strategy, all_distinct.contains(base.as_str())) {
                        lines.push(v);
                    }
                }
            }
            InputBlock::Matrix { base, rows, cols } => {
                let nrows = resolve_size(rows, ctx);
                let ncols = resolve_size(cols, ctx).max(1);
                for _ in 0..nrows {
                    let row: Vec<String> = gen_array(base, ncols, bounds, ctx, rng, strategy, false);
                    lines.push(row.join(" "));
                }
            }
            InputBlock::TypedRepeat { count, branches } => {
                let n = resolve_size(count, ctx);
                for _ in 0..n {
                    let bi = rng.gen_range(0..branches.len());
                    let branch = &branches[bi];
                    let mut inner_lines: Vec<String> = Vec::new();
                    process_blocks(
                        &branch.inner, bounds, string_vars, size_vars, all_distinct,
                        ctx, &mut inner_lines, rng, strategy, small_n,
                    );
                    // Merge type token + inner output onto as few lines as possible.
                    // Typically inner is one Scalars block → one line.
                    if inner_lines.is_empty() {
                        lines.push(branch.type_val.clone());
                    } else {
                        let first = inner_lines.remove(0);
                        if first.trim().is_empty() {
                            lines.push(branch.type_val.clone());
                        } else {
                            lines.push(format!("{} {}", branch.type_val, first.trim()));
                        }
                        lines.extend(inner_lines);
                    }
                }
            }
            InputBlock::OuterRepeat { count, inner } => {
                let t = resolve_size(count, ctx);
                let inner_size_vars = collect_size_vars(inner);
                let mut sums: HashMap<String, i64> = HashMap::new();
                for _ in 0..t {
                    let mut local_ctx = ctx.clone();
                    process_blocks(
                        inner, bounds, string_vars, &inner_size_vars, all_distinct,
                        &mut local_ctx, lines, rng, strategy, small_n,
                    );
                    // Accumulate values of variables newly introduced in inner blocks.
                    for (k, v) in &local_ctx {
                        if !ctx.contains_key(k) {
                            *sums.entry(k.clone()).or_insert(0) += v;
                        }
                    }
                }
                // Store accumulated sums in outer ctx for post-generation sum checking.
                for (var, sum) in sums {
                    ctx.insert(format!("__sum_{}", var), sum);
                }
            }
            InputBlock::Unsupported(_) => {}
        }
    }
}

fn generate_once(
    blocks: &[InputBlock],
    parsed: &ConstraintParsed,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
) -> (HashMap<String, i64>, String) {
    let bounds = &parsed.bounds;
    let string_vars = &parsed.string_vars;
    let size_vars = collect_size_vars(blocks);
    let small_n = if let CaseStrategy::SmallSize(n) = strategy { Some(*n) } else { None };
    let mut ctx: HashMap<String, i64> = HashMap::new();
    let mut lines: Vec<String> = Vec::new();

    process_blocks(blocks, bounds, string_vars, &size_vars, &parsed.all_distinct, &mut ctx, &mut lines, rng, strategy, small_n);

    (ctx, lines.join("\n") + "\n")
}

/// Generate a random input satisfying ordering, ≠, and sum constraints.
///
/// Returns `None` if this is a corner-case strategy and the sum constraint cannot be satisfied
/// within 20 retries — the caller should skip the test case entirely but still count it as
/// consumed.  For Random, retries are unlimited so `Some` is always returned eventually.
pub(crate) fn generate_random_input(
    blocks: &[InputBlock],
    parsed: &ConstraintParsed,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
) -> Option<String> {
    let is_corner = !matches!(strategy, CaseStrategy::Random);
    let mut retries = 0u32;
    loop {
        let (ctx, input) = generate_once(blocks, parsed, rng, strategy);
        let order_ok = parsed.var_to_var.iter().all(|(lo, hi)| {
            ctx.get(lo).copied().unwrap_or(i64::MIN) <= ctx.get(hi).copied().unwrap_or(i64::MAX)
        });
        let neq_ok = parsed.var_not_eq.iter().all(|(a, b)| {
            ctx.get(a).copied() != ctx.get(b).copied()
                || !ctx.contains_key(a) || !ctx.contains_key(b)
        });
        let sum_ok = parsed.sum_constraints.iter().all(|sc| {
            let key = format!("__sum_{}", sc.inner_var);
            ctx.get(&key).copied().unwrap_or(0) <= sc.limit
        });
        if order_ok && neq_ok && sum_ok {
            return Some(input);
        }
        retries += 1;
        if is_corner && retries >= 20 {
            return None;
        }
    }
}
