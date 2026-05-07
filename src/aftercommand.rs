use std::process::{Command, Stdio};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use regex::Regex;
use serde::Serialize;

use crate::cli::default_shell;

#[derive(Clone, Debug)]
pub struct AfterCommandConfig {
    pub hook: String,
    pub shell: String,
    pub command_display: String,
    pub regex: Option<Regex>,
    pub changed_cells: Option<usize>,
    pub every: Option<usize>,
    pub debounce_ms: Option<u64>,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug)]
pub struct AfterCommandEvent {
    pub changed: bool,
    pub output: String,
    pub timestamp_unix_ms: u64,
    pub frame_seq: u64,
    pub width: u16,
    pub height: u16,
    pub changed_cell_count: usize,
    pub last_input_summary: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AfterCommandPayload {
    pub command: String,
    pub changed: bool,
    pub output: String,
    pub unix_timestamp: u64,
    pub frame_seq: u64,
    pub width: u16,
    pub height: u16,
    pub changed_cell_count: usize,
    pub matched_rules: Vec<String>,
    pub last_input_summary: String,
}

#[derive(Debug)]
pub struct AfterCommandRuntime {
    config: AfterCommandConfig,
    tx: SyncSender<AfterCommandPayload>,
    changed_frame_count: usize,
    last_trigger_unix_ms: Option<u64>,
}

impl AfterCommandRuntime {
    pub fn new(config: AfterCommandConfig) -> Self {
        let (shell_program, shell_args) = parse_shell(&config.shell);
        let hook = config.hook.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let timeout = Duration::from_millis(config.timeout_ms.max(1));

        thread::spawn(move || {
            while let Ok(payload) = rx.recv() {
                let _ =
                    run_hook_with_timeout(&shell_program, &shell_args, &hook, timeout, &payload);
            }
        });

        Self {
            config,
            tx,
            changed_frame_count: 0,
            last_trigger_unix_ms: None,
        }
    }

    pub fn evaluate_and_enqueue(&mut self, event: AfterCommandEvent) -> Result<Option<String>> {
        if !event.changed {
            return Ok(None);
        }

        self.changed_frame_count += 1;

        if self
            .config
            .debounce_ms
            .zip(self.last_trigger_unix_ms)
            .is_some_and(|(debounce_ms, last_ms)| {
                event.timestamp_unix_ms.saturating_sub(last_ms) < debounce_ms
            })
        {
            return Ok(None);
        }

        let mut matched_rules = Vec::new();

        if let Some(regex) = &self.config.regex
            && regex.is_match(&event.output)
        {
            matched_rules.push(format!("regex:{}", regex.as_str()));
        }

        if let Some(threshold) = self.config.changed_cells
            && event.changed_cell_count >= threshold
        {
            matched_rules.push(format!("changed-cells:{}", event.changed_cell_count));
        }

        if let Some(every) = self.config.every
            && every > 0
            && self.changed_frame_count % every == 0
        {
            matched_rules.push(format!("every:{every}"));
        }

        if self.config.regex.is_none()
            && self.config.changed_cells.is_none()
            && self.config.every.is_none()
        {
            matched_rules.push("changed".to_string());
        }

        if matched_rules.is_empty() {
            return Ok(None);
        }

        let payload = AfterCommandPayload {
            command: self.config.command_display.clone(),
            changed: event.changed,
            output: event.output,
            unix_timestamp: event.timestamp_unix_ms / 1000,
            frame_seq: event.frame_seq,
            width: event.width,
            height: event.height,
            changed_cell_count: event.changed_cell_count,
            matched_rules: matched_rules.clone(),
            last_input_summary: event.last_input_summary,
        };

        match self.tx.try_send(payload) {
            Ok(()) => {
                self.last_trigger_unix_ms = Some(event.timestamp_unix_ms);
                Ok(Some(matched_rules.join(" | ")))
            }
            Err(TrySendError::Full(_)) => Ok(Some("dropped: worker busy".to_string())),
            Err(TrySendError::Disconnected(_)) => Ok(Some("dropped: worker stopped".to_string())),
        }
    }
}

fn run_hook_with_timeout(
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
        thread::sleep(Duration::from_millis(10));
    }
}

