//! Child-process execution helpers shared by the random-test runner and the
//! cross-check runner. Data-model independent.

use anyhow::Context as _;
use std::{path::Path, time::Duration};

#[derive(Debug)]
pub(crate) enum RunResult {
    Ok(String),
    RuntimeError(i32),
    TimeLimitExceeded,
}

/// Run `artifact` with `input` on stdin, optionally bounded by `timelimit`.
///
/// `stderr` is discarded. A `None` timelimit means run to completion (used for
/// slow brute-force binaries in cross-check).
pub(crate) fn run_with_input(
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
    // Broken-pipe errors on early process exit are expected and ignored.
    thread::spawn(move || {
        let _ = stdin.write_all(&input_bytes);
    });

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
                Ok(Some(s)) => {
                    final_status = Some(s);
                    break;
                }
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
        Some(status) if status.success() => {
            RunResult::Ok(String::from_utf8_lossy(&stdout_bytes).into_owned())
        }
        Some(status) => RunResult::RuntimeError(status.code().unwrap_or(-1)),
    })
}
