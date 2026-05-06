//! Best-effort input template generator for AtCoder `tasks_print` HTML.
//!
//! This module parses `task.html` (the print view of a contest), extracts the
//! "入力" blocks, and heuristically infers `proconio::input!` declarations.
//! It also inserts small helper loops for common patterns like per-row lengths.
//!
//! The intent is to reduce boilerplate for typical A–D style inputs while
//! remaining conservative. When a pattern is ambiguous or unsupported, the
//! generator leaves a human-readable comment instead of guessing incorrectly.
//!
//! Type inference is based on constraints: if a variable is ever bounded by a
//! negative number, it is treated as `i64`; otherwise it defaults to `usize`.
//! Strings (S/T/U/X) and concatenated grid rows are inferred as `Chars`.
//! These heuristics are not perfect, but they usually match AtCoder style.

use super::html_parse::{
    base_var, base_vars, is_case_placeholder_line, is_concat_hint, is_query_placeholder_line,
    is_string_symbol, normalize_constraint, normalize_line, parse_1d_array_line,
    parse_3d_array_block, parse_fixed_indexed_line, parse_grid_lines, parse_grid_row_block,
    parse_indexed_token, parse_matrix_block, parse_matrix_fixed_block, parse_n_repeat,
    parse_task_sections, parse_varlen_rows, parse_vertical_scalars, snake, TaskSection,
};
use crate::shell::Shell;
use anyhow::Context as _;
use camino::{Utf8Path, Utf8PathBuf};
use heck::KebabCase;
use regex::Regex;
use std::collections::HashMap;
use std::fs;

/// Check whether a token looks like a purely numeric expression.
fn is_numeric_expr(tok: &str) -> bool {
    if tok.is_empty() {
        return false;
    }
    tok.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '*' | '/' | '^' | '(' | ')'))
}

/// Check whether a numeric expression is negative after removing outer parens.
fn numeric_is_negative(tok: &str) -> bool {
    let mut t = tok.trim();
    loop {
        let t2 = t.trim_start_matches('(').trim_end_matches(')');
        if t2.len() == t.len() {
            break;
        }
        t = t2;
    }
    t.starts_with('-')
}

/// Determine which base variables should be treated as signed (`i64`).
///
/// Heuristic: if a constraint compares a variable against a negative bound,
/// or uses an absolute value like `|X|`, the base is treated as signed.
pub(crate) fn signed_bases(constraints: &[String]) -> std::collections::HashSet<String> {
    let mut signed = std::collections::HashSet::new();
    let op_re = Regex::new(r"(<=|>=|<|>)").unwrap();
    let abs_re = Regex::new(r"\|([^|]+)\|").unwrap();

    for item in constraints {
        for cap in abs_re.captures_iter(item) {
            for base in base_vars(cap.get(1).unwrap().as_str()) {
                signed.insert(base);
            }
        }

        let norm = normalize_constraint(item);
        let mut tokens = Vec::new();
        let mut ops = Vec::new();
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

        for i in 0..ops.len() {
            let left = tokens[i].clone();
            let right = tokens[i + 1].clone();
            let left_vars = base_vars(&left);
            let right_vars = base_vars(&right);
            let left_num = is_numeric_expr(&left);
            let right_num = is_numeric_expr(&right);

            if !right_vars.is_empty() && left_num && numeric_is_negative(&left) {
                for var in right_vars {
                    signed.insert(var);
                }
            }
            if !left_vars.is_empty() && right_num && numeric_is_negative(&right) {
                for var in left_vars {
                    signed.insert(var);
                }
            }
        }
    }
    signed
}

/// Map a base name to `i64` or `usize` based on constraints.
pub(crate) fn num_ty_for_base(base: &str, signed: &std::collections::HashSet<String>) -> &'static str {
    if signed.contains(&snake(base)) {
        "i64"
    } else {
        "usize"
    }
}

