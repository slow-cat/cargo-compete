use crate::{project::PackageExt as _, shell::Shell};
use heck::KebabCase as _;
use anyhow::ensure;
use az::SaturatingAs as _;
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata as cm;
use human_size::{Byte, Size};
use liquid::object;
use maplit::btreemap;
use snowchains_core::{
    judge::CommandExpression,
    testsuite::{PartialBatchTestCase, TestSuite},
    web::PlatformKind,
};
use std::{
    collections::{BTreeMap, HashSet},
    env,
    path::{Path, PathBuf},
    sync::Arc,
};
use url::Url;

pub(crate) struct Args<'a> {
    pub(crate) metadata: &'a cm::Metadata,
    pub(crate) member: &'a cm::Package,
    pub(crate) bin: &'a cm::Target,
    pub(crate) bin_alias: &'a str,
    pub(crate) cargo_compete_config_test_suite: &'a liquid::Template,
    pub(crate) problem_url: &'a Url,
    pub(crate) toolchain: Option<&'a str>,
    pub(crate) release: bool,
    pub(crate) test_case_names: Option<HashSet<String>>,
    pub(crate) display_limit: Size,
    pub(crate) cookies_path: &'a Path,
    pub(crate) shell: &'a mut Shell,
    /// Skip sample tests and go directly to random/cross-check
    pub(crate) no_test: bool,
    /// Number of random test cases to run after samples pass (None = skip)
    pub(crate) random_count: Option<u32>,
    /// Path to a cross-check source file (enables cross-check mode)
    pub(crate) cross_src: Option<PathBuf>,
    /// Number of cross-check cases (overrides random_count for cross mode)
    pub(crate) cross_count: Option<u32>,
}

