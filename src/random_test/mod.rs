pub(crate) mod generate;
pub(crate) mod parse;

pub(crate) use generate::{generate_random_input, make_strategy_list, CaseStrategy};
pub(crate) use parse::{
    apply_string_symbol_fallback, parse_constraints, parse_input_blocks, ConstraintParsed,
    InputBlock, TypedBranch,
};

use crate::{shell::Shell, web::input_template::{is_case_placeholder_line, is_query_placeholder_line, parse_task_sections}};
use anyhow::Context as _;
use heck::KebabCase as _;
use maplit::btreemap;
use rand::SeedableRng;
use snowchains_core::{
    color_spec,
    judge::{CommandExpression, Verdict},
    testsuite::{BatchTestSuite, Match, PartialBatchTestCase},
};
use termcolor::Color;
use std::{
    path::Path,
    sync::Arc,
    time::Duration,
};

// ── Task spec loading ────────────────────────────────────────────────────────

fn load_task_spec(
    task_html_path: &Path,
    bin_letter: &str,
    shell: &mut Shell,
) -> anyhow::Result<Option<(ConstraintParsed, Vec<InputBlock>)>> {
    if !task_html_path.exists() {
        shell.warn(format!(
            "task.html not found at {}; skipping random test",
            task_html_path.display()
        ))?;
        return Ok(None);
    }

    let task_html = std::fs::read_to_string(task_html_path)
        .with_context(|| format!("failed to read {}", task_html_path.display()))?;

    let sections = parse_task_sections(&task_html);
    let section = match sections.into_iter().find(|s| s.letter.eq_ignore_ascii_case(bin_letter)) {
        Some(s) => s,
        None => {
            shell.warn(format!(
                "task section '{}' not found in task.html; skipping random test",
                bin_letter
            ))?;
            return Ok(None);
        }
    };

    let mut parsed = parse_constraints(&section.constraints_items);
    let blocks = build_input_blocks(section.input_blocks);
    // Step B: is_string_symbol fallback (requires blocks to be built first)
    apply_string_symbol_fallback(&blocks, &section.constraints_items, &mut parsed);

    Ok(Some((parsed, blocks)))
}

/// Build input blocks from the raw `<pre>` blocks of a task section.
///
/// Handles two outer-repeat patterns:
/// - Case/query placeholder → `OuterRepeat` (T test cases form)
/// - Query placeholder + every subsequent block starts with a literal integer → `TypedRepeat`
fn build_input_blocks(input_blocks: Vec<Vec<String>>) -> Vec<InputBlock> {
    if input_blocks.len() < 2 {
        let input_lines: Vec<String> = input_blocks.into_iter().flatten().collect();
        return parse_input_blocks(&input_lines);
    }

    let first_has_placeholder = input_blocks[0]
        .iter()
        .any(|l| is_case_placeholder_line(l) || is_query_placeholder_line(l));

    if !first_has_placeholder {
        let input_lines: Vec<String> = input_blocks.into_iter().flatten().collect();
        return parse_input_blocks(&input_lines);
    }

    // Outer lines = lines in block 0 without placeholder / vdots
    let outer_lines: Vec<String> = input_blocks[0]
        .iter()
        .filter(|l| {
            !is_case_placeholder_line(l)
                && !is_query_placeholder_line(l)
                && !l.contains("\\vdots")
        })
        .cloned()
        .collect();
    let outer_blocks = parse_input_blocks(&outer_lines);
    let count = outer_blocks
        .iter()
        .find_map(|b| {
            if let InputBlock::Scalars(vars) = b {
                vars.last().map(|v| parse::SizeRef::Var(v.to_lowercase()))
            } else {
                None
            }
        })
        .unwrap_or(parse::SizeRef::Lit(1));

    // Check if the remaining blocks are typed query branches:
    // each branch's first line starts with a literal non-negative integer.
    let first_has_query = input_blocks[0].iter().any(|l| is_query_placeholder_line(l));
    let branches_are_typed = first_has_query && input_blocks[1..].iter().all(|block| {
        block.first()
            .and_then(|l| l.split_whitespace().next())
            .and_then(|t| t.parse::<usize>().ok())
            .is_some()
    });

    let mut blocks = outer_blocks;

    if branches_are_typed {
        // TypedRepeat: each subsequent pre-block is one branch
        // e.g. "1 x" → type_val="1", inner=[Scalars(["x"])]
        let branches: Vec<TypedBranch> = input_blocks[1..]
            .iter()
            .map(|block| {
                let first = block.first().map(|s| s.as_str()).unwrap_or("");
                let mut toks = first.split_whitespace();
                let type_val = toks.next().unwrap_or("1").to_string();
                // Remaining tokens on first line become the first inner line
                let rest: String = toks.collect::<Vec<_>>().join(" ");
                let mut inner_lines: Vec<String> = Vec::new();
                if !rest.is_empty() {
                    inner_lines.push(rest);
                }
                inner_lines.extend(block[1..].iter().cloned());
                let inner = parse_input_blocks(&inner_lines);
                TypedBranch { type_val, inner }
            })
            .collect();
        blocks.push(InputBlock::TypedRepeat { count, branches });
    } else {
        // OuterRepeat: remaining pre-blocks flattened as inner blocks
        let inner_lines: Vec<String> = input_blocks[1..].iter().flatten().cloned().collect();
        let inner = parse_input_blocks(&inner_lines);
        blocks.push(InputBlock::OuterRepeat { count, inner });
    }

    blocks
}

