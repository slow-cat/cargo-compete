use super::parse::{BoundVal, ConstraintParsed, InputBlock, SizeRef, StringVarSpec, SumConstraint, VarBound, VarType};
use rand::{seq::{index::sample, SliceRandom}, Rng, SeedableRng};
use std::collections::{HashMap, HashSet};

pub(crate) type SumVar = (String, i64);

// ── Case strategies ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum DeterministicStrategy {
    AllMax,
    AllMin,
}

#[derive(Debug, Clone)]
pub(crate) enum RandomStrategy {
    Random,
    SmallSize(i64),
    /// outer_var (T/Q)=1, each inner_var=its limit, other size vars=max, array elements/string chars=random.
    SumMaxSingle { inner_vars: Vec<SumVar>, outer_var: Option<String> },
    ArrayMonoInc,
    ArrayMonoDec,
    ArrayAllSame,
    ArrayAltMaxMin,
    ArrayMountain,
    ArrayOneMaxRestMin,
    /// Each element is randomly one of two consecutive values chosen from [lo, hi].
    ArrayNarrowRange,
    /// Array elements follow a random repeating pattern of 2–5 values.
    ArrayPeriodic,
    /// Variables whose range spans zero (lo < 0 < hi) are set to 0.
    ZeroCorner,
}

#[derive(Debug, Clone)]
pub(crate) enum CaseStrategy {
    Deterministic(DeterministicStrategy),
    Random(RandomStrategy),
}