/// Map a raw token (possibly indexed) to `i64` or `usize`.
fn num_ty_for_token(tok: &str, signed: &std::collections::HashSet<String>) -> &'static str {
    if let Some((base, _)) = parse_indexed_token(&normalize_line(tok)) {
        num_ty_for_base(&base, signed)
    } else {
        num_ty_for_base(tok, signed)
    }
}

/// Output of the input inference step.
struct GuessResult {
    decls: Vec<String>,
    needs_chars: bool,
    extra_lines: Vec<String>,
}

/// Infer input declarations from a list of input-format lines.
///
/// This is the main heuristic engine. It tries specialized parsers first
/// (grids, matrices, repeated pairs, etc.), then falls back to scalar lines.
/// Signedness is inferred from constraints and passed via `signed`.
fn guess_input_from_lines(
    lines: &[String],
    signed: &std::collections::HashSet<String>,
) -> GuessResult {
    let mut decls: Vec<String> = Vec::new();
    let mut needs_chars = false;
    let mut extra_lines: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut known_h: Option<String> = None;
    let mut known_w: Option<String> = None;

    let norm_lines: Vec<String> = lines.iter().map(|l| normalize_line(l)).collect();
    let concat_hints: Vec<bool> = lines
        .iter()
        .zip(norm_lines.iter())
        .map(|(o, n)| is_concat_hint(o, n))
        .collect();

    let t_is_testcases = lines
        .iter()
        .any(|l| l.to_ascii_lowercase().contains("case"));

    let mut i = 0usize;
    while i < lines.len() {
        let ln = &norm_lines[i];
        let orig = &lines[i];
        let concat_hint = concat_hints[i];
        if is_case_placeholder_line(ln) || is_query_placeholder_line(ln) || ln.contains("\\vdots") {
            i += 1;
            continue;
        }

        if let Some((prefix_bases, len_base, elem_base, count_expr, consumed)) =
            parse_varlen_rows(&norm_lines, i)
        {
            let elem_name = snake(&elem_base);
            let elem_ty = num_ty_for_base(&elem_base, signed);
            for base in &prefix_bases {
                let name = snake(base);
                let ty = num_ty_for_base(base, signed);
                if seen.insert(name.clone()) {
                    extra_lines.push(format!(
                        "let mut {name}: Vec<{ty}> = Vec::with_capacity({count_expr});"
                    ));
                }
            }
            if seen.insert(elem_name.clone()) {
                extra_lines.push(format!(
                    "let mut {elem_name}: Vec<Vec<{elem_ty}>> = Vec::with_capacity({count_expr});"
                ));
            }

            let len_var = format!("{}_i", snake(&len_base));
            let row_var = format!("{}_row", elem_name);
            let mut input_fields = Vec::new();
            let mut push_lines = Vec::new();
            for base in &prefix_bases {
                let name = snake(base);
                let var = format!("{name}_i");
                let ty = if base_var(base).as_deref() == Some(len_base.as_str()) {
                    "usize".to_string()
                } else {
                    num_ty_for_base(base, signed).to_string()
                };
                input_fields.push(format!("{var}: {ty}"));
                push_lines.push(format!("    {name}.push({var});"));
            }
            input_fields.push(format!("{row_var}: [{elem_ty}; {len_var}]"));

            extra_lines.push(format!("for _ in 0..{count_expr} {{"));
            extra_lines.push(format!("    input! {{ {} }}", input_fields.join(", ")));
            for line in push_lines {
                extra_lines.push(line);
            }
            extra_lines.push(format!("    {elem_name}.push({row_var});"));
            extra_lines.push("}".to_string());
            i += consumed;
            continue;
        }

        if let Some((name, ty, consumed)) =
            parse_grid_row_block(&norm_lines, lines, i, known_h.as_deref())
        {
            needs_chars = true;
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += consumed;
            continue;
        }
        if let Some((base, w_expr, h_expr, f_expr, consumed)) = parse_3d_array_block(&norm_lines, i)
        {
            let name = snake(&base);
            let elem_ty = num_ty_for_base(&base, signed);
            let ty = format!("[[[{elem_ty}; {w_expr}]; {h_expr}]; {f_expr}]");
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += consumed;
            continue;
        }
        if let Some((base, w_expr, h_expr, consumed)) =
            parse_matrix_block(&norm_lines, i, known_h.as_deref(), known_w.as_deref())
        {
            let name = snake(&base);
            let elem_ty = num_ty_for_base(&base, signed);
            let ty = format!("[[{elem_ty}; {w_expr}]; {h_expr}]");
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += consumed;
            continue;
        }
        if let Some((base, col_count, row_count, consumed)) =
            parse_matrix_fixed_block(&norm_lines, i)
        {
            let name = snake(&base);
            let elem_ty = num_ty_for_base(&base, signed);
            let ty = format!("[[{elem_ty}; {col_count}]; {row_count}]");
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += consumed;
            continue;
        }
        if let Some((name, count_expr, consumed)) = parse_grid_lines(&norm_lines, i, known_h.as_deref()) {
            needs_chars = true;
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: [Chars; {count_expr}],"));
            }
            i += consumed;
            continue;
        }
        if let Some((bases, count_expr, consumed)) = parse_n_repeat(&norm_lines, i) {
            let name = snake(&bases.concat());
            let tys = bases
                .iter()
                .map(|b| num_ty_for_base(b, signed))
                .collect::<Vec<_>>()
                .join(", ");
            let ty = format!("[({tys}); {count_expr}]");
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += consumed;
            continue;
        }
        if let Some((base, count_expr, consumed)) = parse_vertical_scalars(&norm_lines, i) {
            let name = snake(&base);
            let elem_ty = num_ty_for_base(&base, signed);
            let ty = format!("[{elem_ty}; {count_expr}]");
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += consumed;
            continue;
        }
        if let Some((base, len_expr)) = parse_1d_array_line(ln) {
            let name = snake(&base);
            let ty = if concat_hint {
                "Chars".to_string()
            } else {
                let elem_ty = num_ty_for_base(&base, signed);
                format!("[{elem_ty}; {len_expr}]")
            };
            if concat_hint {
                needs_chars = true;
            }
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += 1;
            continue;
        }
        if let Some((base, len)) = parse_fixed_indexed_line(ln) {
            let name = snake(&base);
            let elem_ty = num_ty_for_base(&base, signed);
            let ty = format!("[{elem_ty}; {len}]");
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += 1;
            continue;
        }

        // scalar line like "N M"
        if ln.contains(' ')
            && !ln.contains("\\ldots")
            && !ln.contains("\\cdots")
            && !ln.contains("\\dots")
            && !ln.contains('_')
            && !ln.contains('{')
            && !ln.contains('}')
        {
            for tok in ln.split_whitespace() {
                let name = snake(tok);
                let ty = num_ty_for_base(tok, signed);
                if seen.insert(name.clone()) {
                    decls.push(format!("{name}: {ty},"));
                }
                if name == "h" {
                    known_h = Some("h".to_string());
                }
                if name == "w" {
                    known_w = Some("w".to_string());
                }
            }
            i += 1;
            continue;
        }

        // scalar tokens with subscripts like "S_x S_y"
        if ln.contains(' ') && ln.contains('_') && !ln.contains("\\ldots") {
            let mut ok = true;
            let mut candidates: Vec<(String, String)> = Vec::new();
            for tok in ln.split_whitespace() {
                if let Some((base, idxs)) = parse_indexed_token(tok) {
                    if idxs.len() != 1 {
                        ok = false;
                        break;
                    }
                    let name = snake(&format!("{}_{}", base, idxs[0]));
                    let ty = num_ty_for_base(&base, signed);
                    candidates.push((name, ty.to_string()));
                } else if tok.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let name = snake(tok);
                    let ty = num_ty_for_base(tok, signed);
                    candidates.push((name, ty.to_string()));
                } else {
                    ok = false;
                    break;
                }
            }
            if ok {
                for (name, ty) in candidates {
                    if seen.insert(name.clone()) {
                        decls.push(format!("{name}: {ty},"));
                    }
                }
                i += 1;
                continue;
            }
        }

        // single symbol line
        if !ln.contains(' ')
            && !ln.contains("\\ldots")
            && !ln.contains("\\cdots")
            && !ln.contains("\\dots")
        {
            let sym = ln.trim();
            let name = snake(sym);
            let ty = if sym.eq_ignore_ascii_case("T") && t_is_testcases {
                "usize".to_string()
            } else if is_string_symbol(sym) {
                needs_chars = true;
                "Chars".to_string()
            } else {
                num_ty_for_base(sym, signed).to_string()
            };
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += 1;
            continue;
        }

        decls.push(format!("/* TODO: {orig} */"));
        i += 1;
    }

    GuessResult {
        decls,
        needs_chars,
        extra_lines,
    }
}