// ── Process execution ────────────────────────────────────────────────────────

#[derive(Debug)]
enum RunResult {
    Ok(String),
    RuntimeError(i32),
    TimeLimitExceeded,
}

fn run_with_input(
    artifact: &Path,
    input: &str,
    timelimit: Option<Duration>,
    cwd: &Path,
) -> anyhow::Result<RunResult> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use std::thread;

    let mut child = Command::new(artifact)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .current_dir(cwd)
        .spawn()
        .with_context(|| format!("failed to spawn {}", artifact.display()))?;

    let input_bytes = input.as_bytes().to_vec();
    let mut stdin = child.stdin.take().expect("stdin piped");
    thread::spawn(move || { let _ = stdin.write_all(&input_bytes); });

    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let stdout_handle = thread::spawn(move || -> Vec<u8> {
        use std::io::Read as _;
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });

    let status = if let Some(limit) = timelimit {
        let start = std::time::Instant::now();
        let mut final_status = None;
        loop {
            match child.try_wait() {
                Ok(Some(s)) => { final_status = Some(s); break; }
                Ok(None) => {
                    if start.elapsed() > limit {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => return Err(e.into()),
            }
        }
        final_status
    } else {
        Some(child.wait()?)
    };

    let stdout_bytes = stdout_handle.join().unwrap_or_default();

    Ok(match status {
        None => RunResult::TimeLimitExceeded,
        Some(s) if s.success() => RunResult::Ok(String::from_utf8_lossy(&stdout_bytes).into_owned()),
        Some(s) => RunResult::RuntimeError(s.code().unwrap_or(-1)),
    })
}

fn case_name(strategy: &CaseStrategy, corner_count: &mut u32, random_count: &mut u32) -> String {
    match strategy {
        CaseStrategy::Random => { *random_count += 1; format!("random{}", random_count) }
        _ => { *corner_count += 1; format!("corner{}", corner_count) }
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

pub(crate) struct RandomTestArgs<'a> {
    pub artifact: &'a Path,
    pub task_html_path: &'a Path,
    pub bin_letter: &'a str,
    pub count: u32,
    pub timelimit: Option<Duration>,
    pub display_limit: usize,
    pub cwd: &'a Path,
    pub shell: &'a mut Shell,
}

fn display_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim_end_matches('\n');
    if trimmed.len() <= limit {
        trimmed.to_string()
    } else {
        format!("{}...(truncated, {} bytes total)", &trimmed[..limit], trimmed.len())
    }
}

pub(crate) fn run_random_tests(args: RandomTestArgs<'_>) -> anyhow::Result<()> {
    let RandomTestArgs { artifact, task_html_path, bin_letter, count, timelimit, display_limit, cwd, shell } = args;

    let (parsed, blocks) = match load_task_spec(task_html_path, bin_letter, shell)? {
        Some(v) => v,
        None => return Ok(()),
    };

    writeln!(shell.err())?;
    writeln!(shell.err(), "══════════════════════════════════════════")?;
    writeln!(shell.err(), "               random tests")?;
    writeln!(shell.err(), "══════════════════════════════════════════")?;

    if !parsed.skipped.is_empty() {
        shell.err().set_color(color_spec!(Bold, Fg(Color::Yellow)))?;
        write!(shell.err(), "warning:")?;
        shell.err().reset()?;
        writeln!(shell.err(), " skipped {} unsupported constraint(s): {}", parsed.skipped.len(), parsed.skipped.join("; "))?;
    }

    let mut rng = rand::rngs::SmallRng::from_entropy();
    let strategies = make_strategy_list(&blocks, &parsed.sum_constraints, count);
    let total = strategies.len();
    let mut corner_count = 0u32;
    let mut random_count = 0u32;
    let mut failures = 0usize;
    let mut max_ms = 0u128;

    for (case_idx, strategy) in strategies.iter().enumerate() {
        let name = case_name(strategy, &mut corner_count, &mut random_count);
        let Some(input) = generate_random_input(&blocks, &parsed, &mut rng, strategy) else {
            shell.err().set_color(color_spec!(Bold, Fg(Color::Yellow)))?;
            write!(shell.err(), "{}/{} ({})", case_idx + 1, total, name)?;
            shell.err().reset()?;
            writeln!(shell.err(), " skipped (sum constraint unsatisfiable for this strategy)")?;
            writeln!(shell.err())?;
            continue;
        };

        let start = std::time::Instant::now();
        let result = run_with_input(artifact, &input, timelimit, cwd)?;
        let elapsed_ms = start.elapsed().as_millis();

        if elapsed_ms > max_ms {
            max_ms = elapsed_ms;
        }

        let (ok, verdict_label) = match &result {
            RunResult::Ok(_) => (true, "Accepted".to_string()),
            RunResult::RuntimeError(code) => (false, format!("Runtime Error (exit status: {})", code)),
            RunResult::TimeLimitExceeded => (false, "Time Limit Exceeded".to_string()),
        };
        if !ok { failures += 1; }

        let color = if ok { Color::Green } else { Color::Red };
        shell.err().set_color(color_spec!(Bold, Fg(color)))?;
        write!(shell.err(), "{}/{} ({:?})", case_idx + 1, total, name)?;
        shell.err().reset()?;
        writeln!(shell.err(), " {} ({} ms)", verdict_label, elapsed_ms)?;
        writeln!(shell.err(), "stdin:")?;
        writeln!(shell.err(), "{}", display_text(&input, display_limit))?;
        match &result {
            RunResult::Ok(out) => {
                writeln!(shell.err(), "actual:")?;
                writeln!(shell.err(), "{}", display_text(out, display_limit))?;
            }
            RunResult::RuntimeError(_) | RunResult::TimeLimitExceeded => {
                writeln!(shell.err(), "actual:")?;
                writeln!(shell.err(), "EMPTY")?;
            }
        }
        writeln!(shell.err())?;
    }

    writeln!(shell.err(), "max: {} ms", max_ms)?;
    shell.err().set_color(color_spec!(Bold, Fg(Color::Cyan)))?;
    write!(shell.err(), "note:")?;
    shell.err().reset()?;
    writeln!(shell.err(), " Accepted means the program exited without runtime error or TLE (output is not verified)")?;
    if failures > 0 {
        anyhow::bail!("{}/{} tests failed", failures, total);
    }
    Ok(())
}

pub(crate) struct CrossCheckArgs<'a> {
    pub artifact_a: &'a Path,
    pub artifact_b: &'a Path,
    pub alias_a: &'a str,
    pub alias_b: &'a str,
    pub task_html_path: &'a Path,
    pub bin_letter: &'a str,
    pub count: u32,
    pub r#match: Match,
    pub timelimit: Option<Duration>,
    pub display_limit: usize,
    pub cwd: &'a Path,
    pub shell: &'a mut Shell,
}