#[allow(clippy::collapsible_match)]
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
            InputBlock::OuterRepeat { count, inner, .. } => {
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

fn collect_has_i64(blocks: &[InputBlock]) -> bool {
    blocks.iter().any(|b| match b {
        InputBlock::Scalars(vars) => vars.iter().any(|(_, ty)| *ty == VarType::I64),
        InputBlock::Array1D { ty, .. } | InputBlock::Vertical { ty, .. } | InputBlock::Matrix { ty, .. } => *ty == VarType::I64,
        InputBlock::NRepeat { cols, .. } => cols.iter().any(|(_, ty)| *ty == VarType::I64),
        InputBlock::OuterRepeat { inner, .. } => collect_has_i64(inner),
        InputBlock::TypedRepeat { branches, .. } => branches.iter().any(|b| collect_has_i64(&b.inner)),
        _ => false,
    })
}

/// Return the sum_constraints and outer_var from the first OuterRepeat block found.
fn collect_outer_repeat_info(blocks: &[InputBlock]) -> (Vec<SumConstraint>, Option<String>) {
    for b in blocks {
        if let InputBlock::OuterRepeat { count, sum_constraints, .. } = b {
            let outer_var = if let SizeRef::Var(v) = count { Some(v.clone()) } else { None };
            return (sum_constraints.clone(), outer_var);
        }
    }
    (vec![], None)
}

pub(crate) fn make_strategy_list(blocks: &[InputBlock], count: u32) -> Vec<CaseStrategy> {
    let has_array = has_array_blocks(blocks);
    let has_i64 = collect_has_i64(blocks);
    let (sum_constraints, outer_var) = collect_outer_repeat_info(blocks);
    let n = count as usize;

    // Build corner pool
    let mut corners: Vec<CaseStrategy> = vec![
        CaseStrategy::Deterministic(DeterministicStrategy::AllMax),
        CaseStrategy::Deterministic(DeterministicStrategy::AllMin),
        CaseStrategy::Random(RandomStrategy::SmallSize(1)),
        CaseStrategy::Random(RandomStrategy::SmallSize(2)),
        CaseStrategy::Random(RandomStrategy::SmallSize(3)),
    ];
    if has_i64 {
        corners.push(CaseStrategy::Random(RandomStrategy::ZeroCorner));
    }
    if !sum_constraints.is_empty() {
        corners.push(CaseStrategy::Random(RandomStrategy::SumMaxSingle {
            inner_vars: sum_constraints.iter().map(|sc| (sc.inner_var.clone(), sc.limit.min(sc.inner_var_hi))).collect(),
            outer_var: outer_var.clone(),
        }));
    }
    if has_array {
        corners.extend([
            CaseStrategy::Random(RandomStrategy::ArrayMonoInc),
            CaseStrategy::Random(RandomStrategy::ArrayMonoDec),
            CaseStrategy::Random(RandomStrategy::ArrayAllSame),
            CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin),
            CaseStrategy::Random(RandomStrategy::ArrayMountain),
            CaseStrategy::Random(RandomStrategy::ArrayOneMaxRestMin),
            CaseStrategy::Random(RandomStrategy::ArrayNarrowRange),
            CaseStrategy::Random(RandomStrategy::ArrayPeriodic),
        ]);
    }

    let initial_random = if n < 10 { 1usize } else { 2usize }.min(n);
    let corner_count = corners.len();
    let mut rng = rand::rngs::SmallRng::from_entropy();

    // Shuffle corner pool so each run assigns corners in a random order (no fixed sequence)
    corners.shuffle(&mut rng);

    // Only random strategies can be repeated after full coverage (deterministic ones produce identical output)
    let random_corners: Vec<RandomStrategy> = corners.iter()
        .filter_map(|s| if let CaseStrategy::Random(r) = s { Some(r.clone()) } else { None })
        .collect();

    let mut result: Vec<CaseStrategy> = vec![CaseStrategy::Random(RandomStrategy::Random); initial_random];
    if n <= initial_random {
        return result;
    }

    // Before full coverage: draw corners one by one without duplicates (shuffled above)
    // After full coverage: 30% random-element corner / 70% random mix
    let remaining = n - initial_random;
    let corner_take = corner_count.min(remaining);
    for item in corners.iter().take(corner_take) {
        result.push(item.clone());
    }
    for _ in corner_take..remaining {
        if rng.gen_bool(0.3) && !random_corners.is_empty() {
            let idx = rng.gen_range(0..random_corners.len());
            result.push(CaseStrategy::Random(random_corners[idx].clone()));
        } else {
            result.push(CaseStrategy::Random(RandomStrategy::Random));
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
            // For lo: use Lit directly; for Var/VarOffset, resolve from ctx only if already set,
            // otherwise fall back to i64::MIN (no lo clamp) to avoid premature large fallback
            // when a dependency variable hasn't been generated yet.
            let lo = match bounds.get(var).map(|b| &b.lo) {
                Some(BoundVal::Lit(l)) => *l,
                Some(BoundVal::Var(v)) => ctx.get(v.as_str()).copied().unwrap_or(i64::MIN),
                Some(BoundVal::VarOffset(v, offset)) => {
                    ctx.get(v.as_str()).map(|&x| x + offset).unwrap_or(i64::MIN)
                }
                _ => i64::MIN,
            };
            let (_, hi) = var_lo_hi(var, bounds, ctx);
            return n.clamp(lo, hi);
        }
    }
    // For Set bounds, always sample from set regardless of strategy
    if let Some(b) = bounds.get(var) {
        if let BoundVal::Set(vals) = &b.hi {
            if !vals.is_empty() {
                return match strategy {
                    CaseStrategy::Deterministic(DeterministicStrategy::AllMax) => {
                        *vals.iter().max().expect("guarded by non-empty check")
                    }
                    CaseStrategy::Deterministic(DeterministicStrategy::AllMin) => {
                        *vals.iter().min().expect("guarded by non-empty check")
                    }
                    _ => {
                        let idx = rng.gen_range(0..vals.len());
                        vals[idx]
                    }
                };
            }
        }
    }
    // SumMaxSingle: outer_var=1, each inner_var=its limit, other size vars=max, others=random
    if let CaseStrategy::Random(RandomStrategy::SumMaxSingle { inner_vars, outer_var }) = strategy {
        for (inner_var, limit) in inner_vars {
            if var == inner_var.as_str() {
                return (*limit).min(MAX_ARRAY_SIZE as i64);
            }
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
        CaseStrategy::Deterministic(DeterministicStrategy::AllMax) => var_lo_hi(var, bounds, ctx).1,
        CaseStrategy::Deterministic(DeterministicStrategy::AllMin) => var_lo_hi(var, bounds, ctx).0,
        CaseStrategy::Random(RandomStrategy::ZeroCorner) => {
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
    array_len: usize,
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
                    CaseStrategy::Deterministic(DeterministicStrategy::AllMax) => {
                        *vals.iter().max().expect("guarded by non-empty check")
                    }
                    CaseStrategy::Deterministic(DeterministicStrategy::AllMin) => {
                        *vals.iter().min().expect("guarded by non-empty check")
                    }
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
        CaseStrategy::Deterministic(DeterministicStrategy::AllMax) => hi,
        CaseStrategy::Deterministic(DeterministicStrategy::AllMin) => lo,
        CaseStrategy::Random(RandomStrategy::ArrayMonoInc) => {
            if array_len <= 1 { return lo; }
            lo + ((hi - lo) as f64 * idx as f64 / (array_len - 1) as f64) as i64
        }
        CaseStrategy::Random(RandomStrategy::ArrayMonoDec) => {
            if array_len <= 1 { return hi; }
            hi - ((hi - lo) as f64 * idx as f64 / (array_len - 1) as f64) as i64
        }
        CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin) => {
            if idx % 2 == 0 { hi } else { lo }
        }
        CaseStrategy::Random(RandomStrategy::ArrayMountain) => {
            if array_len <= 1 { return (lo + hi) / 2; }
            let half = (array_len - 1) / 2;
            if idx <= half {
                lo + ((hi - lo) as f64 * idx as f64 / half.max(1) as f64) as i64
            } else {
                hi - ((hi - lo) as f64 * (idx - half) as f64 / (array_len - 1 - half).max(1) as f64) as i64
            }
        }
        CaseStrategy::Random(RandomStrategy::ArrayOneMaxRestMin) => {
            if idx == array_len / 2 { hi } else { lo }
        }
        CaseStrategy::Random(RandomStrategy::ZeroCorner) => {
            if lo < 0 && hi > 0 { 0 } else { rng.gen_range(lo..=hi) }
        }
        _ => gen_val(var, bounds, ctx, rng),
    }
}

fn gen_array(
    var: &str,
    array_len: usize,
    bounds: &HashMap<String, VarBound>,
    ctx: &HashMap<String, i64>,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
    distinct: bool,
) -> Vec<String> {
    if array_len == 0 {
        return vec![];
    }
    if distinct {
        return gen_distinct_array(var, array_len, bounds, ctx, rng, strategy);
    }
    if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayAllSame)) {
        let val = gen_val(var, bounds, ctx, rng);
        return vec![val.to_string(); array_len];
    }
    if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin)) {
        let (lo, hi) = var_lo_hi(var, bounds, ctx);
        let phase: usize = rng.gen_range(0..2);
        return (0..array_len).map(|i| if (i + phase) % 2 == 0 { hi } else { lo }).map(|v| v.to_string()).collect();
    }
    if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayNarrowRange)) {
        let (lo, hi) = var_lo_hi(var, bounds, ctx);
        if hi > lo {
            let base = rng.gen_range(lo..hi);
            return (0..array_len).map(|_| (if rng.gen_bool(0.5) { base } else { base + 1 }).to_string()).collect();
        }
        // range too narrow (lo==hi): fall through to per-element generation
    }
    if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayPeriodic)) {
        let period_len = rng.gen_range(2..=(5_usize).min(array_len.max(2)));
        let (lo, hi) = var_lo_hi(var, bounds, ctx);
        let period: Vec<i64> = (0..period_len)
            .map(|_| if lo == hi { lo } else { rng.gen_range(lo..=hi) })
            .collect();
        return (0..array_len).map(|i| period[i % period_len].to_string()).collect();
    }
    (0..array_len)
        .map(|i| gen_array_elem(var, i, array_len, bounds, ctx, rng, strategy).to_string())
        .collect()
}

