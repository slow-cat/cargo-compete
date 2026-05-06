use super::{
    generate::{generate_random_input, make_strategy_list},
    runner::{case_name, run_with_input, RunResult},
    task_spec::load_task_spec,
};
use crate::shell::Shell;
use maplit::btreemap;
use rand::SeedableRng;
use snowchains_core::{
    judge::{CommandExpression, Verdict},
    testsuite::{BatchTestSuite, Match, PartialBatchTestCase},
};
use std::{
    path::Path,
    sync::Arc,
    time::Duration,
};
use termcolor::Color;

pub(crate) struct CrossCheckArgs<'a> {
    pub artifact_main: &'a Path,
    pub artifact_cross: &'a Path,
    pub alias_main: &'a str,
    pub alias_cross: &'a str,
    pub task_html_path: &'a Path,
    pub bin_letter: &'a str,
    pub count: u32,
    pub r#match: Match,
    pub timelimit: Option<Duration>,
    pub cwd: &'a Path,
    pub shell: &'a mut Shell,
}

pub(crate) fn run_cross_check(args: CrossCheckArgs<'_>) -> anyhow::Result<()> {
    let CrossCheckArgs { artifact_main, artifact_cross, alias_main, alias_cross, task_html_path, bin_letter, count, r#match, timelimit, cwd, shell } = args;

    let (parsed, blocks) = match load_task_spec(task_html_path, bin_letter, shell)? {
        Some(v) => v,
        None => return Ok(()),
    };

    let mut rng = rand::rngs::SmallRng::from_entropy();
    let strategies = make_strategy_list(&blocks, count);
    let mut corner_count = 0u32;
    let mut random_count = 0u32;

    // Phase 1: run cross binary, collect successful cases
    let mut partial_cases: Vec<PartialBatchTestCase> = Vec::new();
    let mut brute_skipped = 0u32;

    for strategy in &strategies {
        let name = case_name(strategy, &mut corner_count, &mut random_count);
        let Some(input) = generate_random_input(&blocks, &parsed, &mut rng, strategy) else {
            shell.warn(format!("cross-check: skipping {} (constraints unsatisfiable for this strategy)", name))?;
            brute_skipped += 1;
            continue;
        };
        match run_with_input(artifact_cross, &input, timelimit, cwd)? {
            RunResult::Ok(out_b) => {
                partial_cases.push(PartialBatchTestCase {
                    name: Some(name),
                    r#in: Arc::from(input.as_str()),
                    out: Some(Arc::from(out_b.as_str())),
                    timelimit,
                    r#match: None,
                });
            }
            RunResult::RuntimeError(code) => {
                shell.warn(format!("cross-check: brute-force RE on {} (exit {}); skipping", name, code))?;
                brute_skipped += 1;
            }
            RunResult::TimeLimitExceeded => {
                shell.warn(format!("cross-check: brute-force TLE on {}; skipping", name))?;
                brute_skipped += 1;
            }
        }
    }

    if partial_cases.is_empty() {
        anyhow::bail!("no valid cross-check cases (brute-force binary failed on all {} cases)", count);
    }

    let total = partial_cases.len();

    super::write_section_banner(shell.err(), "cross-check tests")?;

    // Phase 2: judge main binary against all cases at once (progress bar visible)
    let suite = BatchTestSuite { timelimit, r#match: r#match.clone(), cases: partial_cases, extend: vec![] };
    let test_cases = suite.load_test_cases(cwd, None::<std::collections::HashSet<String>>, |_| Ok(vec![]))?;

    let mut outcome = snowchains_core::judge::judge(
        shell.progress_draw_target(),
        tokio::signal::ctrl_c,
        &CommandExpression {
            program: artifact_main.into(),
            args: vec![],
            cwd: cwd.into(),
            env: btreemap!(),
        },
        &test_cases,
    )?;

    let failures = outcome.verdicts.iter().filter(|v| !matches!(v, Verdict::Accepted { .. })).count();

    if failures > 0 {
        outcome.verdicts.retain(|v| !matches!(v, Verdict::Accepted { .. }));
        writeln!(shell.err())?;
        outcome.print_pretty(shell.err(), Some(4096))?;
    }

    writeln!(shell.err())?;
    if failures > 0 {
        shell.err_label(Color::Magenta, "expected", alias_cross)?;
        shell.err_label(Color::Magenta, "actual", alias_main)?;
        writeln!(shell.err())?;
    }
    if !parsed.skipped.is_empty() {
        shell.err_label(Color::Yellow, "warning", format!("skipped {} unsupported constraint(s): {}", parsed.skipped.len(), parsed.skipped.join("; ")))?;
    }
    if brute_skipped > 0 {
        shell.err_label(Color::Yellow, "warning", format!("{} case(s) skipped due to brute-force failure", brute_skipped))?;
    }
    if failures > 0 {
        anyhow::bail!("{}/{} tests failed", failures, total);
    }
    Ok(())
}
