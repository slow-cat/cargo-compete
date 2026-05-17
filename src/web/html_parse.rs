use regex::Regex;

// ── Types ────────────────────────────────────────────────────────────────────

/// Minimal representation of a single task section (A, B, C, ...).
#[derive(Debug, Clone)]
pub(crate) struct TaskSection {
    pub(crate) letter: String,
    pub(crate) input_blocks: Vec<Vec<String>>,
    pub(crate) constraints_items: Vec<String>,
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn strip_tags(html: &str) -> String {
    let re = Regex::new(r"(?s)<.*?>").expect("invalid regex");
    let mut s = re.replace_all(html, "").to_string();
    s = s.replace("&lt;", "<");
    s = s.replace("&gt;", ">");
    s = s.replace("&amp;", "&");
    s
}

fn extract_constraints_items(seg: &str) -> Vec<String> {
    let code_re = Regex::new(r"<code>([^<]*)</code>").unwrap();
    let li_re = Regex::new(r"(?s)<li>(.*?)</li>").unwrap();
    for key in ["制約", "Constraints"] {
        let re = Regex::new(&format!(
            r"(?s)<h3>{}</h3>.*?<ul>(.*?)</ul>",
            regex::escape(key)
        ))
        .unwrap();
        if let Some(cap) = re.captures(seg) {
            let ul = cap.get(1).unwrap().as_str();
            let mut items = Vec::new();
            for li in li_re.captures_iter(ul) {
                let li_html = li.get(1).unwrap().as_str();
                // Preserve char values from <code> tags when multiple are present.
                // Multiple <code> tags indicate an enumeration of possible values.
                let processed = if li_html.matches("<code>").count() >= 2 {
                    code_re.replace_all(li_html, "「$1」").to_string()
                } else {
                    li_html.to_string()
                };
                let txt = strip_tags(&processed).trim().to_string();
                if !txt.is_empty() {
                    items.push(txt);
                }
            }
            return items;
        }
    }
    Vec::new()
}

/// Convert a symbol to a snake-case identifier suitable for Rust bindings.
pub(super) fn snake(s: &str) -> String {
    let mut out = String::new();
    let mut prev_is_underscore = false;
    for ch in s.chars() {
        let c = if ch.is_ascii_alphanumeric() { ch } else { '_' };
        if c == '_' {
            if !prev_is_underscore {
                out.push('_');
            }
            prev_is_underscore = true;
        } else {
            out.push(c.to_ascii_lowercase());
            prev_is_underscore = false;
        }
    }
    out.trim_matches('_').to_string()
}

/// Convert a LaTeX-like symbol expression to a Rust-ish expression.
///
/// Examples: `N` → `n`, `N-1` → `n-1`, `5N` → `5*n`.
fn sym_expr(s: &str) -> String {
    let mut t = s.trim().replace(' ', "");
    t = t.replace('\\', "");
    if let Some((a, b)) = t.split_once('-') {
        if b.chars().all(|c| c.is_ascii_digit()) {
            return format!("{}-{}", snake(a), b);
        }
    }
    let coef_re = Regex::new(r"^(\d+)([A-Za-z]+)$").unwrap();
    if let Some(cap) = coef_re.captures(&t) {
        return format!("{}*{}", &cap[1], snake(&cap[2]));
    }
    if t.chars().all(|c| c.is_ascii_alphabetic()) {
        return snake(&t);
    }
    t
}

/// Convert a 1-based/0-based indexed last term into a length expression.
fn len_expr(first_idx: &str, last_raw: &str) -> String {
    if first_idx == "0" {
        let mm = Regex::new(r"^([A-Za-z]+)-1$").unwrap();
        if let Some(c2) = mm.captures(last_raw) {
            snake(c2.get(1).unwrap().as_str())
        } else {
            format!("({})+1", sym_expr(last_raw))
        }
    } else {
        sym_expr(last_raw)
    }
}

// ── Public parsing utilities ──────────────────────────────────────────────────

/// Detect lines like `case_i` that indicate testcase placeholders.
pub(crate) fn is_case_placeholder_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("case") && (l.contains('_') || l.contains("\\mathrm"))
}

