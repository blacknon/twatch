use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use regex::Regex;
use serde::Serialize;

mod rules;
mod worker;

use rules::evaluate_rules;
use worker::{parse_shell, run_hook_with_timeout};

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
        let matched_rules = evaluate_rules(
            &self.config,
            &event,
            &mut self.changed_frame_count,
            self.last_trigger_unix_ms,
        );

        let Some(matched_rules) = matched_rules else {
            return Ok(None);
        };

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

#[cfg(test)]
mod tests {
    use super::{AfterCommandConfig, AfterCommandEvent, AfterCommandPayload, AfterCommandRuntime};

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
        let payload = AfterCommandPayload {
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