pub(crate) fn run_cross_check(args: CrossCheckArgs<'_>) -> anyhow::Result<()> {
    let CrossCheckArgs { artifact_a, artifact_b, alias_a, alias_b, task_html_path, bin_letter, count, r#match, timelimit, display_limit, cwd, shell } = args;

    let (parsed, blocks) = match load_task_spec(task_html_path, bin_letter, shell)? {
        Some(v) => v,
        None => return Ok(()),
    };
    if !parsed.skipped.is_empty() {
        shell.warn(format!(
            "skipped {} unsupported constraint(s): {}",
            parsed.skipped.len(),
            parsed.skipped.join("; ")
        ))?;
    }

    let mut rng = rand::rngs::SmallRng::from_entropy();
    let mut cases: Vec<PartialBatchTestCase> = Vec::new();
    let mut skipped = 0u32;
    let strategies = make_strategy_list(&blocks, &parsed.sum_constraints, count);
    let mut corner_count = 0u32;
    let mut random_count = 0u32;

    for strategy in &strategies {
        let name = case_name(strategy, &mut corner_count, &mut random_count);
        let Some(input) = generate_random_input(&blocks, &parsed, &mut rng, strategy) else {
            shell.warn(format!("cross-check: skipping {} (sum constraint unsatisfiable for this strategy)", name))?;
            skipped += 1;
            continue;
        };
        match run_with_input(artifact_b, &input, timelimit, cwd)? {
            RunResult::Ok(out_b) => {
                cases.push(PartialBatchTestCase {
                    name: Some(name),
                    r#in: Arc::from(input.as_str()),
                    out: Some(Arc::from(out_b.as_str())),
                    timelimit,
                    r#match: None,
                });
            }
            RunResult::RuntimeError(code) => {
                shell.warn(format!("cross-check: brute-force RE on {} (exit {}); skipping", name, code))?;
                skipped += 1;
            }
            RunResult::TimeLimitExceeded => {
                shell.warn(format!("cross-check: brute-force TLE on {}; skipping", name))?;
                skipped += 1;
            }
        }
    }

    if cases.is_empty() {
        anyhow::bail!("no valid cross-check cases (brute-force binary failed on all {} cases)", count);
    }
    if skipped > 0 {
        shell.warn(format!("{} case(s) skipped due to brute-force failure", skipped))?;
    }

    let suite = BatchTestSuite { timelimit, r#match, cases, extend: vec![] };
    let test_cases = suite.load_test_cases(cwd, None::<std::collections::HashSet<String>>, |_| Ok(vec![]))?;

    let outcome = snowchains_core::judge::judge(
        indicatif::ProgressDrawTarget::hidden(),
        tokio::signal::ctrl_c,
        &CommandExpression {
            program: artifact_a.into(),
            args: vec![],
            cwd: cwd.into(),
            env: btreemap!(),
        },
        &test_cases,
    )?;

    writeln!(shell.err())?;
    // Accepted: summary line only. Non-Accepted: summary + stdin + actual.
    {
        let total = outcome.verdicts.len();
        for (i, verdict) in outcome.verdicts.iter().enumerate() {
            if i > 0 { writeln!(shell.err())?; }
            match verdict {
                Verdict::Accepted { test_case_name, elapsed, .. } => {
                    write!(shell.err(), "{}/{} ({}) ", i + 1, total, test_case_name.as_deref().unwrap_or(""))?;
                    shell.err().set_color(color_spec!(Bold, Fg(Color::Green)))?;
                    writeln!(shell.err(), "Accepted ({} ms)", elapsed.as_millis())?;
                    shell.err().reset()?;
                }
                Verdict::WrongAnswer { test_case_name, elapsed, stdin, stdout, .. } => {
                    write!(shell.err(), "{}/{} ({}) ", i + 1, total, test_case_name.as_deref().unwrap_or(""))?;
                    shell.err().set_color(color_spec!(Bold, Fg(Color::Yellow)))?;
                    writeln!(shell.err(), "Wrong Answer ({} ms)", elapsed.as_millis())?;
                    shell.err().reset()?;
                    writeln!(shell.err(), "stdin:")?;
                    writeln!(shell.err(), "{}", display_text(stdin, display_limit))?;
                    writeln!(shell.err(), "actual:")?;
                    writeln!(shell.err(), "{}", display_text(stdout, display_limit))?;
                }
                Verdict::RuntimeError { test_case_name, elapsed, stdin, status, .. } => {
                    write!(shell.err(), "{}/{} ({}) ", i + 1, total, test_case_name.as_deref().unwrap_or(""))?;
                    shell.err().set_color(color_spec!(Bold, Fg(Color::Yellow)))?;
                    writeln!(shell.err(), "Runtime Error ({} ms, {})", elapsed.as_millis(), status)?;
                    shell.err().reset()?;
                    writeln!(shell.err(), "stdin:")?;
                    writeln!(shell.err(), "{}", display_text(stdin, display_limit))?;
                }
                Verdict::TimelimitExceeded { test_case_name, timelimit, stdin, .. } => {
                    write!(shell.err(), "{}/{} ({}) ", i + 1, total, test_case_name.as_deref().unwrap_or(""))?;
                    shell.err().set_color(color_spec!(Bold, Fg(Color::Red)))?;
                    writeln!(shell.err(), "Timelimit Exceeded ({} ms)", timelimit.as_millis())?;
                    shell.err().reset()?;
                    writeln!(shell.err(), "stdin:")?;
                    writeln!(shell.err(), "{}", display_text(stdin, display_limit))?;
                }
            }
        }
        shell.err().flush()?;
    }

    let max_ms = outcome.verdicts.iter().map(|v| match v {
        Verdict::Accepted { elapsed, .. }
        | Verdict::WrongAnswer { elapsed, .. }
        | Verdict::RuntimeError { elapsed, .. } => elapsed.as_millis(),
        Verdict::TimelimitExceeded { timelimit, .. } => timelimit.as_millis(),
    }).max().unwrap_or(0);

    let failed = outcome.verdicts.iter().filter(|v| !matches!(v, Verdict::Accepted { .. })).count();

    if failed > 0 {
        writeln!(shell.err())?;
        shell.err().set_color(color_spec!(Bold, Fg(Color::Magenta)))?;
        write!(shell.err(), "expected:")?;
        shell.err().reset()?;
        writeln!(shell.err(), " {}", alias_b)?;
        shell.err().set_color(color_spec!(Bold, Fg(Color::Magenta)))?;
        write!(shell.err(), "actual:")?;
        shell.err().reset()?;
        writeln!(shell.err(), " {}", alias_a)?;
        writeln!(shell.err())?;
        anyhow::bail!("{}/{} tests failed (max: {} ms)", failed, outcome.verdicts.len(), max_ms);
    }
    Ok(())
}

// ── Cargo.toml cross-bin registration ───────────────────────────────────────

/// Register a cross-check binary in Cargo.toml if not already present.
/// Returns the registered bin name.
pub(crate) fn ensure_cross_bin_registered(
    manifest_path: &Path,
    cross_src: &Path,
    contest: &str,
    problem_url: &str,
    shell: &mut Shell,
) -> anyhow::Result<String> {
    let stem = cross_src
        .file_stem()
        .and_then(|s| s.to_str())
        .with_context(|| format!("invalid cross file: {}", cross_src.display()))?;
    let stem_kebab = stem.to_kebab_case();
    let bin_name = format!("{}-{}", contest, stem_kebab);
    let alias = stem_kebab.clone();

    let content = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let mut doc: toml_edit::Document = content.parse()
        .with_context(|| "failed to parse Cargo.toml")?;

    let already = doc["package"]["metadata"]["cargo-compete"]["bin"]
        .as_table()
        .map(|t| t.contains_key(&bin_name))
        .unwrap_or(false);

    if already {
        return Ok(bin_name);
    }

    shell.status("Registering", format!("cross-check binary `{}`", bin_name))?;

    let bin_item = doc.entry("bin").or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));
    if let toml_edit::Item::ArrayOfTables(arr) = bin_item {
        let mut tbl = toml_edit::Table::new();
        tbl["name"] = toml_edit::value(bin_name.clone());
        let manifest_dir = manifest_path.parent().unwrap_or(Path::new("."));
        let rel = cross_src
            .strip_prefix(manifest_dir)
            .unwrap_or(cross_src)
            .to_string_lossy()
            .replace('\\', "/");
        tbl["path"] = toml_edit::value(rel);
        arr.push(tbl);
    }

    let meta = &mut doc["package"]["metadata"]["cargo-compete"]["bin"];
    meta[&bin_name]["alias"] = toml_edit::value(alias);
    meta[&bin_name]["problem"] = toml_edit::value(problem_url);

    std::fs::write(manifest_path, doc.to_string())
        .with_context(|| "failed to write Cargo.toml")?;

    Ok(bin_name)
}