/// Detect lines like `query_i` that indicate query placeholders.
pub(crate) fn is_query_placeholder_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("query") && (l.contains('_') || l.contains("\\mathrm") || l.contains("\\text"))
}

/// Normalize a constraint string for simple parsing.
pub(crate) fn normalize_constraint(s: &str) -> String {
    let mut t = s.to_string();
    t = t
        .replace("≤", "<=")
        .replace("≦", "<=")
        .replace("≧", ">=")
        .replace("≥", ">=");
    t = t.replace("\\leq", "<=").replace("\\le", "<=");
    t = t.replace("\\geq", ">=").replace("\\ge", ">=");
    t = t.replace("−", "-");
    t = t.replace("\\times", "*").replace("×", "*");
    t = t.replace(' ', "");
    t
}

/// Extract a base variable name from a token like `A_i` or `A_{i,j}`.
pub(crate) fn base_var(tok: &str) -> Option<String> {
    let mut t = tok.to_string();
    t = t
        .replace("\\mathrm", "")
        .replace("\\text", "")
        .replace("\\rm", "");
    t = t.replace(['{', '}'], "");
    t = t.replace('|', "");
    t = t.replace('\\', "");
    if t.is_empty() {
        return None;
    }
    let start = t
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic())
        .map(|(i, _)| i)?;
    let t = &t[start..];
    let mut base = t;
    if let Some((b, _)) = base.split_once('_') {
        base = b;
    }
    if let Some((b, _)) = base.split_once('[') {
        base = b;
    }
    if base.is_empty() {
        return None;
    }
    Some(snake(base))
}

pub(crate) fn base_vars(tok: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in tok.split(',') {
        if let Some(b) = base_var(part) {
            out.push(b);
        }
    }
    out
}

/// Normalize a single input-format line for pattern matching.
pub(crate) fn normalize_line(line: &str) -> String {
    let mut s = line.trim().to_string();
    s = s.replace("\\cdots", "\\ldots").replace("\\dots", "\\ldots");
    s = s.replace("\\vdots", " \\vdots ");
    s = s.replace("\\ldots", " \\ldots ");

    let underscore_re = Regex::new(r"\s*_\s*").unwrap();
    s = underscore_re.replace_all(&s, "_").to_string();

    let comma_re = Regex::new(r",\s+").unwrap();
    s = comma_re.replace_all(&s, ",").to_string();
    let brace_left_re = Regex::new(r"\{\s+").unwrap();
    s = brace_left_re.replace_all(&s, "{").to_string();
    let brace_right_re = Regex::new(r"\s+\}").unwrap();
    s = brace_right_re.replace_all(&s, "}").to_string();

    // Split concatenated tokens like S_{1,1}S_{1,2}, C[1][1]C[1][2], c_1c_2
    let brace_concat_re = Regex::new(r"\}([A-Za-z\\])").unwrap();
    s = brace_concat_re.replace_all(&s, "} $1").to_string();
    let bracket_concat_re = Regex::new(r"\]([A-Za-z\\])").unwrap();
    s = bracket_concat_re.replace_all(&s, "] $1").to_string();
    let digit_concat_re = Regex::new(r"([A-Za-z]_\d+)([A-Za-z\\])").unwrap();
    s = digit_concat_re.replace_all(&s, "$1 $2").to_string();

    let ws_re = Regex::new(r"\s+").unwrap();
    s = ws_re.replace_all(&s, " ").to_string();
    s.trim().to_string()
}

/// Check if normalization increased token count (used to detect concatenation).
pub(super) fn is_concat_hint(orig: &str, norm: &str) -> bool {
    let o = orig.split_whitespace().count();
    let n = norm.split_whitespace().count();
    n > o
}

