use crate::web::html_parse::{
    is_case_placeholder_line, is_query_placeholder_line, is_string_symbol, normalize_constraint,
    normalize_line, parse_1d_array_line, parse_grid_lines, parse_grid_row, parse_matrix_block,
    parse_n_repeat, parse_vertical_scalars,
};
use regex::Regex;
use std::collections::{HashMap, HashSet};

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct VarBound {
    pub lo: BoundVal,
    pub hi: BoundVal,
}

impl Default for VarBound {
    fn default() -> Self {
        VarBound {
            lo: BoundVal::Lit(1),
            hi: BoundVal::Lit(1_000_000_000),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum BoundVal {
    Lit(i64),
    Var(String),
    VarOffset(String, i64),
    /// Discrete candidate set (from "いずれか" / "∈{...}" constraints).
    Set(Vec<i64>),
}

#[derive(Debug, Clone)]
pub(crate) enum CharSet {
    LowerAlpha,
    UpperAlpha,
    Explicit(Vec<char>),
}

impl CharSet {
    pub(crate) fn all_chars(&self) -> Vec<char> {
        match self {
            CharSet::LowerAlpha => ('a'..='z').collect(),
            CharSet::UpperAlpha => ('A'..='Z').collect(),
            CharSet::Explicit(cs) => cs.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StringVarSpec {
    pub charset: CharSet,
    pub lo_len: BoundVal,
    pub hi_len: BoundVal,
}

/// A "sum of X across T test cases <= limit" constraint.
/// Used at generation time: after generating all T inner blocks, the actual
/// sum of inner_var values is checked and rejected if it exceeds limit.
#[derive(Debug, Clone)]
pub(crate) struct SumConstraint {
    pub inner_var: String,
    pub limit: i64,
}

/// One branch of a typed query repeat block (e.g. `1 x` or `2 l r`).
#[derive(Debug, Clone)]
pub(crate) struct TypedBranch {
    /// The literal type token that appears at the start of the query line (e.g. "1", "2").
    pub type_val: String,
    /// The variables that follow the type token on the same (or subsequent) line(s).
    pub inner: Vec<InputBlock>,
}

pub(crate) struct ConstraintParsed {
    pub bounds: HashMap<String, VarBound>,
    /// lo_var <= hi_var ordering pairs
    pub var_to_var: Vec<(String, String)>,
    /// a != b pairs
    pub var_not_eq: Vec<(String, String)>,
    /// array base vars whose elements must be pairwise distinct ("相異なる")
    pub all_distinct: HashSet<String>,
    pub string_vars: HashMap<String, StringVarSpec>,
    pub skipped: Vec<String>,
    /// Sum constraints collected from "X の総和は Y 以下" items.
    pub sum_constraints: Vec<SumConstraint>,
}

// ── Numeric expression helpers ────────────────────────────────────────────────

pub(crate) fn eval_expr(expr: &str) -> Option<i64> {
    let expr = expr.trim();
    if let Ok(n) = expr.parse::<i64>() {
        return Some(n);
    }
    if let Some(pos) = expr.find('*') {
        let left = eval_expr(&expr[..pos])?;
        let right = eval_expr(&expr[pos + 1..])?;
        return Some(left * right);
    }
    if let Some(pos) = expr.find('^') {
        let base = eval_expr(&expr[..pos])?;
        let exp = eval_expr(&expr[pos + 1..])?;
        if exp >= 0 && exp <= 30 {
            return Some(base.pow(exp as u32));
        }
    }
    None
}

fn split_first_arg(s: &str) -> &str {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return &s[..i],
            _ => {}
        }
    }
    s
}

fn parse_bound_val(expr: &str) -> Option<BoundVal> {
    let expr = expr.trim();
    // min(...) → first argument as upper bound
    if expr.starts_with("min(") && expr.ends_with(')') {
        let inner = &expr[4..expr.len() - 1];
        let first = split_first_arg(inner).trim();
        return parse_bound_val(first);
    }
    if let Some(n) = eval_expr(expr) {
        return Some(BoundVal::Lit(n));
    }
    if let Some((var, k)) = expr.split_once('-') {
        let var = var.trim();
        if var.chars().all(|c| c.is_ascii_alphanumeric())
            && var.starts_with(|c: char| c.is_ascii_alphabetic())
        {
            if let Ok(k) = k.trim().parse::<i64>() {
                return Some(BoundVal::VarOffset(var.to_lowercase(), -k));
            }
        }
    }
    if let Some((var, k)) = expr.split_once('+') {
        let var = var.trim();
        if var.chars().all(|c| c.is_ascii_alphanumeric())
            && var.starts_with(|c: char| c.is_ascii_alphabetic())
        {
            if let Ok(k) = k.trim().parse::<i64>() {
                return Some(BoundVal::VarOffset(var.to_lowercase(), k));
            }
        }
    }
    // Only accept plain variable names (no underscore) as Var bounds.
    // "R_i" is an array-element reference, not a scalar variable, so reject it here.
    if expr.chars().all(|c| c.is_ascii_alphanumeric())
        && expr.starts_with(|c: char| c.is_ascii_alphabetic())
    {
        return Some(BoundVal::Var(expr.to_lowercase()));
    }
    // Fallback: split on non-ASCII (Japanese) character boundaries to get clean ASCII segments.
    // normalize_constraint removes spaces, so "N は 1" becomes "Nは1"; splitting on 'は' yields
    // ["N", "1"]. We try segments right-to-left so the numeric bound is found first.
    if !expr.is_ascii() {
        let segments: Vec<&str> = expr
            .split(|c: char| !c.is_ascii())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        for seg in segments.iter().rev() {
            if let Some(v) = parse_bound_val(seg) {
                return Some(v);
            }
        }
    }
    None
}

fn extract_var_names(tok: &str) -> Vec<String> {
    tok.split(',')
        .filter_map(|part| {
            let p = part.trim();
            let base = if let Some(i) = p.find('_') { &p[..i] } else { p };
            let base = base.trim_matches(|c| c == '{' || c == '}');
            if !base.is_empty()
                && base.chars().all(|c| c.is_ascii_alphanumeric())
                && base.starts_with(|c: char| c.is_ascii_alphabetic())
            {
                Some(base.to_lowercase())
            } else {
                None
            }
        })
        .collect()
}

fn parse_var_list(s: &str) -> Vec<String> {
    s.split(',')
        .filter_map(|part| {
            let part = part.trim();
            let base = part.split('_').next().unwrap_or(part).trim();
            let base = base.split(' ').next().unwrap_or(base).trim();
            if !base.is_empty() && base.starts_with(|c: char| c.is_ascii_alphabetic()) {
                Some(base.to_lowercase())
            } else {
                None
            }
        })
        .collect()
}

// ── Normalization ─────────────────────────────────────────────────────────────

fn normalize_for_random(s: &str) -> String {
    let mut t = normalize_constraint(s);
    t = t.replace("\\neq", "!=").replace("\\ne", "!=");
    t = t
        .replace("\\min\\left(", "min(")
        .replace("\\min(", "min(")
        .replace("\\max\\left(", "max(")
        .replace("\\max(", "max(")
        .replace("\\left(", "(")
        .replace("\\right)", ")");
    t
}

// ── Enum constraint ("いずれか" / "∈{...}") ───────────────────────────────────

fn try_parse_enum_constraint(item: &str) -> Option<(Vec<String>, Vec<i64>)> {
    if item.contains("のいずれか") || item.contains("いずれかの") {
        let ha_pos = item.find(" は")?;
        let vars_str = &item[..ha_pos];
        let rest = item[ha_pos + " は".len()..].trim_start_matches(' ');

        let nums_raw = if let Some(p) = rest.find("のいずれか") {
            &rest[..p]
        } else if let Some(p) = rest.find("いずれかの") {
            &rest[..p]
        } else {
            return None;
        };

        let nums: Vec<i64> = nums_raw
            .split(|c| c == ',' || c == ' ')
            .filter_map(|p| p.trim().parse::<i64>().ok())
            .collect();
        if nums.is_empty() {
            return None;
        }

        let var_names = parse_var_list(vars_str);
        if var_names.is_empty() {
            return None;
        }
        return Some((var_names, nums));
    }

    if item.contains("\\in") {
        let in_pos = item.find("\\in")?;
        let vars_str = item[..in_pos].trim();
        let rest = item[in_pos..].trim_start_matches("\\in").trim();
        let content = rest
            .trim_start_matches("\\lbrace")
            .trim_start_matches("\\{")
            .trim()
            .trim_end_matches("\\rbrace")
            .trim_end_matches("\\}")
            .trim();

        let nums: Vec<i64> = content
            .split(',')
            .filter_map(|p| p.trim().parse::<i64>().ok())
            .collect();
        if nums.is_empty() {
            return None;
        }
        let var_names = parse_var_list(vars_str);
        if var_names.is_empty() {
            return None;
        }
        return Some((var_names, nums));
    }

    None
}

// ── Per-item helpers for special constraint types ────────────────────────────

/// Extract base variable names from a "相異なる" constraint item.
fn parse_all_distinct_item(item: &str) -> Vec<String> {
    let var_part = if let Some(p) = item.split("はすべて相異なる").next() {
        p
    } else if let Some(p) = item.split("は相異なる").next() {
        p
    } else {
        return vec![];
    };
    let mut result = Vec::new();
    for token in var_part.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        let token = token.trim();
        if token.is_empty() { continue; }
        if token.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
            let base: String = token.chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect::<String>()
                .to_lowercase();
            if !base.is_empty() { result.push(base); }
        }
    }
    result
}

/// Parse one "X の総和は Y 以下" item; returns SumConstraint and adjusts T/Q bounds if needed.
fn try_parse_sum_constraint_item(
    item: &str,
    bounds: &mut HashMap<String, VarBound>,
) -> Option<SumConstraint> {
    let pos = item.find("の総和は")?;
    let before = &item[..pos];
    if before.contains('^') { return None; }
    let var_name = before.split_whitespace().last().unwrap_or("").to_lowercase();
    if var_name.is_empty() || !var_name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let after = &item[pos + "の総和は".len()..];
    let limit_raw = ["以下", "を超えない"]
        .iter()
        .find_map(|&delim| after.find(delim).map(|idx| after[..idx].trim()))
        .unwrap_or("");
    let limit = eval_expr(&normalize_constraint(limit_raw))?;

    let lo = bounds
        .get(&var_name)
        .map(|b| match &b.lo { BoundVal::Lit(n) => *n, _ => 1 })
        .unwrap_or(1)
        .max(1);
    let sum_based_t_hi = (limit / lo).min(200);
    for candidate in ["t", "q"] {
        if let Some(bound) = bounds.get_mut(candidate) {
            if let BoundVal::Lit(cur) = &bound.hi {
                if *cur > sum_based_t_hi {
                    bound.hi = BoundVal::Lit(sum_based_t_hi);
                }
            }
        }
    }
    // Cap inner_var.hi to limit/T_cap so AllMax never generates violating sums.
    // SumMaxSingle overrides this back to `limit` at generation time (T=1, N=limit).
    let inner_hi = (limit / sum_based_t_hi).max(1);
    let inner_entry = bounds.entry(var_name.clone()).or_insert_with(VarBound::default);
    match &inner_entry.hi {
        BoundVal::Lit(cur) if *cur > inner_hi => { inner_entry.hi = BoundVal::Lit(inner_hi); }
        BoundVal::Lit(_) => {}
        _ => { inner_entry.hi = BoundVal::Lit(inner_hi); }
    }
    Some(SumConstraint { inner_var: var_name, limit })
}

// ── Main constraint parser ────────────────────────────────────────────────────

pub(crate) fn parse_constraints(items: &[String]) -> ConstraintParsed {
    let op_re = Regex::new(r"(<=|>=|!=|<|>)").unwrap();
    let abs_re = Regex::new(r"\|([A-Za-z])\|").unwrap();
    let mut bounds: HashMap<String, VarBound> = HashMap::new();
    let mut var_to_var: Vec<(String, String)> = Vec::new();
    let mut var_not_eq: Vec<(String, String)> = Vec::new();
    let mut seen_var_to_var: HashSet<(String, String)> = HashSet::new();
    let mut seen_var_not_eq: HashSet<(String, String)> = HashSet::new();
    let mut all_distinct: HashSet<String> = HashSet::new();
    let mut sum_constraints: Vec<SumConstraint> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // Pre-pass: build string_vars so that abs-length items later in the list can
    // reference them.  Step B (is_string_symbol fallback) happens after block parsing.
    let mut string_vars = parse_string_constraints(items);
    apply_abs_length_constraints(items, &mut string_vars);

    for item in items {
        let mut handled = false;

        // 1. Enum constraint ("いずれか" / "∈{...}")
        if let Some((var_names, vals)) = try_parse_enum_constraint(item) {
            let lo = vals.iter().copied().min().unwrap_or(0);
            for var in &var_names {
                let entry = bounds.entry(var.clone()).or_insert_with(VarBound::default);
                entry.lo = BoundVal::Lit(lo);
                entry.hi = BoundVal::Set(vals.clone());
            }
            handled = true;
        }

        // 2. String constraint ("からなる") — pre-processed; mark handled if var registered.
        if item.contains("からなる") {
            if let Some(ha_pos) = item.find(" は") {
                let vars_str = item[..ha_pos].trim();
                if parse_var_list(vars_str).iter().any(|v| string_vars.contains_key(v)) {
                    handled = true;
                }
            }
        }

        // 3. Abs-length constraint ("|S| <= N") — pre-processed; mark handled if var registered.
        if !handled && item.contains('|') {
            let norm = normalize_constraint(item);
            if abs_re.find_iter(&norm).any(|m| {
                let var = m.as_str().trim_matches('|').to_lowercase();
                string_vars.contains_key(&var)
            }) {
                handled = true;
            }
        }

        // 4. All-distinct ("相異なる")
        if !handled && item.contains("相異なる") {
            let found = parse_all_distinct_item(item);
            if !found.is_empty() {
                all_distinct.extend(found);
                handled = true;
            }
        }

        // 5. Sum constraint ("の総和は")
        if !handled && item.contains("の総和は") {
            if let Some(sc) = try_parse_sum_constraint_item(item, &mut bounds) {
                sum_constraints.push(sc);
                handled = true;
            }
        }

        // 6. Numeric bounds (inequality operators)
        if !handled {
            let norm = normalize_for_random(item);
            if !norm.contains("dfrac") && !norm.contains("frac") && !norm.contains("sqrt") {
                let mut tokens: Vec<String> = Vec::new();
                let mut ops: Vec<String> = Vec::new();
                let mut last = 0usize;
                for m in op_re.find_iter(&norm) {
                    tokens.push(norm[last..m.start()].to_string());
                    ops.push(m.as_str().to_string());
                    last = m.end();
                }
                tokens.push(norm[last..].to_string());

                if !ops.is_empty() {
                    let mut any_neq = false;
                    for j in 0..ops.len() {
                        if ops[j] == "!=" {
                            let lt = tokens[j].trim();
                            let rt = tokens[j + 1].trim();
                            // Element-wise constraint (A_i != B_i) — can't enforce; let it fall to skipped.
                            if lt.contains('_') || rt.contains('_') { continue; }
                            let lv = extract_var_names(lt);
                            let rv = extract_var_names(rt);
                            if lv.len() == 1 && rv.len() == 1 {
                                let pair = (lv[0].clone(), rv[0].clone());
                                if seen_var_not_eq.insert(pair.clone()) {
                                    var_not_eq.push(pair);
                                }
                                any_neq = true;
                            }
                        }
                    }
                    for j in 0..ops.len() {
                        if ops[j] == "<=" || ops[j] == "<" {
                            let lv = extract_var_names(tokens[j].trim());
                            let rv = extract_var_names(tokens[j + 1].trim());
                            for l in &lv { for r in &rv {
                                let pair = (l.clone(), r.clone());
                                if seen_var_to_var.insert(pair.clone()) { var_to_var.push(pair); }
                            }}
                        } else if ops[j] == ">=" || ops[j] == ">" {
                            let lv = extract_var_names(tokens[j + 1].trim());
                            let rv = extract_var_names(tokens[j].trim());
                            for l in &lv { for r in &rv {
                                let pair = (l.clone(), r.clone());
                                if seen_var_to_var.insert(pair.clone()) { var_to_var.push(pair); }
                            }}
                        }
                    }
                    let mut parsed_any = false;
                    for i in 0..tokens.len() {
                        let token_str = tokens[i].trim();
                        // Skip tokens like "N,1" (from "1<=i<=N,1<=j<=N") where a comma-separated
                        // part is a numeric literal — these are subscript range expressions, not
                        // multi-variable tokens, and would wrongly overwrite real variable bounds.
                        if token_str.contains(',') {
                            let has_numeric_part = token_str.split(',')
                                .any(|p| p.trim().starts_with(|c: char| c.is_ascii_digit()));
                            if has_numeric_part { continue; }
                        }
                        let vars = extract_var_names(token_str);
                        if vars.is_empty() { continue; }
                        let lo = if i > 0 && (ops[i-1] == "<=" || ops[i-1] == "<") {
                            parse_bound_val(tokens[i-1].trim())
                        } else if i < ops.len() && (ops[i] == ">=" || ops[i] == ">") {
                            parse_bound_val(tokens[i+1].trim())
                        } else { None };
                        let hi = if i < ops.len() && (ops[i] == "<=" || ops[i] == "<") {
                            parse_bound_val(tokens[i+1].trim())
                        } else if i > 0 && (ops[i-1] == ">=" || ops[i-1] == ">") {
                            parse_bound_val(tokens[i-1].trim())
                        } else { None };
                        for var in vars {
                            if string_vars.contains_key(&var) { continue; }
                            let entry = bounds.entry(var).or_insert_with(VarBound::default);
                            if let Some(lo) = lo.clone() { entry.lo = lo; parsed_any = true; }
                            if let Some(hi) = hi.clone() { entry.hi = hi; parsed_any = true; }
                        }
                    }
                    if parsed_any || any_neq { handled = true; }
                }
            }
        }

        // 7. Ignorable: purely informational items that need no numeric action.
        //    "N は整数" — integer type is the default; no bound information added.
        if !handled && item.contains('は') && (item.contains("整数") || item.contains("正整数")) {
            handled = true;
        }

        // 8. If the variable is already registered as a string var (pre-pass), no warning needed.
        if !handled {
            if let Some(ha_pos) = item.find(" は") {
                let vars_str = item[..ha_pos].trim();
                let base = if let Some(idx) = vars_str.find('_') { vars_str[..idx].trim() } else { vars_str };
                if string_vars.contains_key(&base.to_lowercase()) {
                    handled = true;
                }
            }
        }

        if !handled {
            skipped.push(item.clone());
        }
    }

    // Remove string variables from numeric bounds.
    for var in string_vars.keys() {
        bounds.remove(var);
    }

    ConstraintParsed { bounds, var_to_var, var_not_eq, all_distinct, string_vars, skipped, sum_constraints }
}

// ── String constraint parsing ─────────────────────────────────────────────────


fn parse_bound_val_simple(raw: &str) -> BoundVal {
    let norm = normalize_constraint(raw);
    if let Some(n) = eval_expr(&norm) {
        BoundVal::Lit(n)
    } else if raw.chars().all(|c| c.is_ascii_alphanumeric()) {
        BoundVal::Var(raw.to_lowercase())
    } else {
        BoundVal::Lit(100)
    }
}

fn parse_length_spec(item: &str) -> (BoundVal, BoundVal) {
    // "長さ X 以上 Y 以下" pattern
    let range_re = Regex::new(r"長さ\s*(.+?)\s*以上\s*(.+?)\s*以下").unwrap();
    if let Some(cap) = range_re.captures(item) {
        let lo_raw = cap.get(1).unwrap().as_str().trim();
        let hi_raw = cap.get(2).unwrap().as_str().trim();
        return (parse_bound_val_simple(lo_raw), parse_bound_val_simple(hi_raw));
    }
    // "長さ X の" pattern (exact length — both lo and hi are the same expression)
    let single_re = Regex::new(r"長さ\s*([A-Za-z0-9\\^* ]+?)\s*の").unwrap();
    if let Some(cap) = single_re.captures(item) {
        let len_raw = cap.get(1).unwrap().as_str().trim();
        if !len_raw.contains("以上") && !len_raw.contains("以下") {
            let bv = parse_bound_val_simple(len_raw);
            return (bv.clone(), bv);
        }
    }
    (BoundVal::Lit(1), BoundVal::Lit(100))
}

fn extract_quoted_chars(s: &str) -> Vec<char> {
    let mut chars = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('「') {
        rest = &rest[open + '「'.len_utf8()..];
        if let Some(close) = rest.find('」') {
            let content = &rest[..close];
            if content.chars().count() == 1 {
                chars.push(content.chars().next().unwrap());
            }
            rest = &rest[close + '」'.len_utf8()..];
        } else {
            break;
        }
    }
    chars
}

fn parse_string_constraints(items: &[String]) -> HashMap<String, StringVarSpec> {
    let mut result = HashMap::new();

    for item in items {
        let Some(ha_pos) = item.find(" は") else { continue };
        let vars_str = item[..ha_pos].trim();
        let rest = item[ha_pos + " は".len()..].trim_start_matches(' ');

        // Determine charset.
        let charset = if rest.contains("英小文字") {
            CharSet::LowerAlpha
        } else if rest.contains("英大文字") {
            CharSet::UpperAlpha
        } else if item.matches('「').count() >= 2 {
            // Explicit chars preserved from HTML <code> tags (2+ tags = enumeration).
            let chars = extract_quoted_chars(rest);
            if chars.is_empty() { continue; }
            CharSet::Explicit(chars)
        } else {
            continue;
        };

        // Length spec: present when "長さ" or "からなる" appears; otherwise single char (grid cell).
        let (lo_len, hi_len) = if item.contains("長さ") || item.contains("からなる") {
            parse_length_spec(item)
        } else {
            (BoundVal::Lit(1), BoundVal::Lit(1))
        };

        // Variable name extraction.
        // For "X_{i,j}" style (2D grid subscript), take only the part before '_'.
        // For plain "X" or "X_i", use parse_var_list which handles comma-separated lists.
        let var_names = if vars_str.contains('{') {
            // 2D subscript: "S_{i,j}" → base "s"
            let base = vars_str[..vars_str.find('_').unwrap_or(vars_str.len())].trim();
            if base.starts_with(|c: char| c.is_ascii_alphabetic()) {
                vec![base.to_lowercase()]
            } else {
                continue;
            }
        } else {
            let names = parse_var_list(vars_str);
            if names.is_empty() { continue; }
            names
        };

        let spec = StringVarSpec { charset, lo_len, hi_len };
        for var in &var_names {
            result.insert(var.clone(), spec.clone());
        }
    }

    result
}

/// Parse `1 \le |S| \le N` style constraints and update string variable length bounds.
fn apply_abs_length_constraints(items: &[String], string_vars: &mut HashMap<String, StringVarSpec>) {
    let op_re = Regex::new(r"(<=|>=|<|>)").unwrap();
    let abs_re = Regex::new(r"\|([A-Za-z])\|").unwrap();

    for item in items {
        if !item.contains('|') {
            continue;
        }
        let norm = normalize_constraint(item);
        let mut tokens: Vec<String> = Vec::new();
        let mut ops: Vec<String> = Vec::new();
        let mut last = 0usize;
        for m in op_re.find_iter(&norm) {
            tokens.push(norm[last..m.start()].to_string());
            ops.push(m.as_str().to_string());
            last = m.end();
        }
        tokens.push(norm[last..].to_string());
        if ops.is_empty() {
            continue;
        }
        for i in 0..tokens.len() {
            let tok = tokens[i].trim();
            let Some(cap) = abs_re.captures(tok) else { continue };
            let var_name = cap.get(1).unwrap().as_str().to_lowercase();
            let Some(spec) = string_vars.get_mut(&var_name) else { continue };
            if i > 0 && (ops[i - 1] == "<=" || ops[i - 1] == "<") {
                let lo_raw = tokens[i - 1].trim();
                if let Some(n) = eval_expr(lo_raw) {
                    spec.lo_len = BoundVal::Lit(n);
                } else if lo_raw.chars().all(|c| c.is_ascii_alphanumeric()) {
                    spec.lo_len = BoundVal::Var(lo_raw.to_lowercase());
                }
            }
            if i < ops.len() && (ops[i] == "<=" || ops[i] == "<") {
                let hi_raw = tokens[i + 1].trim();
                if let Some(n) = eval_expr(hi_raw) {
                    spec.hi_len = BoundVal::Lit(n);
                } else if hi_raw.chars().all(|c| c.is_ascii_alphanumeric()) {
                    spec.hi_len = BoundVal::Var(hi_raw.to_lowercase());
                }
            }
        }
    }
}

// ── Step B: is_string_symbol fallback ────────────────────────────────────────

/// Step B of string-var detection: walk the already-parsed input blocks and register
/// any variable whose name matches the `is_string_symbol` heuristic (S/T/U/X) that
/// was not yet captured by the `からなる`-based Step A.
///
/// Called from the caller (mod.rs) after both `parse_constraints` and
/// `parse_input_blocks` have completed, because Step B requires the block structure.
pub(crate) fn apply_string_symbol_fallback(
    blocks: &[InputBlock],
    items: &[String],
    parsed: &mut ConstraintParsed,
) {
    fn collect_bases(blocks: &[InputBlock], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                // Only Vertical blocks are candidates: S_1 ⋮ S_N is always vertical.
                // Scalar/Array/NRepeat variables named X/T/U are usually numeric.
                InputBlock::Vertical { base, .. } => out.push(base.clone()),
                InputBlock::OuterRepeat { inner, .. } => collect_bases(inner, out),
                _ => {}
            }
        }
    }

