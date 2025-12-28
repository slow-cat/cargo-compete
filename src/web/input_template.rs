use crate::shell::Shell;
use anyhow::Context as _;
use camino::{Utf8Path, Utf8PathBuf};
use heck::KebabCase;
use regex::Regex;
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone)]
struct TaskSection {
    letter: String,
    input_blocks: Vec<Vec<String>>,
}

fn strip_tags(html: &str) -> String {
    // Remove tags in a very rough way (AtCoder tasks_print is predictable enough).
    let re = Regex::new(r"(?s)<.*?>").expect("invalid regex");
    let mut s = re.replace_all(html, "").to_string();
    // Minimal HTML entity decoding we actually see in tasks_print.
    s = s.replace("&lt;", "<");
    s = s.replace("&gt;", ">");
    s = s.replace("&amp;", "&");
    s
}

fn is_case_placeholder_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("case") && (l.contains('_') || l.contains("\\mathrm"))
}

fn is_query_placeholder_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("query") && (l.contains('_') || l.contains("\\mathrm") || l.contains("\\text"))
}

fn parse_task_sections(task_html: &str) -> Vec<TaskSection> {
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
        out.push(TaskSection {
            letter,
            input_blocks: blocks,
        });
    }
    out
}

fn snake(s: &str) -> String {
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

fn sym_expr(s: &str) -> String {
    // Convert common AtCoder latex-ish symbols to a Rust-ish expression: N-1, 5N, etc.
    let mut t = s.trim().replace(' ', "");
    t = t.replace('\\', "");
    if let Some((a, b)) = t.split_once('-') {
        if b.chars().all(|c| c.is_ascii_digit()) {
            return format!("{}-{}", snake(a), b);
        }
    }
    // 5N form
    let coef_re = Regex::new(r"^(\d+)([A-Za-z]+)$").unwrap();
    if let Some(cap) = coef_re.captures(&t) {
        return format!("{}*{}", &cap[1], snake(&cap[2]));
    }
    if t.chars().all(|c| c.is_ascii_alphabetic()) {
        return snake(&t);
    }
    t
}

fn is_string_symbol(sym: &str) -> bool {
    matches!(sym.to_ascii_uppercase().as_str(), "S" | "T" | "U" | "X")
}

fn parse_1d_array_line(line: &str) -> Option<(String, String)> {
    // A_1 A_2 \ldots A_N  or A_0 ... A_{N-1}
    let ln = line
        .replace("\\cdots", "\\ldots")
        .replace("\\dots", "\\ldots");
    // NOTE: Rust's `regex` crate does NOT support backreferences like \1.
    // Capture the base name three times and validate equality in code.
    let re = Regex::new(
        r"^([A-Za-z]+)_(?:\{)?(\d+)(?:\})?\s+([A-Za-z]+)_(?:\{)?(\d+)(?:\})?\s+\\ldots\s+([A-Za-z]+)_(?:\{)?(.+?)(?:\})?$",
    )
    .unwrap();
    let cap = re.captures(&ln)?;
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
    let len_expr = if first_idx == "0" {
        // if last is N-1, length is N; else (last+1)
        let mm = Regex::new(r"^([A-Za-z]+)-1$").unwrap();
        if let Some(c2) = mm.captures(last_raw) {
            snake(c2.get(1).unwrap().as_str())
        } else {
            format!("({})+1", sym_expr(last_raw))
        }
    } else {
        sym_expr(last_raw)
    };
    Some((snake(base1), format!("[usize; {}]", len_expr)))
}

fn parse_pair_repeat(lines: &[String], idx: usize) -> Option<(String, String, usize)> {
    // x_1 y_1  ... x_M y_M
    let re = Regex::new(r"^([A-Za-z]+)_\{?\d+\}?\s+([A-Za-z]+)_\{?\d+\}?$").unwrap();
    let cap = re.captures(lines.get(idx)?)?;
    let a = cap.get(1)?.as_str();
    let b = cap.get(2)?.as_str();

    let last_re = Regex::new(&format!(
        r"^{}_(?:\{{)?(.+?)(?:\}})?\s+{}_(?:\{{)?(.+?)(?:\}})?$",
        regex::escape(a),
        regex::escape(b)
    ))
    .unwrap();

    let mut count_expr: Option<String> = None;
    let mut last_found: Option<usize> = None;
    let mut j = idx + 1;
    while j < lines.len() && j < idx + 12 {
        if lines[j].contains("\\vdots") {
            j += 1;
            continue;
        }
        if let Some(c2) = last_re.captures(&lines[j]) {
            count_expr = Some(c2.get(1).unwrap().as_str().to_string());
            last_found = Some(j);
            j += 1;
            continue;
        }
        if last_found.is_some() {
            break;
        }
        j += 1;
    }
    let count_expr = count_expr?;
    let count_expr = sym_expr(count_expr.trim_matches('{').trim_matches('}'));
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    let name = snake(&(a.to_string() + b));
    Some((name, format!("[(usize, usize); {}]", count_expr), consumed))
}

fn parse_vertical_scalars(lines: &[String], idx: usize) -> Option<(String, String, usize)> {
    // B_1 \vdots B_N  -> b: [usize; n]
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
            break;
        }
        j += 1;
    }
    let last = last?;
    let count_expr = sym_expr(last.trim_matches('{').trim_matches('}'));
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    Some((snake(base), format!("[usize; {}]", count_expr), consumed))
}

fn parse_grid_lines(
    lines: &[String],
    idx: usize,
    known_h: Option<&str>,
) -> Option<(String, String, usize)> {
    // S_1 \vdots S_H  -> s: [Chars; h]
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
            break;
        }
        j += 1;
    }
    let last = last?;
    let h_expr = known_h
        .map(|h| h.to_string())
        .unwrap_or_else(|| sym_expr(last.trim_matches('{').trim_matches('}')));
    let consumed = last_found.map(|lf| lf + 1 - idx).unwrap_or(1);
    Some((snake(base), format!("[Chars; {}]", h_expr), consumed))
}

