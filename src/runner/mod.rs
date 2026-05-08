use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use crossterm::event::{KeyEvent, MouseEvent};
use time::{OffsetDateTime, format_description::FormatItem, macros::format_description};

use crate::logging::load_records;
use crate::screen::ScreenSnapshot;

mod capture_hook;
mod encode;
mod pty;

pub use pty::PtyRunner;

const TIME_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]");

#[derive(Clone, Debug)]
pub struct CaptureFrame {
    pub label: String,
    pub timestamp_unix_ms: u64,
    pub snapshot: ScreenSnapshot,
    pub raw_output: String,
    pub changed: bool,
}

pub trait FrameSource {
    fn capture(&mut self, width: u16, height: u16) -> Result<CaptureFrame>;
    fn resize(&mut self, width: u16, height: u16) -> Result<()>;
    fn send_key(&mut self, key: KeyEvent) -> Result<()>;
    fn send_mouse(&mut self, event: MouseEvent, body_row_offset: u16) -> Result<()>;
    fn toggle_child_pause(&mut self) -> Result<Option<bool>> {
        Ok(None)
    }
    fn supports_child_pause(&self) -> bool {
        false
    }
    fn has_pending_update(&self) -> bool;
    fn is_event_driven(&self) -> bool;
    fn take_update_receiver(&mut self) -> Option<Receiver<SourceEvent>>;
    fn terminate(&mut self) -> Result<()>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceEvent {
    Updated,
    Closed,
}

pub struct DemoRunner {
    sequence: u64,
    last_snapshot: Option<ScreenSnapshot>,
}

pub struct ReplayRunner {
    frame: CaptureFrame,
}

impl DemoRunner {
    pub fn new() -> Self {
        Self {
            sequence: 0,
            last_snapshot: None,
        }
    }
}

impl ReplayRunner {
    pub fn from_log(path: &str) -> Result<Self> {
        let records = load_records(path)?;
        let record = records
            .last()
            .cloned()
            .context("replay log does not contain any frames")?;
        Ok(Self {
            frame: CaptureFrame {
                label: record.label,
                timestamp_unix_ms: record.timestamp_unix_ms,
                raw_output: record.snapshot.lines().join("\n"),
                snapshot: record.snapshot,
                changed: false,
            },
        })
    }
}

impl FrameSource for DemoRunner {
    fn capture(&mut self, width: u16, height: u16) -> Result<CaptureFrame> {
        self.sequence += 1;
        let phase = (self.sequence % 4) as usize;
        let states = [
            ("starting", "booting workers", "queue=12 inflight=1"),
            ("running", "workers healthy", "queue=8 inflight=3"),
            ("degraded", "worker-2 retrying", "queue=14 inflight=2"),
            ("recovered", "all workers healthy", "queue=7 inflight=4"),
        ];
        let (status, details, counters) = states[phase];

        let snapshot = ScreenSnapshot::from_text_lines(
            width,
            height,
            &[
                "service: demo-api",
                &format!("status: {status}"),
                details,
                counters,
                &format!("last tick: {:04}", self.sequence),
            ],
        );
        let raw_output = snapshot.lines().join("\n");
        let changed = self.last_snapshot.as_ref() != Some(&snapshot);
        self.last_snapshot = Some(snapshot.clone());

        Ok(CaptureFrame {
            label: time_label(),
            timestamp_unix_ms: unix_timestamp_millis(),
            snapshot,
            raw_output,
            changed,
        })
    }

    fn resize(&mut self, _width: u16, _height: u16) -> Result<()> {
        Ok(())
    }

    fn send_key(&mut self, _key: KeyEvent) -> Result<()> {
        Ok(())
    }

    fn send_mouse(&mut self, _event: MouseEvent, _body_row_offset: u16) -> Result<()> {
        Ok(())
    }

    fn has_pending_update(&self) -> bool {
        false
    }

    fn is_event_driven(&self) -> bool {
        false
    }

    fn take_update_receiver(&mut self) -> Option<Receiver<SourceEvent>> {
        None
    }