/// Determine whether a symbol should be treated as a string (`Chars`).
pub(crate) fn is_string_symbol(sym: &str) -> bool {
    matches!(sym.to_ascii_uppercase().as_str(), "S" | "T" | "U" | "X")
}

/// Parse task sections from the `tasks_print` HTML.
pub(crate) fn parse_task_sections(task_html: &str) -> Vec<TaskSection> {
    let span_re = Regex::new(r#"(?s)<span class="h2">\s*([A-Z])\s*-\s*([^<]+)</span>"#)
        .expect("invalid regex");
    let mut spans: Vec<(usize, usize, String, String)> = Vec::new();
    for cap in span_re.captures_iter(task_html) {
        let m = cap.get(0).unwrap();
        let letter = cap.get(1).unwrap().as_str().trim().to_string();
        let title = cap.get(2).unwrap().as_str().trim().to_string();
        spans.push((m.start(), m.end(), letter, title));
    }

    let mut out = Vec::new();
    let pre_re = Regex::new(r"(?s)<pre>(.*?)</pre>").expect("invalid regex");
    for idx in 0..spans.len() {
        let (start, _end, letter, _title) = spans[idx].clone();
        let end = if idx + 1 < spans.len() {
            spans[idx + 1].0
        } else {
            task_html.len()
        };
        let seg = &task_html[start..end];

        let in_pos = seg.find(r"<h3>入力</h3>");
        if in_pos.is_none() {
            continue;
        }
        let in_pos = in_pos.unwrap();
        let out_pos = seg.find(r"<h3>出力</h3>").unwrap_or(seg.len());
        let inp = &seg[in_pos..out_pos];

        let mut blocks: Vec<Vec<String>> = Vec::new();
        for cap in pre_re.captures_iter(inp) {
            let pre = cap.get(1).unwrap().as_str();
            let txt = strip_tags(pre);
            let lines: Vec<String> = txt
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect();
            blocks.push(lines);
        }
        let constraints_items = extract_constraints_items(seg);
        out.push(TaskSection {
            letter,
            input_blocks: blocks,
            constraints_items,
        });
    }
    out
}

/// Parse a single indexed token.
///
/// Supports underscore-form (`A_{1,2}`) and bracket-form (`C[1][2]`).
pub(super) fn parse_indexed_token(token: &str) -> Option<(String, Vec<String>)> {
    let t = token.trim();
    // Bracket form: C[1][2]
    let bracket_re = Regex::new(r"^([A-Za-z]+)((?:\[[^\]]+\])+)$").unwrap();
    if let Some(cap) = bracket_re.captures(t) {
        let base = cap.get(1)?.as_str().to_string();
        let rest = cap.get(2)?.as_str();
        let idx_re = Regex::new(r"\[([^\]]+)\]").unwrap();
        let mut idxs = Vec::new();
        for c in idx_re.captures_iter(rest) {
            let idx = c.get(1)?.as_str().trim().to_string();
            if !idx.is_empty() {
                idxs.push(idx);
            }
        }
        if !idxs.is_empty() {
            return Some((base, idxs));
        }
    }

    // Underscore form: A_1, A_{1,2}
    let us_re = Regex::new(r"^([A-Za-z]+)_(?:\{)?(.+?)(?:\})?$").unwrap();
    if let Some(cap) = us_re.captures(t) {
        let base = cap.get(1)?.as_str().to_string();
        let idxs_raw = cap.get(2)?.as_str();
        let idxs = idxs_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if !idxs.is_empty() {
            return Some((base, idxs));
        }
    }
    None
}

/// Parse a 1D array line with ellipsis.
///
/// Handles `A_1 A_2 \ldots A_N`, `A_1 \ldots A_N`, and `A_0 ... A_{N-1}`.
pub(crate) fn parse_1d_array_line(line: &str) -> Option<(String, String)> {
    let ln = line;
    // NOTE: Rust's `regex` crate does NOT support backreferences like \1.
    let re_full = Regex::new(
        r"^([A-Za-z]+)_(?:\{)?(\d+)(?:\})?\s+([A-Za-z]+)_(?:\{)?(\d+)(?:\})?\s+\\ldots\s+([A-Za-z]+)_(?:\{)?(.+?)(?:\})?$",
    )
    .unwrap();
    if let Some(cap) = re_full.captures(ln) {
        let base1 = cap.get(1)?.as_str();
        let first_idx = cap.get(2)?.as_str();
        let base2 = cap.get(3)?.as_str();
        let base3 = cap.get(5)?.as_str();
        if base1 != base2 || base1 != base3 {
            return None;
        }
        let last_raw = cap
            .get(6)?
            .as_str()
            .trim()
            .trim_matches('{')
            .trim_matches('}');
        return Some((base1.to_string(), len_expr(first_idx, last_raw)));
    }

    let re_short = Regex::new(
        r"^([A-Za-z]+)_(?:\{)?(\d+)(?:\})?\s+\\ldots\s+([A-Za-z]+)_(?:\{)?(.+?)(?:\})?$",
    )
    .unwrap();
    let cap = re_short.captures(ln)?;
    let base1 = cap.get(1)?.as_str();
    let first_idx = cap.get(2)?.as_str();
    let base2 = cap.get(3)?.as_str();
    if base1 != base2 {
        return None;
    }
    let last_raw = cap
        .get(4)?
        .as_str()
        .trim()
        .trim_matches('{')
        .trim_matches('}');
    Some((base1.to_string(), len_expr(first_idx, last_raw)))
}

/// Parse a fixed 1D array line without ellipsis, e.g. `A_1 A_2 A_3`.
pub(super) fn parse_fixed_indexed_line(line: &str) -> Option<(String, usize)> {
    let toks: Vec<&str> = line.split_whitespace().collect();
    if toks.len() < 2 {
        return None;
    }
    let mut base: Option<String> = None;
    let mut first_idx: Option<i32> = None;
    let mut prev_idx: Option<i32> = None;
    for tok in &toks {
        let (b, idxs) = parse_indexed_token(tok)?;
        if idxs.len() != 1 {
            return None;
        }
        let idx = idxs[0].parse::<i32>().ok()?;
        if let Some(b0) = &base {
            if *b0 != b {
                return None;
            }
        } else {
            base = Some(b);
            first_idx = Some(idx);
        }
        if let Some(prev) = prev_idx {
            if idx != prev + 1 {
                return None;
            }
        }
        prev_idx = Some(idx);
    }
    let base = base?;
    let first_idx = first_idx?;
    let last_idx = prev_idx?;
    let len = (last_idx - first_idx + 1) as usize;
    Some((base, len))
}

/// Parse repeated N-tuples like `x_1 y_1 ...` ... `x_M y_M ...`.
pub(crate) fn parse_n_repeat(lines: &[String], idx: usize) -> Option<(Vec<String>, String, usize)> {
    fn parse_first_line(line: &str) -> Option<(Vec<String>, String)> {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() < 2 {
            return None;
        }
        let mut bases = Vec::new();
        let mut first_idx: Option<String> = None;
        for tok in toks {
            let (base, idxs) = parse_indexed_token(tok)?;
            if idxs.len() != 1 {
                return None;
            }
            let idx = idxs[0].clone();
            if let Some(prev) = &first_idx {
                if *prev != idx {
                    return None;
                }
            } else {
                if !idx.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                first_idx = Some(idx);
            }
            bases.push(base);
        }
        Some((bases, first_idx?))
    }

    fn parse_last_line(line: &str, bases: &[String]) -> Option<String> {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() != bases.len() {
            return None;
        }
        let mut last_idx: Option<String> = None;
        for (tok, base) in toks.into_iter().zip(bases.iter()) {
            let (b, idxs) = parse_indexed_token(tok)?;
            if &b != base || idxs.len() != 1 {
                return None;
            }
            let idx = idxs[0].clone();
            if let Some(prev) = &last_idx {
                if *prev != idx {
                    return None;
                }
            } else {
                last_idx = Some(idx);
            }
        }
        last_idx
    }

    let (bases, first_idx) = parse_first_line(lines.get(idx)?)?;

    let mut last_idx: Option<String> = None;
    let mut last_found: Option<usize> = None;
    let mut j = idx + 1;
    while j < lines.len() && j < idx + 12 {
        if lines[j].contains("\\vdots")
            || lines[j].contains("\\ldots")
            || lines[j].contains("\\cdots")
            || lines[j].contains("\\dots")
        {
            j += 1;
            continue;
        }
        if let Some(idx_expr) = parse_last_line(&lines[j], &bases) {
            last_idx = Some(idx_expr);
            last_found = Some(j);
            j += 1;
            continue;
        }
        if last_found.is_some() {
            break;
        }
        j += 1;
    }
    let last_idx = last_idx?;
    let count_expr = len_expr(&first_idx, &last_idx);
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    Some((bases, count_expr, consumed))
}

/// Parse vertical scalars like `B_1`, `\vdots`, `B_N`.
pub(crate) fn parse_vertical_scalars(lines: &[String], idx: usize) -> Option<(String, String, usize)> {
    let re = Regex::new(r"^([A-Za-z]+)_(?:\{)?1(?:\})?$").unwrap();
    let cap = re.captures(lines.get(idx)?)?;
    let base = cap.get(1)?.as_str();
    if base.eq_ignore_ascii_case("S") {
        return None;
    }
    let last_re = Regex::new(&format!(r"^{}_(?:\{{)?(.+?)(?:\}})?$", regex::escape(base))).unwrap();
    let mut last: Option<String> = None;
    let mut last_found: Option<usize> = None;
    let mut j = idx + 1;
    while j < lines.len() && j < idx + 8 {
        if lines[j].contains("\\vdots") {
            j += 1;
            continue;
        }
        if let Some(c2) = last_re.captures(&lines[j]) {
            last = Some(c2.get(1).unwrap().as_str().to_string());
            last_found = Some(j);
            j += 1;
            continue;
        }
        if last_found.is_some() {
            break;
        }
        j += 1;
    }
    let last = last?;
    let count_expr = sym_expr(last.trim_matches('{').trim_matches('}'));
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    Some((base.to_string(), count_expr, consumed))
}

/// Compact representation of a row with ellipsis, e.g. `A_{i,1} ... A_{i,W}`.
#[derive(Debug, Clone)]
pub(crate) struct RowPattern {
    pub(crate) base: String,
    pub(crate) prefix: Vec<String>,
    pub(crate) col_first: String,
    pub(crate) col_last: String,
}

/// Parse a row containing `\ldots` and indexed tokens.
pub(crate) fn parse_row_with_ellipsis(line: &str) -> Option<RowPattern> {
    let toks: Vec<&str> = line.split_whitespace().collect();
    if !toks.contains(&"\\ldots") {
        return None;
    }
    let first = toks.first()?;
    let last = toks.last()?;
    let (base1, idxs1) = parse_indexed_token(first)?;
    let (base2, idxs2) = parse_indexed_token(last)?;
    if base1 != base2 || idxs1.len() != idxs2.len() || idxs1.len() < 2 {
        return None;
    }
    let prefix1 = idxs1[..idxs1.len() - 1].to_vec();
    let prefix2 = idxs2[..idxs2.len() - 1].to_vec();
    if prefix1 != prefix2 {
        return None;
    }
    Some(RowPattern {
        base: base1,
        prefix: prefix1,
        col_first: idxs1.last()?.to_string(),
        col_last: idxs2.last()?.to_string(),
    })
}

/// Parse a row with fixed, explicit columns (no ellipsis).
fn parse_row_fixed(line: &str) -> Option<(String, String, usize)> {
    let toks: Vec<&str> = line.split_whitespace().collect();
    if toks.len() < 2 {
        return None;
    }
    let mut base: Option<String> = None;
    let mut row_idx: Option<String> = None;
    let mut prev_col: Option<i32> = None;
    for tok in &toks {
        let (b, idxs) = parse_indexed_token(tok)?;
        if idxs.len() != 2 {
            return None;
        }
        if let Some(b0) = &base {
            if *b0 != b {
                return None;
            }
        } else {
            base = Some(b);
        }
        if let Some(r0) = &row_idx {
            if *r0 != idxs[0] {
                return None;
            }
        } else {
            row_idx = Some(idxs[0].clone());
        }
        let col = idxs[1].parse::<i32>().ok()?;
        if let Some(prev) = prev_col {
            if col != prev + 1 {
                return None;
            }
        }
        prev_col = Some(col);
    }
    Some((base?, row_idx?, toks.len()))
}

/// Parse a grid of concatenated row-strings like `S_{1,1}S_{1,2}...`.
///
/// Returns `(base, type_expr, consumed_lines)` where the type is `[Chars; H]`.
pub(super) fn parse_grid_row_block(
    lines: &[String],
    orig_lines: &[String],
    idx: usize,
    known_h: Option<&str>,
) -> Option<(String, String, usize)> {
    let row = parse_row_with_ellipsis(lines.get(idx)?)?;
    if row.prefix.len() != 1 {
        return None;
    }
    let concat = is_concat_hint(orig_lines.get(idx)?, lines.get(idx)?);
    if !concat && !row.base.eq_ignore_ascii_case("S") {
        return None;
    }
    let first_row = row.prefix[0].clone();
    let mut last_row: Option<String> = None;
    let mut last_found: Option<usize> = None;
    let mut j = idx + 1;
    while j < lines.len() && j < idx + 12 {
        if lines[j].contains("\\vdots") {
            j += 1;
            continue;
        }
        if let Some(r2) = parse_row_with_ellipsis(&lines[j]) {
            if r2.base == row.base && r2.prefix.len() == 1 {
                last_row = Some(r2.prefix[0].clone());
                last_found = Some(j);
                j += 1;
                continue;
            }
        }
        if last_found.is_some() {
            break;
        }
        j += 1;
    }
    let last_row = last_row?;
    let h_expr = known_h
        .map(|h| h.to_string())
        .unwrap_or_else(|| len_expr(&first_row, &last_row));
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    Some((snake(&row.base), format!("[Chars; {}]", h_expr), consumed))
}

/// Public wrapper: parse a grid row block and return `(base, count_expr, width_expr, consumed)`.
/// Used by random-test input generation (`parse.rs` `parse_input_blocks`).
pub(crate) fn parse_grid_row(
    lines: &[String],
    orig_lines: &[String],
    idx: usize,
) -> Option<(String, String, String, usize)> {
    let (base, type_str, consumed) = parse_grid_row_block(lines, orig_lines, idx, None)?;
    let count = type_str
        .trim_start_matches("[Chars; ")
        .trim_end_matches(']')
        .to_string();
    let width = parse_row_with_ellipsis(lines.get(idx)?)
        .map(|row| len_expr(&row.col_first, &row.col_last))
        .unwrap_or_else(|| count.clone());
    Some((base, count, width, consumed))
}

/// Parse a numeric matrix block with ellipsis in each row.
pub(crate) fn parse_matrix_block(
    lines: &[String],
    idx: usize,
    known_h: Option<&str>,
    known_w: Option<&str>,
) -> Option<(String, String, String, usize)> {
    let row = parse_row_with_ellipsis(lines.get(idx)?)?;
    if row.prefix.len() != 1 {
        return None;
    }
    let first_row = row.prefix[0].clone();
    let w_expr = known_w
        .map(|w| w.to_string())
        .unwrap_or_else(|| len_expr(&row.col_first, &row.col_last));

    let mut last_row: Option<String> = None;
    let mut last_found: Option<usize> = None;
    let mut j = idx + 1;
    while j < lines.len() && j < idx + 12 {
        if lines[j].contains("\\vdots") {
            j += 1;
            continue;
        }
        if let Some(r2) = parse_row_with_ellipsis(&lines[j]) {
            if r2.base == row.base && r2.prefix.len() == 1 {
                last_row = Some(r2.prefix[0].clone());
                last_found = Some(j);
                j += 1;
                continue;
            }
        }
        if last_found.is_some() {
            break;
        }
        j += 1;
    }
    let last_row = last_row?;
    let h_expr = known_h
        .map(|h| h.to_string())
        .unwrap_or_else(|| len_expr(&first_row, &last_row));
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    Some((row.base, w_expr, h_expr, consumed))
}

/// Parse a numeric matrix with explicit rows and columns (no ellipsis).
pub(super) fn parse_matrix_fixed_block(lines: &[String], idx: usize) -> Option<(String, usize, usize, usize)> {
    let (base, row_idx, col_count) = parse_row_fixed(lines.get(idx)?)?;
    let mut row_count = 1usize;
    let mut j = idx + 1;
    while j < lines.len() {
        if let Some((b2, r2, c2)) = parse_row_fixed(&lines[j]) {
            if b2 == base && c2 == col_count {
                if let (Ok(r0), Ok(r1)) = (row_idx.parse::<i32>(), r2.parse::<i32>()) {
                    if r1 == r0 + row_count as i32 {
                        row_count += 1;
                        j += 1;
                        continue;
                    }
                }
            }
        }
        break;
    }
    if row_count < 2 {
        return None;
    }
    Some((base, col_count, row_count, row_count))
}

/// Parse a 3D array like `S_{f,h,1} ... S_{f,h,W}`.
pub(super) fn parse_3d_array_block(
    lines: &[String],
    idx: usize,
) -> Option<(String, String, String, String, usize)> {
    let row = parse_row_with_ellipsis(lines.get(idx)?)?;
    if row.prefix.len() != 2 {
        return None;
    }
    let first_f = row.prefix[0].clone();
    let first_h = row.prefix[1].clone();
    let w_expr = len_expr(&row.col_first, &row.col_last);

    let mut last_f: Option<String> = None;
    let mut last_h: Option<String> = None;
    let mut last_found: Option<usize> = None;
    let mut j = idx + 1;
    while j < lines.len() && j < idx + 32 {
        if lines[j].contains("\\vdots") {
            j += 1;
            continue;
        }
        if let Some(r2) = parse_row_with_ellipsis(&lines[j]) {
            if r2.base == row.base && r2.prefix.len() == 2 {
                last_f = Some(r2.prefix[0].clone());
                last_h = Some(r2.prefix[1].clone());
                last_found = Some(j);
                j += 1;
                continue;
            }
        }
        if last_found.is_some() {
            break;
        }
        j += 1;
    }
    let last_f = last_f?;
    let last_h = last_h?;
    let f_expr = len_expr(&first_f, &last_f);
    let h_expr = len_expr(&first_h, &last_h);
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    Some((row.base, w_expr, h_expr, f_expr, consumed))
}

/// Parse variable-length rows with fixed prefixes and a trailing list.
///
/// Supported forms include:
/// - `L_i a_{i,1} ... a_{i,L_i}`
/// - `P_i C_i f_{i,1} ... f_{i,C_i}`
pub(super) fn parse_varlen_rows(
    lines: &[String],
    idx: usize,
) -> Option<(Vec<String>, String, String, String, usize)> {
    struct VarlenRow {
        prefix_bases: Vec<String>,
        row_idx: String,
        len_base: String,
        elem_base: String,
    }

    fn parse_row(line: &str) -> Option<VarlenRow> {
        if !line.contains("\\ldots") {
            return None;
        }
        let toks: Vec<&str> = line
            .split_whitespace()
            .filter(|t| *t != "\\ldots")
            .collect();
        if toks.len() < 3 {
            return None;
        }

        let parsed = toks
            .iter()
            .map(|t| parse_indexed_token(t))
            .collect::<Option<Vec<_>>>()?;

        let first_elem_pos = parsed.iter().position(|(_, idxs)| idxs.len() == 2)?;
        if first_elem_pos == 0 {
            return None;
        }

        let (elem_base, elem_idxs) = &parsed[first_elem_pos];
        if elem_idxs.len() != 2 || !elem_idxs[1].chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let row_idx = elem_idxs[0].clone();

        let mut prefix_bases = Vec::new();
        for (base, idxs) in parsed.iter().take(first_elem_pos) {
            if idxs.len() != 1 || idxs[0] != row_idx {
                return None;
            }
            prefix_bases.push(base.clone());
        }

        let (last_base, last_idxs) = parsed.last()?;
        if last_base != elem_base || last_idxs.len() != 2 || last_idxs[0] != row_idx {
            return None;
        }
        let len_base = base_var(&last_idxs[1])?;
        if !prefix_bases
            .iter()
            .any(|b| base_var(b).as_deref() == Some(len_base.as_str()))
        {
            return None;
        }

        Some(VarlenRow {
            prefix_bases,
            row_idx,
            len_base,
            elem_base: elem_base.clone(),
        })
    }

    let first = parse_row(lines.get(idx)?)?;
    if !first.row_idx.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut last_row: Option<String> = None;
    let mut last_found: Option<usize> = None;

    let mut j = idx + 1;
    while j < lines.len() && j < idx + 16 {
        if lines[j].contains("\\vdots") {
            j += 1;
            continue;
        }
        if let Some(row) = parse_row(&lines[j]) {
            if row.prefix_bases == first.prefix_bases
                && row.len_base == first.len_base
                && row.elem_base == first.elem_base
            {
                last_row = Some(row.row_idx);
                last_found = Some(j);
                j += 1;
                continue;
            }
        }
        if last_found.is_some() {
            break;
        }
        j += 1;
    }

    let last_row = last_row?;
    let count_expr = len_expr(&first.row_idx, &last_row);
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    Some((
        first.prefix_bases,
        first.len_base,
        first.elem_base,
        count_expr,
        consumed,
    ))
}

/// Parse a vertical grid like `S_1` ... `S_H` (each row is a string).
///
/// Returns `(base, count_expr, consumed_lines)`.
pub(crate) fn parse_grid_lines(
    lines: &[String],
    idx: usize,
    known_h: Option<&str>,
) -> Option<(String, String, usize)> {
    let re = Regex::new(r"^([A-Za-z]+)_(?:\{)?1(?:\})?$").unwrap();
    let cap = re.captures(lines.get(idx)?)?;
    let base = cap.get(1)?.as_str();
    if !base.eq_ignore_ascii_case("S") {
        return None;
    }
    let last_re = Regex::new(r"^S_(?:\{)?(.+?)(?:\})?$").unwrap();
    let mut last: Option<String> = None;
    let mut last_found: Option<usize> = None;
    let mut j = idx + 1;
    while j < lines.len() && j < idx + 8 {
        if lines[j].contains("\\vdots") {
            j += 1;
            continue;
        }
        if let Some(c2) = last_re.captures(&lines[j]) {
            last = Some(c2.get(1).unwrap().as_str().to_string());
            last_found = Some(j);
            j += 1;
            continue;
        }
        if last_found.is_some() {
            break;
        }
        j += 1;
    }
    let last = last?;
    let h_expr = known_h
        .map(|h| h.to_string())
        .unwrap_or_else(|| sym_expr(last.trim_matches('{').trim_matches('}')));
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    Some((snake(base), h_expr, consumed))
}
