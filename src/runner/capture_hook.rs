// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::default_shell;

pub(crate) struct CaptureHook {
    shell_program: String,
    shell_args: Vec<String>,
    command: String,
    hook: String,
}

#[derive(Serialize)]
struct CaptureHookPayload {
    command: String,
    changed: bool,
    output: String,
    unix_timestamp: u64,
}

impl CaptureHook {
    pub(crate) fn new(shell: &str, command: String, hook: String) -> Self {
        let (shell_program, shell_args) = parse_shell(shell);
        Self {
            shell_program,
            shell_args,
            command,
            hook,
        }
    }

    pub(crate) fn maybe_run(&self, output: &str, changed: bool) -> Result<()> {
        if !changed {
            return Ok(());
        }

        let payload = CaptureHookPayload {
            command: self.command.clone(),
            changed,
            output: output.to_string(),
            unix_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs(),
        };

        let mut cmd = Command::new(&self.shell_program);
        cmd.args(&self.shell_args)
            .arg(&self.hook)
            .env(
                "TWATCH_DATA",
                serde_json::to_string(&payload)
                    .context("failed to serialize aftercommand payload")?,
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = cmd.status();
        Ok(())
    }
}

pub(crate) fn parse_shell(shell: &str) -> (String, Vec<String>) {
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
