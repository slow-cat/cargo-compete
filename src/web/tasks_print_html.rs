use crate::shell::Shell;
use camino::Utf8Path;

pub(crate) fn save_atcoder_tasks_print_if_missing(
    contest: &str,
    dest_dir: &Utf8Path,
    shell: &mut Shell,
) -> anyhow::Result<()> {
    let dest_path = dest_dir.join("task.html");
    if dest_path.exists() {
        return Ok(());
    }

    crate::fs::create_dir_all(dest_dir)?;

    let url = format!("https://atcoder.jp/contests/{contest}/tasks_print");

    let result: anyhow::Result<()> = (|| {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()?;

        shell.status("Downloading", format!("`{}`", url))?;
        let resp = client.get(&url).send()?;
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
