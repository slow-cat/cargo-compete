use crate::shell::Shell;
use camino::Utf8Path;
use std::path::Path;

pub(crate) fn save_atcoder_tasks_print_if_missing(
    contest: &str,
    dest_dir: &Utf8Path,
    cookies_path: &Path,
    shell: &mut Shell,
) -> anyhow::Result<()> {
    let dest_path = dest_dir.join("task.html");
    if dest_path.exists() {
        return Ok(());
    }

    crate::fs::create_dir_all(dest_dir)?;

    let url = format!("https://atcoder.jp/contests/{contest}/tasks_print");
    let cookie_header = atcoder_cookie_header_best_effort(cookies_path);

    let result: anyhow::Result<()> = (|| {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()?;

        shell.status("Downloading", format!("`{}`", url))?;
        let req = client.get(&url);
        let req = match &cookie_header {
            Some(c) => req.header(reqwest::header::COOKIE, c.as_str()),
            None => req,
        };
        let resp = req.send()?;
        let resp = resp.error_for_status()?;
        let body = resp.bytes()?;

        crate::fs::write(&dest_path, body)?;
        shell.status("Wrote", dest_path.as_str())?;
        Ok(())
    })();

    if let Err(err) = result {
        shell.warn(format!(
            "Failed to save `{}` from `{}` ({err}).",
            dest_path, url
        ))?;
    }
    Ok(())
}

fn atcoder_cookie_header_best_effort(cookies_path: &Path) -> Option<String> {
    let content = crate::fs::read_to_string(cookies_path).ok()?;
    let mut pairs = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        if !cookie_domain_looks_like_atcoder(&v) {
            continue;
        }

        let Some(raw_cookie) = v.get("raw_cookie").and_then(|v| v.as_str()) else {
            continue;
        };
        let pair = raw_cookie.split(';').next().unwrap_or("").trim();
        if pair.contains('=') {
            pairs.push(pair.to_string());
        }
    }

    if pairs.is_empty() {
        None
    } else {
        Some(pairs.join("; "))
    }
}

fn cookie_domain_looks_like_atcoder(v: &serde_json::Value) -> bool {
    let Some(domain) = v.get("domain") else {
        return true;
    };

    let domain = domain
        .get("HostOnly")
        .and_then(|v| v.as_str())
        .or_else(|| domain.get("Domain").and_then(|v| v.as_str()));

    domain
        .map(|d| d == "atcoder.jp" || d.ends_with(".atcoder.jp"))
        .unwrap_or(true)
}