    fn terminate(&mut self) -> Result<()> {
        Ok(())
    }
}

impl FrameSource for ReplayRunner {
    fn capture(&mut self, _width: u16, _height: u16) -> Result<CaptureFrame> {
        Ok(self.frame.clone())
    }

    fn resize(&mut self, _width: u16, _height: u16) -> Result<()> {
        Ok(())
    }

    fn send_key(&mut self, _key: KeyEvent) -> Result<()> {
        Ok(())
    }

    fn send_mouse(&mut self, _event: MouseEvent, _body_row_offset: u16) -> Result<()> {
        Ok(())
    }

    fn has_pending_update(&self) -> bool {
        false
    }

    fn is_event_driven(&self) -> bool {
        true
    }

    fn take_update_receiver(&mut self) -> Option<Receiver<SourceEvent>> {
        None
    }

    fn terminate(&mut self) -> Result<()> {
        Ok(())
    }
}

pub(crate) fn time_label() -> String {
    OffsetDateTime::now_local()
        .or_else(|_| Ok(OffsetDateTime::now_utc()))
        .and_then(|now| now.format(TIME_FORMAT))
        .unwrap_or_else(|_| "00:00:00".to_string())
}

pub(crate) fn unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::{DemoRunner, FrameSource, ReplayRunner};
    use crate::logging::{LogRecord, append_record};
    use crate::runner::capture_hook::parse_shell;
    use crate::screen::ScreenSnapshot;

    #[test]
    fn parse_shell_falls_back_to_platform_default() {
        let (program, args) = parse_shell("\"");

        #[cfg(windows)]
        {
            assert_eq!(program, "cmd");
            assert_eq!(args, vec!["/C".to_string()]);
        }

        #[cfg(not(windows))]
        {
            assert_eq!(program, "sh");
            assert_eq!(args, vec!["-c".to_string()]);
        }
    }

    #[test]
    fn replay_runner_returns_latest_frame_from_log() {
        let path =
            std::env::temp_dir().join(format!("twatch-replay-runner-{}.jsonl", std::process::id()));
        let path_str = path.to_string_lossy().into_owned();

        append_record(
            &path_str,
            &LogRecord {
                label: "a".to_string(),
                changed: true,
                timestamp_unix_ms: 1,
                frame_seq: 1,
                width: 20,
                height: 5,
                changed_cell_count: 1,
                input_event_count_since_prev: 0,
                resized: false,
                resize_from_width: 0,
                resize_from_height: 0,
                resize_to_width: 0,
                resize_to_height: 0,
                resize_source: String::new(),
                snapshot: ScreenSnapshot::from_text_lines(20, 5, &["one"]),
            },
        )
        .unwrap();
        append_record(
            &path_str,
            &LogRecord {
                label: "b".to_string(),
                changed: true,
                timestamp_unix_ms: 2,
                frame_seq: 2,
                width: 20,
                height: 5,
                changed_cell_count: 1,
                input_event_count_since_prev: 0,
                resized: false,
                resize_from_width: 0,
                resize_from_height: 0,
                resize_to_width: 0,
                resize_to_height: 0,
                resize_source: String::new(),
                snapshot: ScreenSnapshot::from_text_lines(20, 5, &["two"]),
            },
        )
        .unwrap();

        let mut runner = ReplayRunner::from_log(&path_str).unwrap();
        let frame = FrameSource::capture(&mut runner, 20, 5).unwrap();

        assert_eq!(frame.label, "b");
        assert_eq!(frame.raw_output, "two\n\n\n\n");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn non_pty_sources_do_not_support_child_pause() {
        let mut demo = DemoRunner::new();
        assert!(!demo.supports_child_pause());
        assert!(demo.toggle_child_pause().unwrap().is_none());

        let mut replay = ReplayRunner {
            frame: super::CaptureFrame {
                label: "x".to_string(),
                timestamp_unix_ms: 1,
                snapshot: ScreenSnapshot::from_text_lines(10, 3, &["x"]),
                raw_output: "x".to_string(),
                changed: false,
            },
        };
        assert!(!replay.supports_child_pause());
        assert!(replay.toggle_child_pause().unwrap().is_none());
    }
}
