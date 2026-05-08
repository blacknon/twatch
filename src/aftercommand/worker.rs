use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::aftercommand::AfterCommandPayload;
use crate::cli::default_shell;

pub(super) fn run_hook_with_timeout(
    shell_program: &str,
    shell_args: &[String],
    hook: &str,
    timeout: Duration,
    payload: &AfterCommandPayload,
) -> Result<()> {
    let mut cmd = Command::new(shell_program);
    cmd.args(shell_args)
        .arg(hook)
        .env(
            "TWATCH_DATA",
            serde_json::to_string(payload).context("failed to serialize aftercommand payload")?,
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().context("failed to spawn aftercommand hook")?;
    let start = SystemTime::now();

    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if start.elapsed().unwrap_or(Duration::ZERO) >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn parse_shell(shell: &str) -> (String, Vec<String>) {
    match shell_words::split(shell) {
        Ok(parts) if !parts.is_empty() => {
            (parts[0].clone(), parts.iter().skip(1).cloned().collect())
        }
        _ => {
            let parts = shell_words::split(&default_shell()).expect("default shell must parse");
            (parts[0].clone(), parts.iter().skip(1).cloned().collect())
        }
    }
}