fn guess_input_from_lines(lines: &[String]) -> (Vec<String>, bool) {
    let mut decls: Vec<String> = Vec::new();
    let mut needs_chars = false;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut known_h: Option<String> = None;

    let t_is_testcases = lines
        .iter()
        .any(|l| l.to_ascii_lowercase().contains("case"));

    let mut i = 0usize;
    while i < lines.len() {
        let ln = &lines[i];
        if is_case_placeholder_line(ln) || is_query_placeholder_line(ln) || ln.contains("\\vdots") {
            i += 1;
            continue;
        }

        if let Some((name, ty, consumed)) = parse_grid_lines(lines, i, known_h.as_deref()) {
            needs_chars = true;
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += consumed;
            continue;
        }
        if let Some((name, ty, consumed)) = parse_pair_repeat(lines, i) {
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += consumed;
            continue;
        }
        if let Some((name, ty, consumed)) = parse_vertical_scalars(lines, i) {
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += consumed;
            continue;
        }
        if let Some((name, ty)) = parse_1d_array_line(ln) {
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
                if seen.insert(name.clone()) {
                    decls.push(format!("{name}: usize,"));
                }
                if name == "h" {
                    known_h = Some("h".to_string());
                }
            }
            i += 1;
            continue;
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
                "usize".to_string()
            };
            if seen.insert(name.clone()) {
                decls.push(format!("{name}: {ty},"));
            }
            i += 1;
            continue;
        }

        decls.push(format!("/* TODO: {ln} */"));
        i += 1;
    }

    (decls, needs_chars)
}

fn render_section(task: &TaskSection) -> anyhow::Result<String> {
    let all_lines: Vec<String> = task.input_blocks.iter().flatten().cloned().collect();
    let has_cases = all_lines.iter().any(|l| is_case_placeholder_line(l));
    let has_queries = all_lines.iter().any(|l| is_query_placeholder_line(l));

    let first = task
        .input_blocks
        .first()
        .with_context(|| format!("{}: missing input format <pre>", task.letter))?;
    let (decls, needs_chars) = guess_input_from_lines(first);
    let mut out: Vec<String> = Vec::new();
    if needs_chars {
        out.push("use proconio::{input, marker::Chars};".to_string());
    } else {
        out.push("use proconio::input;".to_string());
    }
    out.push("fn main() {".to_string());

    if !has_cases && !has_queries {
        out.push("    input! {".to_string());
        for d in decls {
            out.push(format!("        {d}"));
        }
        out.push("    }".to_string());
        out.push("}".to_string());
        return Ok(out.join("\n"));
    }

    // Header
    out.push("    input! {".to_string());
    for d in decls {
        out.push(format!("        {d}"));
    }
    out.push("    }".to_string());

    if has_cases {
        if task.input_blocks.len() >= 2 {
            let (case_decls, case_needs_chars) = guess_input_from_lines(&task.input_blocks[1]);
            if case_needs_chars && !needs_chars {
                out[0] = "use proconio::{input, marker::Chars};".to_string();
            }
            out.push("    for _ in 0..t {".to_string());
            out.push("        input! {".to_string());
            for d in case_decls {
                out.push(format!("            {d}"));
            }
            out.push("        }".to_string());
            out.push("        /* TODO: solve testcase */".to_string());
            out.push("    }".to_string());
            out.push("}".to_string());
            return Ok(out.join("\n"));
        }
        out.push("    for _ in 0..t {".to_string());
        out.push("        input! { /* TODO: per-testcase fields */ }".to_string());
        out.push("        /* TODO: solve testcase */".to_string());
        out.push("    }".to_string());
        out.push("}".to_string());
        return Ok(out.join("\n"));
    }

    // Queries
    out.push("    for _ in 0..q {".to_string());
    out.push("        input! { qt: usize }".to_string());
    let mut qtypes: Vec<(i32, Vec<String>)> = Vec::new();
    for b in task.input_blocks.iter().skip(1) {
        if b.len() != 1 {
            continue;
        }
        let toks: Vec<&str> = b[0].split_whitespace().collect();
        if toks.is_empty() {
            continue;
        }
        let qt = toks[0].parse::<i32>().ok();
        if qt.is_none() {
            continue;
        }
        let rest = toks[1..].iter().map(|s| s.to_string()).collect();
        qtypes.push((qt.unwrap(), rest));
    }
    qtypes.sort_by_key(|x| x.0);
    if !qtypes.is_empty() {
        out.push("        match qt {".to_string());
        for (qt, toks) in qtypes {
            if toks.is_empty() {
                out.push(format!("            {qt} => {{}},"));
            } else {
                let inner = toks
                    .iter()
                    .map(|t| format!("{}: usize", snake(t)))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push(format!("            {qt} => {{ input! {{ {inner} }} }},"));
            }
        }
        out.push("            _ => unreachable!(),".to_string());
        out.push("        }".to_string());
    } else {
        out.push("        /* TODO: per-query fields */".to_string());
    }
    out.push("        /* TODO: process query */".to_string());
    out.push("    }".to_string());
    out.push("}".to_string());
    Ok(out.join("\n"))
}

pub(crate) fn generate_template(
    dest_dir: &Utf8Path,
    shell: &mut Shell,
) -> anyhow::Result<Option<HashMap<Utf8PathBuf, String>>> {
    let task_path = dest_dir.join("task.html");
    if !task_path.exists() {
        return Ok(None);
    }
    let html = fs::read_to_string(&task_path).with_context(|| format!("failed to read {task_path}"))?;
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
