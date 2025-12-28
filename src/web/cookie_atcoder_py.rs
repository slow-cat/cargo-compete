use crate::shell::Shell;
use std::{env, path::Path, process::Command};

pub(crate) fn update_atcoder_cookie_best_effort(cookies_path: &Path, shell: &mut Shell) {
    let python = env::var("ACCC_PYTHON").unwrap_or_else(|_| "python3".into());
    let browser = env::var("ACCC_BROWSER").unwrap_or_else(|_| "firefox".into());

    let py = r#"
import json, pathlib, sys
from datetime import datetime, timezone
from yt_dlp.cookies import extract_cookies_from_browser, YDLLogger

browser = sys.argv[1]
out_path = pathlib.Path(sys.argv[2])

jar = extract_cookies_from_browser(browser, logger=YDLLogger())
for c in jar:
    if c.domain == "atcoder.jp" and c.name == "REVEL_SESSION":
        line = {
            "raw_cookie": f"{c.name}={c.value}; HttpOnly; Secure",
            "path": [c.path, bool(c.path_specified)],
            "domain": {"HostOnly": c.domain},
        }
        if c.expires is not None:
            line["expires"] = {
                "AtUtc": datetime.fromtimestamp(c.expires, tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
            }
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(line) + "\n", encoding="utf-8")
        sys.exit(0)
sys.exit(2)
"#;

    let status = Command::new(python)
        .args(["-c", py, &browser, cookies_path.to_string_lossy().as_ref()])
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            let _ = shell.warn(format!("cookie update skipped: accc python exited {}", s));
        }
        Err(e) => {
            let _ = shell.warn(format!("cookie update skipped: failed to run python: {e}"));
        }
    }
}