/// Render a single task section into a Rust `main` template.
fn render_section(task: &TaskSection) -> anyhow::Result<String> {
    let all_lines: Vec<String> = task.input_blocks.iter().flatten().cloned().collect();
    let has_cases = all_lines.iter().any(|l| is_case_placeholder_line(l));
    let has_queries = all_lines.iter().any(|l| is_query_placeholder_line(l));

    let first = task
        .input_blocks
        .first()
        .with_context(|| format!("{}: missing input format <pre>", task.letter))?;
    let signed = signed_bases(&task.constraints_items);
    let GuessResult {
        decls,
        needs_chars,
        extra_lines,
    } = guess_input_from_lines(first, &signed);
    let mut out: Vec<String> = Vec::new();
    if needs_chars {
        out.push("use proconio::{input, marker::Chars};".to_string());
    } else {
        out.push("use proconio::input;".to_string());
    }
    out.push("fn main() {".to_string());

    if !has_cases && !has_queries {
        out.push("    input! {".to_string());
        for d in &decls {
            out.push(format!("        {d}"));
        }
        out.push("    }".to_string());
        for l in &extra_lines {
            out.push(format!("    {l}"));
        }
        out.push("}".to_string());
        return Ok(out.join("\n"));
    }

    // Header
    out.push("    input! {".to_string());
    for d in decls {
        out.push(format!("        {d}"));
    }
    out.push("    }".to_string());
    for l in extra_lines {
        out.push(format!("    {l}"));
    }

    if has_cases {
        if task.input_blocks.len() >= 2 {
            let GuessResult {
                decls: case_decls,
                needs_chars: case_needs_chars,
                extra_lines: case_extra_lines,
            } = guess_input_from_lines(&task.input_blocks[1], &signed);
            if case_needs_chars && !needs_chars {
                out[0] = "use proconio::{input, marker::Chars};".to_string();
            }
            out.push("    for _ in 0..t {".to_string());
            out.push("        input! {".to_string());
            for d in case_decls {
                out.push(format!("            {d}"));
            }
            out.push("        }".to_string());
            for l in case_extra_lines {
                out.push(format!("        {l}"));
            }
            out.push("        /* solve testcase */".to_string());
            out.push("    }".to_string());
            out.push("}".to_string());
            return Ok(out.join("\n"));
        }
        out.push("    for _ in 0..t {".to_string());
        out.push("        input! { /* per-testcase fields */ }".to_string());
        out.push("        /* solve testcase */".to_string());
        out.push("    }".to_string());
        out.push("}".to_string());
        return Ok(out.join("\n"));
    }

    // Queries
    out.push("    for _ in 0..q {".to_string());
    let mut qtypes: Vec<(i32, Vec<String>)> = Vec::new();
    let mut sym_types: Vec<(String, Vec<String>)> = Vec::new();
    let mut sym_name: Option<String> = None;
    let mut mixed_symbol = false;
    for b in task.input_blocks.iter().skip(1) {
        if b.len() != 1 {
            continue;
        }
        let toks: Vec<&str> = b[0].split_whitespace().collect();
        if toks.is_empty() {
            continue;
        }
        if let Ok(qt) = toks[0].parse::<i32>() {
            let rest = toks[1..].iter().map(|s| s.to_string()).collect();
            qtypes.push((qt, rest));
            continue;
        }
        let name = snake(toks[0]);
        if let Some(prev) = &sym_name {
            if *prev != name {
                mixed_symbol = true;
            }
        } else {
            sym_name = Some(name.clone());
        }
        let rest = toks[1..].iter().map(|s| s.to_string()).collect();
        sym_types.push((name, rest));
    }
    if !qtypes.is_empty() {
        qtypes.sort_by_key(|x| x.0);
        out.push("        input! { qt: usize }".to_string());
        out.push("        match qt {".to_string());
        for (qt, toks) in qtypes {
            if toks.is_empty() {
                out.push(format!("            {qt} => {{}},"));
            } else {
                let inner = toks
                    .iter()
                    .map(|t| format!("{}: {}", snake(t), num_ty_for_token(t, &signed)))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push(format!("            {qt} => {{ input! {{ {inner} }} }},"));
            }
        }
        out.push("            _ => unreachable!(),".to_string());
        out.push("        }".to_string());
    } else if !sym_types.is_empty() && !mixed_symbol {
        if sym_types.len() == 1 {
            let toks = &sym_types[0].1;
            let mut all = vec![sym_name.unwrap_or_else(|| "t".to_string())];
            all.extend(toks.iter().map(|t| snake(t)));
            let inner = all
                .iter()
                .map(|t| format!("{t}: {}", num_ty_for_token(t, &signed)))
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!("        input! {{ {inner} }}"));
        } else {
            let qt_name = sym_name.unwrap_or_else(|| "t".to_string());
            let qt_ty = num_ty_for_token(&qt_name, &signed);
            out.push(format!("        input! {{ {qt_name}: {qt_ty} }}"));
            out.push(format!("        match {qt_name} {{"));
            for (idx, (_, toks)) in sym_types.iter().enumerate() {
                let qt = idx + 1;
                if toks.is_empty() {
                    out.push(format!("            {qt} => {{}},"));
                } else {
                    let inner = toks
                        .iter()
                        .map(|t| format!("{}: {}", snake(t), num_ty_for_token(t, &signed)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push(format!("            {qt} => {{ input! {{ {inner} }} }},"));
                }
            }
            out.push("            _ => unreachable!(),".to_string());
            out.push("        }".to_string());
        }
    } else {
        out.push("        /* TODO: per-query fields */".to_string());
    }
    out.push("        /* process query */".to_string());
    out.push("    }".to_string());
    out.push("}".to_string());
    Ok(out.join("\n"))
}

/// Generate templates for all tasks found in `dest_dir/task.html`.
///
/// Returns a map of destination source paths to generated file contents.
pub(crate) fn generate_template(
    dest_dir: &Utf8Path,
    shell: &mut Shell,
) -> anyhow::Result<Option<HashMap<Utf8PathBuf, String>>> {
    let task_path = dest_dir.join("task.html");
    if !task_path.exists() {
        return Ok(None);
    }
    let html =
        fs::read_to_string(&task_path).with_context(|| format!("failed to read {task_path}"))?;
    let sections = parse_task_sections(&html);
    let src_dir = dest_dir.join("src").join("bin");
    let mut out: HashMap<Utf8PathBuf, String> = HashMap::new();
    for task in &sections {
        let src_path = src_dir
            .join(task.letter.to_kebab_case())
            .with_extension("rs");
        match render_section(task) {
            Ok(content) => {
                out.insert(src_path, content);
            }
            Err(err) => {
                shell.warn(format!("render_section failed at {}: {err}", task.letter))?;
            }
        }
    }
    Ok(Some(out))
}

#[cfg(test)]
#[path = "input_template_tests.rs"]
mod tests;