    let mut bases = Vec::new();
    collect_bases(blocks, &mut bases);

    let mut added = false;
    for base in &bases {
        if is_string_symbol(base) && !parsed.string_vars.contains_key(base.as_str()) {
            parsed.string_vars.insert(
                base.clone(),
                StringVarSpec {
                    charset: CharSet::LowerAlpha,
                    lo_len: BoundVal::Lit(1),
                    hi_len: BoundVal::Lit(100),
                },
            );
            parsed.bounds.remove(base.as_str());
            added = true;
        }
    }

    if added {
        // Apply |S| \le N style constraints to newly added string vars too (Step C again).
        apply_abs_length_constraints(items, &mut parsed.string_vars);
    }
}

// ── Input format parsing ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum SizeRef {
    Lit(usize),
    Var(String),
    VarOffset(String, i64),
}

#[derive(Debug, Clone)]
pub(crate) enum InputBlock {
    Scalars(Vec<String>),
    Array1D { base: String, len: SizeRef },
    NRepeat { cols: Vec<String>, count: SizeRef },
    Vertical { base: String, count: SizeRef, width: Option<SizeRef> },
    /// 2-D integer matrix: `rows` lines each containing `cols` integers for `base`.
    Matrix { base: String, rows: SizeRef, cols: SizeRef },
    /// Outer "T test cases" or "Q queries" loop: repeat `inner` blocks `count` times.
    OuterRepeat { count: SizeRef, inner: Vec<InputBlock> },
    /// Typed query repeat: repeat `count` queries, each beginning with a literal type token
    /// that selects one of the `branches`.  E.g. `1 x` / `2 l r` in abc442/d.
    TypedRepeat { count: SizeRef, branches: Vec<TypedBranch> },
    Unsupported(()),
}

