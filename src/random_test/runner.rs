use super::{
    generate::{generate_random_input, make_strategy_list, CaseStrategy, RandomStrategy},
    task_spec::load_task_spec,
};
use crate::shell::Shell;
use anyhow::Context as _;
use maplit::btreemap;
use rand::SeedableRng;
use snowchains_core::judge::{CommandExpression, Verdict};
use std::{
    io::Write as _,
    path::Path,
    sync::Arc,
    time::Duration,
};
use termcolor::Color;

// ── Process execution ────────────────────────────────────────────────────────

#[derive(Debug)]
pub(super) enum RunResult {
    Ok(String),
    RuntimeError(i32),
    TimeLimitExceeded,
}

pub(super) fn run_with_input(
    artifact: &Path,
    input: &str,
    timelimit: Option<Duration>,
    cwd: &Path,
) -> anyhow::Result<RunResult> {
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
    // Broken-pipe errors on early process exit are expected and intentionally ignored.
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
        Some(status) if status.success() => RunResult::Ok(String::from_utf8_lossy(&stdout_bytes).into_owned()),
        Some(status) => RunResult::RuntimeError(status.code().unwrap_or(-1)),
    })
}

pub(super) fn case_name(strategy: &CaseStrategy, corner_count: &mut u32, random_count: &mut u32) -> String {
    match strategy {
        CaseStrategy::Random(RandomStrategy::Random) => { *random_count += 1; format!("random{}", random_count) }
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
    pub cwd: &'a Path,
    pub shell: &'a mut Shell,
}

pub(crate) fn run_random_tests(args: RandomTestArgs<'_>) -> anyhow::Result<()> {
    let RandomTestArgs { artifact, task_html_path, bin_letter, count, timelimit, cwd, shell } = args;

    let (parsed, blocks) = match load_task_spec(task_html_path, bin_letter, shell)? {
        Some(v) => v,
        None => return Ok(()),
    };

    let mut rng = rand::rngs::SmallRng::from_entropy();
    let strategies = make_strategy_list(&blocks, &parsed.sum_constraints, count);
    let mut corner_count = 0u32;
    let mut random_count = 0u32;

    let mut test_cases: Vec<snowchains_core::testsuite::BatchTestCase> = Vec::new();
    for strategy in &strategies {
        let name = case_name(strategy, &mut corner_count, &mut random_count);
        match generate_random_input(&blocks, &parsed, &mut rng, strategy) {
            Some(input) => test_cases.push(snowchains_core::testsuite::BatchTestCase {
                name: Some(name),
                timelimit,
                input: Arc::from(input.as_str()),
                output: snowchains_core::testsuite::ExpectedOutput::Deterministic(
                    snowchains_core::testsuite::DeterministicExpectedOutput::Pass,
                ),
            }),
            None => shell.warn(format!("skipping {} (constraints unsatisfiable)", name))?,
        }
    }

    if test_cases.is_empty() {
        shell.warn("no test cases generated")?;
        return Ok(());
    }

    let outcome = snowchains_core::judge::judge(
        shell.progress_draw_target(),
        tokio::signal::ctrl_c,
        &CommandExpression {
            program: artifact.into(),
            args: vec![],
            cwd: cwd.into(),
            env: btreemap!(),
        },
        &test_cases,
    )?;

    super::write_section_banner(shell.err(), "random tests")?;
    outcome.print_pretty(shell.err(), Some(200))?;
    writeln!(shell.err())?;

    let mut failures = 0usize;
    for verdict in &outcome.verdicts {
        if !matches!(verdict, Verdict::Accepted { .. }) { failures += 1; }
    }

    let has_accepted = failures < outcome.verdicts.len();
    if has_accepted {
        shell.err_label(Color::Cyan, "note", "Accepted means no crash or TLE; output correctness is not verified")?;
    }
    if !parsed.skipped.is_empty() {
        shell.err_label(Color::Yellow, "warning", &format!("skipped {} unsupported constraint(s): {}", parsed.skipped.len(), parsed.skipped.join("; ")))?;
    }
    if failures > 0 {
        anyhow::bail!("{}/{} tests failed", failures, test_cases.len());
    }
    Ok(())
}