fn parse_shell(shell: &str) -> (String, Vec<String>) {
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

#[cfg(test)]
mod tests {
    use super::{AfterCommandConfig, AfterCommandEvent, AfterCommandRuntime};

    fn runtime() -> AfterCommandRuntime {
        AfterCommandRuntime::new(AfterCommandConfig {
            hook: "echo noop".to_string(),
            shell: "sh -c".to_string(),
            command_display: "demo".to_string(),
            regex: None,
            changed_cells: None,
            every: None,
            debounce_ms: None,
            timeout_ms: 100,
        })
    }

    #[test]
    fn triggers_on_changed_by_default() {
        let mut runtime = runtime();
        let result = runtime
            .evaluate_and_enqueue(AfterCommandEvent {
                changed: true,
                output: "hello".to_string(),
                timestamp_unix_ms: 1000,
                frame_seq: 1,
                width: 80,
                height: 24,
                changed_cell_count: 3,
                last_input_summary: String::new(),
            })
            .unwrap();

        assert_eq!(result.as_deref(), Some("changed"));
    }

    #[test]
    fn supports_regex_and_threshold_rules() {
        let mut runtime = AfterCommandRuntime::new(AfterCommandConfig {
            hook: "echo noop".to_string(),
            shell: "sh -c".to_string(),
            command_display: "demo".to_string(),
            regex: Some(regex::Regex::new("panic").unwrap()),
            changed_cells: Some(10),
            every: None,
            debounce_ms: None,
            timeout_ms: 100,
        });
        let result = runtime
            .evaluate_and_enqueue(AfterCommandEvent {
                changed: true,
                output: "panic happened".to_string(),
                timestamp_unix_ms: 1000,
                frame_seq: 1,
                width: 80,
                height: 24,
                changed_cell_count: 12,
                last_input_summary: String::new(),
            })
            .unwrap()
            .unwrap();

        assert!(result.contains("regex:panic"));
        assert!(result.contains("changed-cells:12"));
    }

    #[test]
    fn respects_debounce() {
        let mut runtime = AfterCommandRuntime::new(AfterCommandConfig {
            hook: "echo noop".to_string(),
            shell: "sh -c".to_string(),
            command_display: "demo".to_string(),
            regex: None,
            changed_cells: None,
            every: Some(1),
            debounce_ms: Some(100),
            timeout_ms: 100,
        });

        let first = runtime
            .evaluate_and_enqueue(AfterCommandEvent {
                changed: true,
                output: "one".to_string(),
                timestamp_unix_ms: 1000,
                frame_seq: 1,
                width: 80,
                height: 24,
                changed_cell_count: 1,
                last_input_summary: String::new(),
            })
            .unwrap();
        let second = runtime
            .evaluate_and_enqueue(AfterCommandEvent {
                changed: true,
                output: "two".to_string(),
                timestamp_unix_ms: 1050,
                frame_seq: 2,
                width: 80,
                height: 24,
                changed_cell_count: 1,
                last_input_summary: String::new(),
            })
            .unwrap();

        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[test]
    fn payload_serializes_debug_fields() {
        let payload = super::AfterCommandPayload {
            command: "demo".to_string(),
            changed: true,
            output: "panic".to_string(),
            unix_timestamp: 1,
            frame_seq: 42,
            width: 80,
            height: 24,
            changed_cell_count: 7,
            matched_rules: vec!["regex:panic".to_string()],
            last_input_summary: "input: Enter".to_string(),
        };

        let json = serde_json::to_string(&payload).unwrap();

        assert!(json.contains("\"frame_seq\":42"));
        assert!(json.contains("\"changed_cell_count\":7"));
        assert!(json.contains("\"last_input_summary\":\"input: Enter\""));
    }
}