fn parse_size_ref(expr: &str) -> SizeRef {
    // Strip parentheses so "(t)+1" is treated the same as "t+1".
    let owned;
    let expr = {
        let s = expr.trim();
        if s.contains('(') || s.contains(')') {
            owned = s.replace('(', "").replace(')', "");
            owned.trim()
        } else {
            s
        }
    };
    if let Ok(n) = expr.parse::<usize>() {
        return SizeRef::Lit(n);
    }
    if let Some((var, k)) = expr.split_once('-') {
        if let Ok(k) = k.trim().parse::<i64>() {
            return SizeRef::VarOffset(var.trim().to_lowercase(), -k);
        }
    }
    if let Some((var, k)) = expr.split_once('+') {
        if let Ok(k) = k.trim().parse::<i64>() {
            return SizeRef::VarOffset(var.trim().to_lowercase(), k);
        }
    }
    SizeRef::Var(expr.to_lowercase())
}

pub(crate) fn parse_input_blocks(lines: &[String]) -> Vec<InputBlock> {
    let norm: Vec<String> = lines.iter().map(|l| normalize_line(l)).collect();
    let mut blocks = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let ln = &norm[i];

        if is_case_placeholder_line(ln) || is_query_placeholder_line(ln) || ln.contains("\\vdots") {
            i += 1;
            continue;
        }

        if let Some((bases, count_expr, consumed)) = parse_n_repeat(&norm, i) {
            let cols = bases.into_iter().map(|b| b.to_lowercase()).collect();
            blocks.push(InputBlock::NRepeat { cols, count: parse_size_ref(&count_expr) });
            i += consumed;
            continue;
        }

        if let Some((base, count_expr, consumed)) = parse_vertical_scalars(&norm, i) {
            blocks.push(InputBlock::Vertical {
                base: base.to_lowercase(),
                count: parse_size_ref(&count_expr),
                width: None,
            });
            i += consumed;
            continue;
        }

        if let Some((base, count_expr, consumed)) = parse_grid_lines(&norm, i, None) {
            blocks.push(InputBlock::Vertical {
                base: base.to_lowercase(),
                count: parse_size_ref(&count_expr),
                width: None,
            });
            i += consumed;
            continue;
        }

        if let Some((base, count_expr, width_expr, consumed)) = parse_grid_row(&norm, lines, i) {
            blocks.push(InputBlock::Vertical {
                base: base.to_lowercase(),
                count: parse_size_ref(&count_expr),
                width: Some(parse_size_ref(&width_expr)),
            });
            i += consumed;
            continue;
        }

        // Integer matrix: C_{1,1} \ldots C_{1,N} / \vdots / C_{N,1} \ldots C_{N,N}
        if let Some((base, cols_expr, rows_expr, consumed)) = parse_matrix_block(&norm, i, None, None) {
            blocks.push(InputBlock::Matrix {
                base: base.to_lowercase(),
                rows: parse_size_ref(&rows_expr),
                cols: parse_size_ref(&cols_expr),
            });
            i += consumed;
            continue;
        }

        if let Some((base, len_expr)) = parse_1d_array_line(ln) {
            blocks.push(InputBlock::Array1D {
                base: base.to_lowercase(),
                len: parse_size_ref(&len_expr),
            });
            i += 1;
            continue;
        }

        if !ln.contains("\\ldots") && !ln.contains("\\cdots") && !ln.contains("\\dots") {
            let toks: Vec<String> = ln
                .split_whitespace()
                .map(|t| {
                    let base = if let Some(idx) = t.find('_') { &t[..idx] } else { t };
                    base.to_lowercase()
                })
                .filter(|t| !t.is_empty() && t.starts_with(|c: char| c.is_ascii_alphabetic()))
                .collect();
            if !toks.is_empty() {
                blocks.push(InputBlock::Scalars(toks));
                i += 1;
                continue;
            }
        }

        blocks.push(InputBlock::Unsupported(()));
        i += 1;
    }

    blocks
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_abc454c_bounds() {
        let items = vec![
            r"2\le N\le 3\times 10^5".to_string(),
            r"1\le M\le 3\times 10^5".to_string(),
            r"1\le A_i,B_i\le N".to_string(),
            r"A_i \neq B_i".to_string(),
        ];
        let parsed = parse_constraints(&items);
        for (k, v) in &parsed.bounds {
            eprintln!("bounds[{:?}] = {:?}", k, v);
        }
        eprintln!("var_to_var = {:?}", parsed.var_to_var);
        eprintln!("var_not_eq = {:?}", parsed.var_not_eq);
        eprintln!("skipped = {:?}", parsed.skipped);
        // A_i != B_i is element-wise — should be in skipped, not var_not_eq
        assert!(parsed.var_not_eq.is_empty(), "element-wise != should not be in var_not_eq");
        assert!(parsed.skipped.iter().any(|s| s.contains("neq") || s.contains("!=")),
            "element-wise != should appear in skipped: {:?}", parsed.skipped);
    }

    #[test]
    fn debug_abc454c_blocks() {
        use rand::SeedableRng;
        let lines = vec![
            "N M".to_string(),
            "A_1 B_1".to_string(),
            "A_2 B_2".to_string(),
            r"\vdots".to_string(),
            "A_M B_M".to_string(),
        ];
        let blocks = parse_input_blocks(&lines);
        let items = vec![
            r"2\le N\le 3\times 10^5".to_string(),
            r"1\le M\le 3\times 10^5".to_string(),
            r"1\le A_i,B_i\le N".to_string(),
            r"A_i \neq B_i".to_string(),
        ];
        let parsed = parse_constraints(&items);
        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);
        let input = super::super::generate::generate_random_input(
            &blocks,
            &parsed,
            &mut rng,
            &super::super::generate::CaseStrategy::Deterministic(
                super::super::generate::DeterministicStrategy::AllMax,
            ),
        ).unwrap();
        let ls: Vec<&str> = input.lines().collect();
        eprintln!("AllMax first line: {:?}", ls.first());
        eprintln!("AllMax second line: {:?}", ls.get(1));
    }

    #[test]
    fn debug_abc441c_blocks() {
        use rand::SeedableRng;
        let raw = vec!["N K X".to_string(), r"A_1 A_2 \ldots A_N".to_string()];
        let blocks = parse_input_blocks(&raw);
        assert!(matches!(blocks.get(1), Some(InputBlock::Array1D { .. })), "line 2 should be Array1D, got {:?}", blocks.get(1));
        let items = vec![
            r"1 \leq K \leq N \leq 3\times 10^5".to_string(),
            r"1 \leq A_i \leq 10^9".to_string(),
            r"1 \leq X \leq 3\times 10^{14}".to_string(),
        ];
        let parsed = parse_constraints(&items);
        eprintln!("bounds n: {:?}", parsed.bounds.get("n"));
        let mut rng = rand::rngs::SmallRng::seed_from_u64(42);
        let input = super::super::generate::generate_random_input(
            &blocks,
            &parsed,
            &mut rng,
            &super::super::generate::CaseStrategy::Random(
                super::super::generate::RandomStrategy::SmallSize(1),
            ),
        ).unwrap();
        eprintln!("SmallSize(1) input: {:?}", input.lines().next());
        let first_line = input.lines().next().unwrap();
        let n: i64 = first_line.split_whitespace().next().unwrap().parse().unwrap();
        assert_eq!(n, 1, "SmallSize(1) should give N=1, got N={}", n);
    }

    #[test]
    fn string_constraint_lower() {
        let items = vec![
            r"S は英小文字からなる長さ N の文字列".to_string(),
            r"1\le N\le 10".to_string(),
        ];
        let parsed = parse_constraints(&items);
        assert!(parsed.string_vars.contains_key("s"), "s should be a string var");
        assert!(!parsed.bounds.contains_key("s"), "s should not be in bounds");
    }

    #[test]
    fn string_constraint_explicit_charset() {
        // After HTML processing, <code>A</code> etc. become 「A」 markers.
        let items = vec![
            "S は 「A」, 「B」, 「C」 からなる長さ N の文字列".to_string(),
            r"1\le N\le 500000".to_string(),
        ];
        let parsed = parse_constraints(&items);
        assert!(parsed.string_vars.contains_key("s"), "s should be a string var");
        let spec = parsed.string_vars.get("s").unwrap();
        if let super::CharSet::Explicit(chars) = &spec.charset {
            assert_eq!(*chars, vec!['A', 'B', 'C']);
        } else {
            panic!("expected Explicit charset");
        }
    }

    #[test]
    fn enum_constraint() {
        let items = vec!["B_i は 1,2 のいずれか (1 \\leq i \\leq N)".to_string()];
        let parsed = parse_constraints(&items);
        let b = parsed.bounds.get("b").expect("b should be in bounds");
        if let BoundVal::Set(vals) = &b.hi {
            assert_eq!(*vals, vec![1, 2]);
        } else {
            panic!("expected Set");
        }
    }

    #[test]
    fn sum_constraint_t_limit() {
        let items = vec![
            r"1\le T\le 50000".to_string(),
            r"1\le N\le 3\times 10^5".to_string(),
            "T 個のテストケースにおける N の総和は 3\\times 10^5 以下".to_string(),
        ];
        let parsed = parse_constraints(&items);
        let t_hi = match &parsed.bounds.get("t").unwrap().hi {
            BoundVal::Lit(n) => *n,
            _ => panic!("expected Lit"),
        };
        // limit=300000, lo(N)=1 → sum_based_t_hi=200; T hi=50000 → capped to 200
        assert_eq!(t_hi, 200, "t_hi should be 200, got {}", t_hi);
        // N.hi is capped to limit/T_cap = 300000/200 = 1500
        let n_hi = match &parsed.bounds.get("n").unwrap().hi {
            BoundVal::Lit(n) => *n,
            _ => panic!("expected Lit for n.hi"),
        };
        assert_eq!(n_hi, 1500, "n.hi should be 1500, got {}", n_hi);
        // SumConstraint should be recorded
        assert!(parsed.sum_constraints.iter().any(|sc| sc.inner_var == "n" && sc.limit == 300000));
    }

    #[test]
    fn japanese_sentence_constraint() {
        // "N は 1 \le N \le 50 を満たす整数" style (abc453-a)
        let items = vec![
            "N は 1 \\le N \\le 50 を満たす整数".to_string(),
            "S は英小文字からなる長さ N の文字列".to_string(),
        ];
        let parsed = parse_constraints(&items);
        let n_bound = parsed.bounds.get("n").expect("n should have bounds");
        assert!(matches!(n_bound.lo, BoundVal::Lit(1)), "n.lo should be 1");
        let n_hi = match &n_bound.hi {
            BoundVal::Lit(n) => *n,
            _ => panic!("expected Lit for n.hi, got {:?}", n_bound.hi),
        };
        assert_eq!(n_hi, 50, "n.hi should be 50");
        assert!(parsed.string_vars.contains_key("s"), "s should be a string var");
    }

    #[test]
    fn abs_length_constraint() {
        // |S| \le 100 style constraint refines string var length
        let items = vec![
            r"S は英小文字からなる文字列".to_string(),
            r"1 \le |S| \le 100".to_string(),
        ];
        let parsed = parse_constraints(&items);
        let spec = parsed.string_vars.get("s").expect("s should be string var");
        assert!(matches!(spec.lo_len, BoundVal::Lit(1)), "lo_len should be Lit(1)");
        assert!(matches!(spec.hi_len, BoundVal::Lit(100)), "hi_len should be Lit(100)");
    }

    #[test]
    fn sum_constraint_records_sum_constraint() {
        // N.hi is capped to limit/T_cap; SumConstraint is stored for generation-time checking.
        let items = vec![
            r"1\le T\le 200000".to_string(),
            r"1\le N\le 200000".to_string(),
            "T 個のテストケースにおける N の総和は 2\\times 10^5 以下".to_string(),
        ];
        let parsed = parse_constraints(&items);
        // N.hi capped to limit/T_cap = 200000/200 = 1000
        let n_hi = match &parsed.bounds.get("n").unwrap().hi {
            BoundVal::Lit(n) => *n,
            _ => panic!("expected Lit"),
        };
        assert_eq!(n_hi, 1000, "n.hi should be 1000, got {}", n_hi);
        // SumConstraint is recorded with limit 200000
        let sc = parsed.sum_constraints.iter().find(|sc| sc.inner_var == "n")
            .expect("should have SumConstraint for n");
        assert_eq!(sc.limit, 200000);
    }
}

#[test]
fn debug_abc443d_sum() {
    let items = vec![
        r"1 \le T \le 50000".to_string(),
        r"2 \le N \le 3 \times 10^5".to_string(),
        r"1 \le R_i \le N".to_string(),
        "ひとつの入力について、 N の総和は 3 \\times 10^5 を超えない".to_string(),
    ];
    let parsed = parse_constraints(&items);
    eprintln!("bounds[t] = {:?}", parsed.bounds.get("t"));
    eprintln!("bounds[n] = {:?}", parsed.bounds.get("n"));
    eprintln!("sum_constraints = {:?}", parsed.sum_constraints);
    eprintln!("skipped = {:?}", parsed.skipped);
    assert!(!parsed.sum_constraints.is_empty(), "sum constraint not detected");
    assert!(matches!(parsed.bounds["t"].hi, BoundVal::Lit(200)), "T not capped");
    assert!(matches!(parsed.bounds["n"].hi, BoundVal::Lit(1500)), "N not capped");
}