pub(crate) fn test(args: Args<'_>) -> anyhow::Result<()> {
    let Args {
        metadata,
        member,
        bin,
        bin_alias,
        cargo_compete_config_test_suite,
        problem_url,
        toolchain,
        release,
        test_case_names,
        display_limit,
        cookies_path,
        shell,
        no_test,
        random_count,
        cross_src,
        cross_count,
    } = args;

    let test_suite_path = test_suite_path(
        &metadata.workspace_root,
        member.manifest_dir(),
        cargo_compete_config_test_suite,
        &bin.name,
        bin_alias,
        problem_url,
        shell,
    )?;

    let test_suite = crate::fs::read_yaml(&test_suite_path)?;

    let test_cases = match test_suite {
        TestSuite::Batch(test_suite) => test_suite.load_test_cases(
            test_suite_path.parent().unwrap().as_ref(),
            test_case_names,
            |override_problem_url| {
                fn read(path: &Path) -> anyhow::Result<Arc<str>> {
                    crate::fs::read_to_string(path).map(Into::into)
                }

                let problem_url = override_problem_url.unwrap_or(problem_url);

                let system_test_cases_dir =
                    crate::web::retrieve_testcases::system_test_cases_dir(problem_url)?;

                let text_files = |dir_name: &str| -> anyhow::Result<Vec<_>> {
                    let paths = crate::fs::read_dir(system_test_cases_dir.join(dir_name))?;
                    Ok(paths
                        .into_iter()
                        .filter(|p| p.extension() == Some("txt".as_ref()))
                        .map(|p| {
                            let s = p
                                .file_stem()
                                .expect("should not be empty")
                                .to_string_lossy()
                                .into_owned();
                            (s, p)
                        })
                        .collect())
                };

                if !system_test_cases_dir.join("in").exists() {
                    crate::web::retrieve_testcases::dl_only_system_test_cases(
                        problem_url,
                        cookies_path,
                        &metadata.workspace_root,
                        shell,
                    )?;
                }

                let mut system_test_cases: BTreeMap<_, (Option<_>, Option<_>)> = btreemap!();

                for (name, path) in text_files("in")? {
                    system_test_cases.entry(name).or_default().0 = Some(read(&path)?);
                }
                for (name, path) in text_files("out")? {
                    system_test_cases.entry(name).or_default().1 = Some(read(&path)?);
                }

                Ok(system_test_cases
                    .into_iter()
                    .flat_map(|(name, (r#in, out))| {
                        let r#in = r#in?;
                        Some(PartialBatchTestCase {
                            name: Some(name),
                            r#in,
                            out,
                            timelimit: None,
                            r#match: None,
                        })
                    })
                    .collect())
            },
        )?,
        TestSuite::Interactive(_) => {
            shell.warn("tests for `Interactive` problems are currently not supported")?;
            vec![]
        }
        TestSuite::Unsubmittable => {
            shell.warn("this is `Unsubmittable` problem")?;
            vec![]
        }
    };

    if let Some(toolchain) = toolchain {
        crate::process::process("rustup").args(&["run", toolchain, "cargo"])
    } else {
        crate::process::process(crate::process::cargo_exe()?)
    }
    .arg("build")
    .arg(if bin.kind == ["example".to_owned()] {
        "--example"
    } else {
        "--bin"
    })
    .arg(&bin.name)
    .args(if release { &["--release"] } else { &[] })
    .arg("--manifest-path")
    .arg(&member.manifest_path)
    .cwd(&metadata.workspace_root)
    .exec_with_shell_status(shell)?;

    let artifact = metadata
        .target_directory
        .join(if release { "release" } else { "debug" })
        .join(if bin.kind == ["example".to_owned()] {
            "examples"
        } else {
            ""
        })
        .join(&bin.name)
        .with_extension(env::consts::EXE_EXTENSION);

    ensure!(
        artifact.exists(),
        "`cargo build` succeeded but `{}` was not produced. probably this is a bug",
        artifact,
    );

    let display_limit_bytes: usize = display_limit.into::<Byte>().value().saturating_as();

    let sample_result = if !no_test {
        let outcome = snowchains_core::judge::judge(
            shell.progress_draw_target(),
            tokio::signal::ctrl_c,
            &CommandExpression {
                program: artifact.clone().into(),
                args: vec![],
                cwd: metadata.workspace_root.clone().into(),
                env: btreemap!(),
            },
            &test_cases,
        )?;
        writeln!(shell.err())?;
        outcome.print_pretty(shell.err(), Some(display_limit_bytes))?;
        outcome.error_on_fail()
    } else {
        Ok(())
    };

    // Run random / cross-check tests after samples (only if samples pass)
    let need_random = random_count.is_some() || cross_src.is_some();
    if sample_result.is_ok() && need_random {
        let effective_count = if cross_src.is_some() {
            cross_count.or(random_count).unwrap_or(100)
        } else {
            random_count.unwrap_or(5)
        };

        // Determine task.html path (AtCoder only)
        let is_atcoder = matches!(PlatformKind::from_url(problem_url), Ok(PlatformKind::Atcoder));

        if is_atcoder {
            let contest_id = snowchains_core::web::atcoder_contest_id(problem_url)?;
            let task_html_path = member.manifest_dir().join("task.html");
            let bin_letter = bin_alias.to_uppercase();

            // Extract timelimit and match type from the loaded test suite yaml
            let test_suite_for_meta: snowchains_core::testsuite::TestSuite =
                crate::fs::read_yaml(&test_suite_path)?;
            let (timelimit, match_type) = match test_suite_for_meta {
                snowchains_core::testsuite::TestSuite::Batch(s) => (s.timelimit, s.r#match),
                _ => (None, snowchains_core::testsuite::Match::Exact),
            };

            if let Some(cross_src_path) = cross_src {
                // ── Cross-check mode ──────────────────────────────────────
                let cross_src_abs = if cross_src_path.is_absolute() {
                    cross_src_path.clone()
                } else {
                    member.manifest_dir().as_std_path().join(&cross_src_path)
                };

                // Register cross binary in Cargo.toml if needed
                let cross_bin_name = crate::random_test::ensure_cross_bin_registered(
                    member.manifest_path.as_std_path(),
                    &cross_src_abs,
                    &contest_id,
                    problem_url.as_str(),
                    shell,
                )?;

                // Build cross binary
                if !no_test {
                    crate::random_test::write_section_banner(shell.err(), "cross-check binary sample tests")?;
                }
                if let Some(tc) = toolchain {
                    crate::process::process("rustup").args(&["run", tc, "cargo"])
                } else {
                    crate::process::process(crate::process::cargo_exe()?)
                }
                .arg("build")
                .arg("--bin")
                .arg(&cross_bin_name)
                .args(if release { &["--release"] } else { &[] })
                .arg("--manifest-path")
                .arg(&member.manifest_path)
                .cwd(&metadata.workspace_root)
                .exec_with_shell_status(shell)?;

                let artifact_cross = metadata
                    .target_directory
                    .join(if release { "release" } else { "debug" })
                    .join(&cross_bin_name)
                    .with_extension(env::consts::EXE_EXTENSION);

                ensure!(artifact_cross.exists(), "built cross binary `{}` not found", artifact_cross);

                let cross_alias = cross_src_abs
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_kebab_case())
                    .unwrap_or_else(|| cross_bin_name.clone());

                let run_cross = if !no_test {
                    // Run sample tests for cross binary without timelimit (brute-force may be slow)
                    let cross_test_cases: Vec<_> = test_cases.iter().cloned()
                        .map(|mut c| { c.timelimit = None; c })
                        .collect();
                    let cross_sample_outcome = snowchains_core::judge::judge(
                        shell.progress_draw_target(),
                        tokio::signal::ctrl_c,
                        &CommandExpression {
                            program: artifact_cross.clone().into(),
                            args: vec![],
                            cwd: metadata.workspace_root.clone().into(),
                            env: btreemap!(),
                        },
                        &cross_test_cases,
                    )?;
                    writeln!(shell.err())?;
                    cross_sample_outcome.print_pretty(shell.err(), Some(display_limit_bytes))?;
                    let cross_sample_result = cross_sample_outcome.error_on_fail();
                    if cross_sample_result.is_err() {
                        cross_sample_result?;
                    }
                    true
                } else {
                    true
                };

                if run_cross {
                    crate::random_test::run_cross_check(crate::random_test::CrossCheckArgs {
                        artifact_main: artifact.as_std_path(),
                        artifact_cross: artifact_cross.as_std_path(),
                        alias_main: bin_alias,
                        alias_cross: &cross_alias,
                        task_html_path: task_html_path.as_std_path(),
                        bin_letter: &bin_letter,
                        count: effective_count,
                        r#match: match_type,
                        timelimit,
                        cwd: metadata.workspace_root.as_std_path(),
                        shell,
                    })?;
                }
            } else {
                // ── Random-only mode ──────────────────────────────────────
                crate::random_test::run_random_tests(crate::random_test::RandomTestArgs {
                    artifact: artifact.as_std_path(),
                    task_html_path: task_html_path.as_std_path(),
                    bin_letter: &bin_letter,
                    count: effective_count,
                    timelimit,
                    cwd: metadata.workspace_root.as_std_path(),
                    shell,
                })?;
            }
        } else {
            shell.warn("random tests are only supported for AtCoder problems")?;
        }
    }

    sample_result
}

pub(crate) fn test_suite_path(
    workspace_root: &Utf8Path,
    pkg_manifest_dir: &Utf8Path,
    cargo_compete_config_test_suite: &liquid::Template,
    bin_name: &str,
    bin_alias: &str,
    problem_url: &Url,
    shell: &mut Shell,
) -> anyhow::Result<Utf8PathBuf> {
    let contest = match PlatformKind::from_url(problem_url) {
        Ok(PlatformKind::Atcoder) => Some(snowchains_core::web::atcoder_contest_id(problem_url)?),
        Ok(PlatformKind::Codeforces) => {
            Some(snowchains_core::web::codeforces_contest_id(problem_url)?.to_string())
        }
        _ => None,
    };

    let vars = object!({
        "manifest_dir": pkg_manifest_dir,
        "contest": contest,
        "bin_name": bin_name,
        "bin_alias": bin_alias,
    });

    let vars_including_deprecated = object!({
        "manifest_dir": pkg_manifest_dir,
        "contest": contest,
        "bin_name": bin_name,
        "bin_alias": bin_alias,
        "problem": bin_alias,
    });

    let (test_suite_path, uses_deprecated_vars) = cargo_compete_config_test_suite
        .render(&vars)
        .map(|r| (r, false))
        .or_else(|_| {
            cargo_compete_config_test_suite
                .render(&vars_including_deprecated)
                .map(|r| (r, true))
        })?;
    let test_suite_path = Utf8Path::new(&test_suite_path);
    let test_suite_path = test_suite_path.strip_prefix(".").unwrap_or(test_suite_path);

    if uses_deprecated_vars {
        shell.warn("deprecated variables used for `.test-suite` in compete.toml")?;
        shell.warn("- `problem` is deprecated. use `bin_alias` instead.")?;
    }

    Ok(workspace_root.join(test_suite_path))
}