/// Generate n pairwise-distinct values in [lo, hi].
/// Falls back to non-distinct if the range is too small.
fn gen_distinct_array(
    var: &str,
    array_len: usize,
    bounds: &HashMap<String, VarBound>,
    ctx: &HashMap<String, i64>,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
) -> Vec<String> {
    let (lo, hi) = var_lo_hi(var, bounds, ctx);
    let range = (hi - lo).saturating_add(1).max(0) as usize;
    if range < array_len {
        // Range too small to guarantee distinct — fall back to normal generation
        return (0..array_len)
            .map(|i| gen_array_elem(var, i, array_len, bounds, ctx, rng, strategy).to_string())
            .collect();
    }
    let mut vals: Vec<i64> = match strategy {
        CaseStrategy::Deterministic(DeterministicStrategy::AllMax) => {
            // hi, hi-1, hi-2, ...
            (0..array_len as i64).map(|i| hi - i).collect()
        }
        CaseStrategy::Deterministic(DeterministicStrategy::AllMin) => {
            // lo, lo+1, lo+2, ...
            (0..array_len as i64).map(|i| lo + i).collect()
        }
        CaseStrategy::Random(RandomStrategy::ArrayMonoInc) => (0..array_len as i64).map(|i| lo + i).collect(),
        CaseStrategy::Random(RandomStrategy::ArrayMonoDec) => (0..array_len as i64).map(|i| hi - i).collect(),
        _ => {
            // Sample array_len distinct indices from [0, range) then map to [lo, hi]
            let indices = sample(rng, range, array_len);
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
    vals.iter().map(|val| val.to_string()).collect()
}

/// Generate one string value.
/// `row_info` = `Some((row_idx, total_rows, phase))` when called from a Vertical/Array block.
/// `phase` is used by ArrayAltMaxMin checkerboard to randomise the starting cell.
fn gen_string(
    spec: &StringVarSpec,
    bounds: &HashMap<String, VarBound>,
    ctx: &HashMap<String, i64>,
    rng: &mut impl Rng,
    strategy: &CaseStrategy,
    row_info: Option<(usize, usize, usize)>,
) -> String {
    let hi_len = resolve_bound(&spec.hi_len, ctx, bounds);
    let lo_len = resolve_bound(&spec.lo_len, ctx, bounds);
    let len = match strategy {
        CaseStrategy::Deterministic(DeterministicStrategy::AllMax)
        | CaseStrategy::Random(RandomStrategy::SumMaxSingle { .. }) => hi_len,
        CaseStrategy::Deterministic(DeterministicStrategy::AllMin) => lo_len,
        CaseStrategy::Random(RandomStrategy::SmallSize(k)) => (*k).clamp(lo_len, hi_len),
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
        CaseStrategy::Deterministic(DeterministicStrategy::AllMax) => {
            pick_char(chars.len() - 1).to_string().repeat(len)
        }
        CaseStrategy::Deterministic(DeterministicStrategy::AllMin) => {
            pick_char(0).to_string().repeat(len)
        }
        CaseStrategy::Random(RandomStrategy::ArrayAllSame) => {
            let char_idx = rng.gen_range(0..chars.len());
            pick_char(char_idx).to_string().repeat(len)
        }
        CaseStrategy::Random(RandomStrategy::ArrayMonoInc) => {
            let char_idx = if let Some((row, total, _)) = row_info {
                if total <= 1 { 0 } else { row * (chars.len() - 1) / (total - 1) }
            } else { 0 };
            pick_char(char_idx.min(chars.len() - 1)).to_string().repeat(len)
        }
        CaseStrategy::Random(RandomStrategy::ArrayMonoDec) => {
            let char_idx = if let Some((row, total, _)) = row_info {
                if total <= 1 { chars.len() - 1 }
                else { (chars.len() - 1) - row * (chars.len() - 1) / (total - 1) }
            } else { chars.len() - 1 };
            pick_char(char_idx.min(chars.len() - 1)).to_string().repeat(len)
        }
        CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin) => {
            if let Some((row, _, phase)) = row_info {
                // Checkerboard: each cell (row, col) flips between last/first char
                (0..len).map(|j| {
                    if (row + j + phase) % 2 == 0 { chars[chars.len() - 1] } else { chars[0] }
                }).collect()
            } else {
                // Single string: alternate within the string
                (0..len).map(|j| if j % 2 == 0 { chars[chars.len() - 1] } else { chars[0] }).collect()
            }
        }
        CaseStrategy::Random(RandomStrategy::ArrayMountain) => {
            let char_idx = if let Some((row, total, _)) = row_info {
                if total <= 1 { (chars.len() - 1) / 2 }
                else {
                    let half = (total - 1) / 2;
                    if row <= half {
                        row * (chars.len() - 1) / half.max(1)
                    } else {
                        (chars.len() - 1).saturating_sub(
                            (row - half) * (chars.len() - 1) / (total - 1 - half).max(1)
                        )
                    }
                }
            } else { (chars.len() - 1) / 2 };
            pick_char(char_idx.min(chars.len() - 1)).to_string().repeat(len)
        }
        CaseStrategy::Random(RandomStrategy::ArrayOneMaxRestMin) => {
            let char_idx = if let Some((row, total, _)) = row_info {
                if row == total / 2 { chars.len() - 1 } else { 0 }
            } else { chars.len() - 1 };
            pick_char(char_idx).to_string().repeat(len)
        }
        CaseStrategy::Random(RandomStrategy::ArrayNarrowRange) => {
            if chars.len() >= 2 {
                let base_char_idx = rng.gen_range(0..chars.len() - 1);
                (0..len).map(|_| if rng.gen_bool(0.5) { chars[base_char_idx] } else { chars[base_char_idx + 1] }).collect()
            } else {
                pick_char(0).to_string().repeat(len)
            }
        }
        CaseStrategy::Random(RandomStrategy::ArrayPeriodic) => {
            if len == 0 { return String::new(); }
            let period_len = rng.gen_range(2..=(5_usize).min(len.max(2)));
            let period: Vec<char> = (0..period_len).map(|_| chars[rng.gen_range(0..chars.len())]).collect();
            (0..len).map(|i| period[i % period_len]).collect()
        }
        _ => (0..len).map(|_| chars[rng.gen_range(0..chars.len())]).collect(),
    }
}

/// Process a slice of blocks, appending generated lines and updating `ctx`.
#[allow(clippy::too_many_arguments)]
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
                for (v, _) in vars {
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
            InputBlock::Array1D { base, len, .. } => {
                let n = resolve_size(len, ctx);
                if let Some(spec) = string_vars.get(base.as_str()) {
                    let vals: Vec<String> = if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayAllSame)) {
                        let s = gen_string(spec, bounds, ctx, rng, strategy, None);
                        vec![s; n]
                    } else if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin)) {
                        let phase: usize = rng.gen_range(0..2);
                        (0..n).map(|i| gen_string(spec, bounds, ctx, rng, strategy, Some((i, n, phase)))).collect()
                    } else {
                        (0..n).map(|i| gen_string(spec, bounds, ctx, rng, strategy, Some((i, n, 0)))).collect()
                    };
                    lines.push(vals.join(" "));
                } else {
                    let vals = gen_array(base, n, bounds, ctx, rng, strategy, all_distinct.contains(base.as_str()));
                    lines.push(vals.join(" "));
                }
            }
            InputBlock::NRepeat { cols, count } => {
                let n = resolve_size(count, ctx);
                if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayAllSame)) {
                    let same_vals: Vec<String> = cols.iter().map(|(c, _)| {
                        if let Some(spec) = string_vars.get(c.as_str()) {
                            gen_string(spec, bounds, ctx, rng, strategy, None)
                        } else {
                            gen_val(c, bounds, ctx, rng).to_string()
                        }
                    }).collect();
                    for _ in 0..n {
                        lines.push(same_vals.join(" "));
                    }
                } else if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin)) {
                    let phase: usize = rng.gen_range(0..2);
                    for row_idx in 0..n {
                        let vals: Vec<String> = cols.iter().map(|(c, _)| {
                            if let Some(spec) = string_vars.get(c.as_str()) {
                                gen_string(spec, bounds, ctx, rng, strategy, Some((row_idx, n, phase)))
                            } else {
                                let (lo, hi) = var_lo_hi(c, bounds, ctx);
                                (if (row_idx + phase) % 2 == 0 { hi } else { lo }).to_string()
                            }
                        }).collect();
                        lines.push(vals.join(" "));
                    }
                } else if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayNarrowRange)) {
                    // Pre-generate a base value per integer column so each column uses a consistent pair
                    let col_bases: Vec<Option<i64>> = cols.iter().map(|(c, _)| {
                        if string_vars.contains_key(c.as_str()) { return None; }
                        let (lo, hi) = var_lo_hi(c, bounds, ctx);
                        if hi > lo { Some(rng.gen_range(lo..hi)) } else { None }
                    }).collect();
                    for row_idx in 0..n {
                        let vals: Vec<String> = cols.iter().enumerate().map(|(ci, (c, _))| {
                            if let Some(spec) = string_vars.get(c.as_str()) {
                                gen_string(spec, bounds, ctx, rng, strategy, Some((row_idx, n, 0)))
                            } else {
                                match col_bases[ci] {
                                    Some(base) => (if rng.gen_bool(0.5) { base } else { base + 1 }).to_string(),
                                    None => gen_val(c, bounds, ctx, rng).to_string(),
                                }
                            }
                        }).collect();
                        lines.push(vals.join(" "));
                    }
                } else if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayPeriodic)) {
                    // Pre-generate a period per integer column
                    let col_periods: Vec<Option<Vec<i64>>> = cols.iter().map(|(c, _)| {
                        if string_vars.contains_key(c.as_str()) { return None; }
                        let period_len = rng.gen_range(2..=(5_usize).min(n.max(2)));
                        let (lo, hi) = var_lo_hi(c, bounds, ctx);
                        Some((0..period_len).map(|_| if lo == hi { lo } else { rng.gen_range(lo..=hi) }).collect())
                    }).collect();
                    for row_idx in 0..n {
                        let vals: Vec<String> = cols.iter().enumerate().map(|(ci, (c, _))| {
                            if let Some(spec) = string_vars.get(c.as_str()) {
                                gen_string(spec, bounds, ctx, rng, strategy, Some((row_idx, n, 0)))
                            } else {
                                match &col_periods[ci] {
                                    Some(period) => period[row_idx % period.len()].to_string(),
                                    None => gen_val(c, bounds, ctx, rng).to_string(),
                                }
                            }
                        }).collect();
                        lines.push(vals.join(" "));
                    }
                } else {
                    for row_idx in 0..n {
                        let vals: Vec<String> = cols.iter().map(|(c, _)| {
                            if let Some(spec) = string_vars.get(c.as_str()) {
                                gen_string(spec, bounds, ctx, rng, strategy, Some((row_idx, n, 0)))
                            } else {
                                gen_array_elem(c, row_idx, n, bounds, ctx, rng, strategy).to_string()
                            }
                        }).collect();
                        lines.push(vals.join(" "));
                    }
                }
            }
            InputBlock::Vertical { base, count, width, .. } => {
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
                    if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayAllSame)) {
                        let row = gen_string(spec, bounds, ctx, rng, strategy, None);
                        for _ in 0..n {
                            lines.push(row.clone());
                        }
                    } else {
                        // For checkerboard, pre-generate the phase so all rows share it
                        let phase = if matches!(strategy, CaseStrategy::Random(RandomStrategy::ArrayAltMaxMin)) {
                            rng.gen_range(0..2)
                        } else {
                            0
                        };
                        for i in 0..n {
                            lines.push(gen_string(spec, bounds, ctx, rng, strategy, Some((i, n, phase))));
                        }
                    }
                } else {
                    for val in gen_array(base, n, bounds, ctx, rng, strategy, all_distinct.contains(base.as_str())) {
                        lines.push(val);
                    }
                }
            }
            InputBlock::Matrix { base, rows, cols, .. } => {
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
                    let branch_idx = rng.gen_range(0..branches.len());
                    let branch = &branches[branch_idx];
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
            InputBlock::OuterRepeat { count, inner, sum_constraints: block_scs } => {
                let repeat_count = resolve_size(count, ctx);
                let inner_size_vars = collect_size_vars(inner);
                let mut sums: HashMap<String, i64> = HashMap::new();
                let has_sum_inner = repeat_count > 1
                    && block_scs.iter().any(|sc| inner_size_vars.contains(&sc.inner_var));
                // Flat per-iteration cap for all strategies: hi = floor(limit/T_actual).
                // T_actual * floor(limit/T_actual) ≤ limit, so sum constraint is always satisfied
                // and each iteration independently samples N in [lo, per_iter_hi].
                let flat_bounds: Option<HashMap<String, VarBound>> = if has_sum_inner {
                    let mut b = bounds.clone();
                    for sc in block_scs {
                        if inner_size_vars.contains(&sc.inner_var) {
                            let per_iter_hi = (sc.limit / repeat_count as i64).max(1).min(sc.inner_var_hi);
                            let entry = b.entry(sc.inner_var.clone()).or_default();
                            entry.hi = BoundVal::Lit(per_iter_hi);
                        }
                    }
                    Some(b)
                } else {
                    None
                };
                for _ in 0..repeat_count {
                    let mut local_ctx = ctx.clone();
                    let effective = flat_bounds.as_ref()
                        .map_or(bounds as &HashMap<String, VarBound>, |b| b);
                    process_blocks(
                        inner, effective, string_vars, &inner_size_vars, all_distinct,
                        &mut local_ctx, lines, rng, strategy, small_n,
                    );
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
    let small_n = if let CaseStrategy::Random(RandomStrategy::SmallSize(n)) = strategy { Some(*n) } else { None };
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
    let is_corner = !matches!(strategy, CaseStrategy::Random(RandomStrategy::Random));
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
        let bounds_ok = ctx.iter()
            .filter(|(k, _)| !k.starts_with("__") && parsed.bounds.contains_key(k.as_str()))
            .all(|(var, &val)| {
                let (lo, hi) = var_lo_hi(var, &parsed.bounds, &ctx);
                val >= lo && val <= hi
            });
        if order_ok && neq_ok && sum_ok && bounds_ok {
            return Some(input);
        }
        retries += 1;
        if is_corner && retries >= 20 {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random_test::parse::{annotate_blocks, parse_constraints, parse_input_blocks};
    use rand::SeedableRng;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn rng() -> impl Rng { rand::rngs::SmallRng::seed_from_u64(42) }

    /// Simple "N\nA_1 ... A_N" problem.
    fn simple_setup() -> (Vec<InputBlock>, crate::random_test::parse::ConstraintParsed) {
        let lines = vec!["N".to_string(), r"A_1 \ldots A_N".to_string()];
        let items = vec![
            r"1\le N\le 100".to_string(),
            r"1\le A_i\le 1000".to_string(),
        ];
        let mut blocks = parse_input_blocks(&lines);
        let parsed = parse_constraints(&items);
        annotate_blocks(&mut blocks, &parsed.bounds, &parsed.string_vars, &parsed.sum_constraints);
        (blocks, parsed)
    }

    fn gen(blocks: &[InputBlock], parsed: &crate::random_test::parse::ConstraintParsed, strategy: &CaseStrategy) -> String {
        generate_random_input(blocks, parsed, &mut rng(), strategy)
            .expect("generate_random_input returned None")
    }

    fn first_line_n(input: &str) -> i64 {
        input.lines().next().unwrap().trim().parse().unwrap()
    }

    fn array_values(input: &str) -> Vec<i64> {
        input.lines().nth(1).unwrap_or("").split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect()
    }

    // ── make_strategy_list ───────────────────────────────────────────────────

    #[test]
    fn strategy_list_length_matches_count() {
        let (blocks, _parsed) = simple_setup();
        for &count in &[1u32, 5, 10, 20] {
            let list = make_strategy_list(&blocks, count);
            assert_eq!(list.len(), count as usize, "count={count}");
        }
    }

    #[test]
    fn strategy_list_starts_with_random() {
        let (blocks, _parsed) = simple_setup();
        let list = make_strategy_list(&blocks, 10);
        // First entries should be Random(Random) for n>=10: 2 initial randoms
        assert!(matches!(list[0], CaseStrategy::Random(RandomStrategy::Random)));
        assert!(matches!(list[1], CaseStrategy::Random(RandomStrategy::Random)));
    }

    #[test]
    fn strategy_list_single_case_is_random() {
        let (blocks, _parsed) = simple_setup();
        let list = make_strategy_list(&blocks, 1);
        assert!(matches!(list[0], CaseStrategy::Random(RandomStrategy::Random)));
    }

    #[test]
    fn strategy_list_post_coverage_has_only_random_strategies() {
        // Scalar-only, no i64 vars: corner_count = 5 (AllMax, AllMin, SmallSize(1,2,3)).
        // ZeroCorner is excluded because N has lo=1>=0.
        let lines = vec!["N".to_string()];
        let items = vec![r"1\le N\le 100".to_string()];
        let mut blocks = parse_input_blocks(&lines);
        let parsed = parse_constraints(&items);
        annotate_blocks(&mut blocks, &parsed.bounds, &parsed.string_vars, &parsed.sum_constraints);

        let list = make_strategy_list(&blocks, 50);
        // initial_random=2 (n>=10), corner_count=5 → post-coverage starts at index 7.
        for strategy in &list[7..] {
            assert!(
                matches!(strategy, CaseStrategy::Random(_)),
                "post-coverage slot must be Random, got {:?}", strategy
            );
        }
    }

    #[test]
    fn strategy_list_zero_corner_included_when_i64_vars_exist() {
        let lines = vec!["X".to_string()];
        let items = vec![r"-100\le X\le 100".to_string()];
        let mut blocks = parse_input_blocks(&lines);
        let parsed = parse_constraints(&items);
        annotate_blocks(&mut blocks, &parsed.bounds, &parsed.string_vars, &parsed.sum_constraints);
        let list = make_strategy_list(&blocks, 20);
        assert!(
            list.iter().any(|s| matches!(s, CaseStrategy::Random(RandomStrategy::ZeroCorner))),
            "ZeroCorner should be included when i64 vars exist"
        );
    }

    #[test]
    fn strategy_list_zero_corner_excluded_when_no_i64_vars() {
        let (blocks, _parsed) = simple_setup(); // 1<=N<=100, 1<=A_i<=1000 (no negative lo)
        let list = make_strategy_list(&blocks, 20);
        assert!(
            !list.iter().any(|s| matches!(s, CaseStrategy::Random(RandomStrategy::ZeroCorner))),
            "ZeroCorner should be excluded when no i64 vars"
        );
    }

    // ── AllMax / AllMin ──────────────────────────────────────────────────────

    #[test]
    fn allmax_produces_maximum_n() {
        let (blocks, parsed) = simple_setup();
        let input = gen(&blocks, &parsed, &CaseStrategy::Deterministic(DeterministicStrategy::AllMax));
        assert_eq!(first_line_n(&input), 100, "AllMax should give N=100");
    }

    #[test]
    fn allmin_produces_minimum_n() {
        let (blocks, parsed) = simple_setup();
        let input = gen(&blocks, &parsed, &CaseStrategy::Deterministic(DeterministicStrategy::AllMin));
        assert_eq!(first_line_n(&input), 1, "AllMin should give N=1");
    }

    #[test]
    fn allmax_array_all_at_bound() {
        let (blocks, parsed) = simple_setup();
        let input = gen(&blocks, &parsed, &CaseStrategy::Deterministic(DeterministicStrategy::AllMax));
        for v in array_values(&input) {
            assert_eq!(v, 1000, "AllMax array element should be 1000");
        }
    }

    #[test]
    fn allmin_array_all_at_bound() {
        let (blocks, parsed) = simple_setup();
        let input = gen(&blocks, &parsed, &CaseStrategy::Deterministic(DeterministicStrategy::AllMin));
        for v in array_values(&input) {
            assert_eq!(v, 1, "AllMin array element should be 1");
        }
    }

    // ── SmallSize ────────────────────────────────────────────────────────────

    #[test]
    fn smallsize_clamps_size_var() {
        let (blocks, parsed) = simple_setup();
        for k in 1i64..=3 {
            let input = gen(&blocks, &parsed, &CaseStrategy::Random(RandomStrategy::SmallSize(k)));
            assert_eq!(first_line_n(&input), k, "SmallSize({k}) should give N={k}");
        }
    }

    #[test]
    fn smallsize_array_length_matches() {
        let (blocks, parsed) = simple_setup();
        let input = gen(&blocks, &parsed, &CaseStrategy::Random(RandomStrategy::SmallSize(3)));
        assert_eq!(array_values(&input).len(), 3, "SmallSize(3) should produce 3-element array");
    }

    // ── ZeroCorner ───────────────────────────────────────────────────────────

    #[test]
    fn zero_corner_sets_spanning_vars_to_zero() {
        let lines = vec!["X".to_string()];
        let items = vec![r"-100\le X\le 100".to_string()];
        let blocks = parse_input_blocks(&lines);
        let parsed = parse_constraints(&items);
        let input = gen(&blocks, &parsed, &CaseStrategy::Random(RandomStrategy::ZeroCorner));
        let x: i64 = input.trim().parse().unwrap();
        assert_eq!(x, 0, "ZeroCorner should set X=0 when lo<0<hi");
    }

    #[test]
    fn zero_corner_non_spanning_var_is_random() {
        let lines = vec!["X".to_string()];
        let items = vec![r"1\le X\le 100".to_string()];
        let blocks = parse_input_blocks(&lines);
        let parsed = parse_constraints(&items);
        // lo=1>0: not lo<0<hi, so X should be random in [1,100]
        let input = gen(&blocks, &parsed, &CaseStrategy::Random(RandomStrategy::ZeroCorner));
        let x: i64 = input.trim().parse().unwrap();
        assert!((1..=100).contains(&x), "ZeroCorner with lo>=0 should give random in [1,100], got {}", x);
    }

    // ── Array strategies ─────────────────────────────────────────────────────

    #[test]
    fn array_mono_inc_is_nondecreasing() {
        let (blocks, parsed) = simple_setup();
        let input = gen(&blocks, &parsed, &CaseStrategy::Random(RandomStrategy::ArrayMonoInc));
        let vals = array_values(&input);
        for w in vals.windows(2) {
            assert!(w[0] <= w[1], "ArrayMonoInc: {} > {}", w[0], w[1]);
        }
    }

    #[test]
    fn array_mono_dec_is_nonincreasing() {
        let (blocks, parsed) = simple_setup();
        let input = gen(&blocks, &parsed, &CaseStrategy::Random(RandomStrategy::ArrayMonoDec));
        let vals = array_values(&input);
        for w in vals.windows(2) {
            assert!(w[0] >= w[1], "ArrayMonoDec: {} < {}", w[0], w[1]);
        }
    }

    #[test]
    fn array_all_same_has_uniform_values() {
        let (blocks, parsed) = simple_setup();
        let input = gen(&blocks, &parsed, &CaseStrategy::Random(RandomStrategy::ArrayAllSame));
        let vals = array_values(&input);
        if vals.len() > 1 {
            assert!(vals.windows(2).all(|w| w[0] == w[1]), "ArrayAllSame: not all equal: {:?}", vals);
        }
    }

    // ── Random strategy satisfies constraints ────────────────────────────────

    #[test]
    fn random_strategy_satisfies_bounds() {
        let (blocks, parsed) = simple_setup();
        for _ in 0..20 {
            let input = generate_random_input(&blocks, &parsed, &mut rng(), &CaseStrategy::Random(RandomStrategy::Random))
                .unwrap();
            let n = first_line_n(&input);
            assert!((1..=100).contains(&n), "N={} out of [1,100]", n);
            for v in array_values(&input) {
                assert!((1..=1000).contains(&v), "A_i={} out of [1,1000]", v);
            }
        }
    }

    #[test]
    fn random_strategy_array_length_matches_n() {
        let (blocks, parsed) = simple_setup();
        for _ in 0..10 {
            let input = generate_random_input(&blocks, &parsed, &mut rng(), &CaseStrategy::Random(RandomStrategy::Random))
                .unwrap();
            let n = first_line_n(&input) as usize;
            assert_eq!(array_values(&input).len(), n, "array length should equal N");
        }
    }

    // ── OuterRepeat: N variation across iterations ───────────────────────────

    // inner ブロックに Array1D を含めることで n が collect_size_vars に検出される
    // (Scalars のみだとサイズ変数と見なされず has_sum_inner=false になる)。
    // 入力形式: "T\nN_1\nA_1...\nN_2\nA_2...\n..." → N は lines[1,3,5,...]。
    fn outer_repeat_setup(t_fixed: i64, n_hi: i64, limit: i64) -> (Vec<InputBlock>, crate::random_test::parse::ConstraintParsed) {
        use crate::random_test::parse::{BoundVal, ConstraintParsed, InputBlock, SizeRef, SumConstraint, VarBound, VarType};
        use std::collections::{HashMap, HashSet};

        let sum_constraint = SumConstraint { inner_var: "n".to_string(), limit, inner_var_hi: n_hi };
        let blocks = vec![
            InputBlock::Scalars(vec![("t".to_string(), VarType::Usize)]),
            InputBlock::OuterRepeat {
                count: SizeRef::Var("t".to_string()),
                inner: vec![
                    InputBlock::Scalars(vec![("n".to_string(), VarType::Usize)]),
                    InputBlock::Array1D { base: "a".to_string(), ty: VarType::Usize, len: SizeRef::Var("n".to_string()) },
                ],
                sum_constraints: vec![sum_constraint.clone()],
            },
        ];
        let mut bounds = HashMap::new();
        bounds.insert("t".to_string(), VarBound { lo: BoundVal::Lit(t_fixed), hi: BoundVal::Lit(t_fixed) });
        bounds.insert("n".to_string(), VarBound { lo: BoundVal::Lit(1), hi: BoundVal::Lit(n_hi) });
        bounds.insert("a".to_string(), VarBound { lo: BoundVal::Lit(1), hi: BoundVal::Lit(100) });
        let parsed = ConstraintParsed {
            bounds,
            var_to_var: vec![],
            var_not_eq: vec![],
            all_distinct: HashSet::new(),
            string_vars: HashMap::new(),
            skipped: vec![],
            sum_constraints: vec![sum_constraint],
        };
        (blocks, parsed)
    }

    #[test]
    fn outer_repeat_random_n_varies_across_iterations() {
        // T=4 (fixed), ΣN≤20 → per_iter_hi = floor(20/4) = 5, lo=1.
        // 30回×4グループ=120個のN値を収集し、2種類以上現れることを確認。
        let (blocks, parsed) = outer_repeat_setup(4, 20, 20);
        let strategy = CaseStrategy::Random(RandomStrategy::Random);
        let mut rng = rand::rngs::SmallRng::seed_from_u64(42);

        let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for _ in 0..30 {
            let input = generate_random_input(&blocks, &parsed, &mut rng, &strategy)
                .expect("generate_random_input returned None");
            // lines[1,3,5,7] が各グループの N
            for line in input.lines().skip(1).step_by(2).take(4) {
                if let Ok(n) = line.trim().parse::<i64>() {
                    seen.insert(n);
                }
            }
        }
        assert!(seen.len() >= 2, "N should vary across iterations but only saw {:?}", seen);
    }

    #[test]
    fn outer_repeat_n_within_per_iter_cap() {
        // T=4 (fixed), ΣN≤20 → per_iter_hi = floor(20/4) = 5.
        // 全グループで N ∈ [1,5] であること。
        let (blocks, parsed) = outer_repeat_setup(4, 20, 20);
        let strategy = CaseStrategy::Random(RandomStrategy::Random);
        let mut rng = rand::rngs::SmallRng::seed_from_u64(123);

        for _ in 0..50 {
            let input = generate_random_input(&blocks, &parsed, &mut rng, &strategy)
                .expect("generate_random_input returned None");
            for line in input.lines().skip(1).step_by(2).take(4) {
                if let Ok(n) = line.trim().parse::<i64>() {
                    assert!((1..=5).contains(&n), "N={} exceeds per_iter_hi=5", n);
                }
            }
        }
    }

    #[test]
    fn outer_repeat_sum_constraint_satisfied() {
        // T=7 (fixed), ΣN≤20 → per_iter_hi = floor(20/7) = 2.
        // T×per_iter_hi=14≤20 なので ΣN は常に20以下になるはず。
        let (blocks, parsed) = outer_repeat_setup(7, 20, 20);
        let strategy = CaseStrategy::Random(RandomStrategy::Random);
        let mut rng = rand::rngs::SmallRng::seed_from_u64(99);

        for _ in 0..50 {
            let input = generate_random_input(&blocks, &parsed, &mut rng, &strategy)
                .expect("generate_random_input returned None");
            let total: i64 = input.lines().skip(1).step_by(2).take(7)
                .filter_map(|l| l.trim().parse::<i64>().ok())
                .sum();
            assert!(total <= 20, "ΣN={} exceeds limit=20", total);
        }
    }

    // ── SumMaxSingle ─────────────────────────────────────────────────────────

    #[test]
    fn sum_max_single_sets_inner_var_to_limit() {
        // SumMaxSingle sets each inner_var to its limit. Use simple_setup (1<=N<=100)
        // with limit within the allowed range so bounds_ok passes.
        let (blocks, parsed) = simple_setup();
        let strategy = CaseStrategy::Random(RandomStrategy::SumMaxSingle {
            inner_vars: vec![("n".to_string(), 50)], // N=50, within [1,100]
            outer_var: None,
        });
        let input = generate_random_input(&blocks, &parsed, &mut rng(), &strategy).unwrap();
        assert_eq!(first_line_n(&input), 50, "SumMaxSingle should set N=50");
    }

    #[test]
    fn sum_max_single_caps_inner_var_at_per_var_hi() {
        // Regression: limit (sum upper bound) can exceed per-variable hi.
        // E.g. N ≤ 100, ΣN ≤ 2×10^5 → SumMaxSingle should set N=100, not N=2×10^5.
        // Before the fix, N was set to min(limit, MAX_ARRAY_SIZE)=200000, failing bounds_ok
        // and returning None after 20 retries.
        let (blocks, parsed) = simple_setup(); // N ∈ [1,100]
        let strategy = CaseStrategy::Random(RandomStrategy::SumMaxSingle {
            inner_vars: vec![("n".to_string(), 200_000)], // sum limit >> per-var hi
            outer_var: None,
        });
        let input = generate_random_input(&blocks, &parsed, &mut rng(), &strategy);
        assert!(input.is_none(), "limit>per_var_hi without inner_var_hi cap should return None (pre-fix behavior)");

        // With make_strategy_list using .min(inner_var_hi), the strategy gets inner_vars=[("n",100)].
        // Simulate that:
        let strategy_fixed = CaseStrategy::Random(RandomStrategy::SumMaxSingle {
            inner_vars: vec![("n".to_string(), 100)], // capped at per-var hi
            outer_var: None,
        });
        let input_fixed = generate_random_input(&blocks, &parsed, &mut rng(), &strategy_fixed)
            .expect("capped SumMaxSingle should succeed");
        assert_eq!(first_line_n(&input_fixed), 100, "N should be capped at per-var hi=100");
    }

    // ── ZeroCorner: NRepeat with signed vars ────────────────────────────────

    /// abc442/e-like structure:
    /// N Q
    /// X_1 Y_1 / ... / X_N Y_N    (X,Y: i64 spanning zero)
    /// A_1 B_1 / ... / A_Q B_Q    (A,B: unsigned, hi=N)
    ///
    /// ZeroCorner should:
    ///   - set all X=0, Y=0 (lo<0<hi)
    ///   - leave N, Q random (lo>=0)
    ///   - leave A, B random in [1, N]
    #[test]
    fn zero_corner_nrepeat_signed_elem_is_zero_n_varies() {
        use crate::random_test::parse::{BoundVal, ConstraintParsed, InputBlock, SizeRef, VarBound, VarType};
        use std::collections::{HashMap, HashSet};

        let blocks = vec![
            InputBlock::Scalars(vec![("n".to_string(), VarType::Usize), ("q".to_string(), VarType::Usize)]),
            InputBlock::NRepeat {
                cols: vec![("x".to_string(), VarType::I64), ("y".to_string(), VarType::I64)],
                count: SizeRef::Var("n".to_string()),
            },
            InputBlock::NRepeat {
                cols: vec![("a".to_string(), VarType::Usize), ("b".to_string(), VarType::Usize)],
                count: SizeRef::Var("q".to_string()),
            },
        ];
        let mut bounds = HashMap::new();
        bounds.insert("n".to_string(), VarBound { lo: BoundVal::Lit(2), hi: BoundVal::Lit(10) });
        bounds.insert("q".to_string(), VarBound { lo: BoundVal::Lit(1), hi: BoundVal::Lit(10) });
        bounds.insert("x".to_string(), VarBound { lo: BoundVal::Lit(-100), hi: BoundVal::Lit(100) });
        bounds.insert("y".to_string(), VarBound { lo: BoundVal::Lit(-100), hi: BoundVal::Lit(100) });
        bounds.insert("a".to_string(), VarBound { lo: BoundVal::Lit(1), hi: BoundVal::Var("n".to_string()) });
        bounds.insert("b".to_string(), VarBound { lo: BoundVal::Lit(1), hi: BoundVal::Var("n".to_string()) });
        let parsed = ConstraintParsed {
            bounds,
            var_to_var: vec![],
            var_not_eq: vec![],
            all_distinct: HashSet::new(),
            string_vars: HashMap::new(),
            skipped: vec![],
            sum_constraints: vec![],
        };

        let strategy = CaseStrategy::Random(RandomStrategy::ZeroCorner);
        let mut seen_n: HashSet<i64> = HashSet::new();

        for seed in 0u64..30 {
            let mut r = rand::rngs::SmallRng::seed_from_u64(seed);
            let input = generate_random_input(&blocks, &parsed, &mut r, &strategy)
                .expect("generate_random_input returned None");
            let mut lines = input.lines();
            let header = lines.next().unwrap();
            let mut hvals = header.split_whitespace();
            let n: i64 = hvals.next().unwrap().parse().unwrap();
            seen_n.insert(n);
            // All X, Y rows must be 0
            for _ in 0..n {
                let row = lines.next().expect("expected X Y row");
                for v in row.split_whitespace() {
                    let val: i64 = v.parse().unwrap();
                    assert_eq!(val, 0, "ZeroCorner: expected X/Y=0 (lo<0<hi), got {val}");
                }
            }
        }

        assert!(
            seen_n.len() >= 2,
            "ZeroCorner: N should vary across runs (lo=2>=0), but only saw N values: {:?}",
            seen_n
        );
    }

    #[test]
    fn sum_max_single_sets_outer_var_to_one() {
        // Two scalars: T and N.  SumMaxSingle with outer_var=T sets T=1.
        let lines = vec!["T".to_string(), "N".to_string()];
        let items = vec![
            r"1\le T\le 100".to_string(),
            r"1\le N\le 100".to_string(),
        ];
        let blocks = parse_input_blocks(&lines);
        let parsed = parse_constraints(&items);
        let strategy = CaseStrategy::Random(RandomStrategy::SumMaxSingle {
            inner_vars: vec![("n".to_string(), 50)],
            outer_var: Some("t".to_string()),
        });
        let input = generate_random_input(&blocks, &parsed, &mut rng(), &strategy).unwrap();
        let t = first_line_n(&input);
        assert_eq!(t, 1, "SumMaxSingle should set T=1, got {t}");
    }
}
