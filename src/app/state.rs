use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use regex::Regex;

use super::{App, AppHistoryMetadata, FilterMode, FocusPane, InputMode};
use crate::cli::Cli;
use crate::history::HistoryStore;
use crate::runner::FrameSource;

impl App {
    pub fn new(cli: &Cli, source: Box<dyn FrameSource>) -> Result<Self> {
        let mut app = Self {
            interval_secs: cli.interval,
            paused: false,
            show_history: false,
            show_help: false,
            show_exit_confirm: false,
            show_inspector: false,
            diff_only: false,
            diff_mode: cli.differences.into(),
            focus: FocusPane::Watch,
            app_input_mode: false,
            watch_scroll: 0,
            horizontal_scroll: 0,
            inspect_x: 0,
            inspect_y: 0,
            filter_query: String::new(),
            status_message: None,
            selected_index: 0,
            follow_latest: true,
            input_mode: InputMode::Normal,
            filter_mode: FilterMode::Plain,
            current_snapshot: None,
            current_metadata: None,
            history: HistoryStore::new(cli.checkpoint_interval, cli.compress),
            metadata: Vec::new(),
            filtered: Vec::new(),
            limit: cli.limit.max(1),
            checkpoint_interval: cli.checkpoint_interval.max(1),
            compress: cli.compress,
            logfile: cli.logfile.clone(),
            screenshot_dir: PathBuf::from(&cli.screenshot_dir),
            screenshot_format: cli.screenshot_format.into(),
            snapshot_on: cli.snapshot_on.clone(),
            snapshot_on_regex: cli
                .snapshot_on_regex
                .as_ref()
                .map(|pattern| {
                    Regex::new(pattern)
                        .with_context(|| format!("invalid snapshot regex: {pattern}"))
                })
                .transpose()?,
            snapshot_on_change_cells: cli.snapshot_on_change_cells,
            snapshot_once: cli.snapshot_once,
            snapshot_trigger_fired: false,
            command_display: if cli.command.is_empty() {
                "demo".to_string()
            } else {
                cli.command.join(" ")
            },
            source,
            last_tick: Instant::now(),
            last_mouse_input: None,
            next_frame_seq: 1,
            input_trace: std::collections::VecDeque::with_capacity(256),
            pending_input_events: Vec::new(),
            next_input_seq: 1,
            resize_trace: std::collections::VecDeque::with_capacity(64),
            pending_resize_event: None,
        };

        app.load_history_from_log()?;
        Ok(app)
    }

    pub fn history_len(&self) -> usize {
        self.metadata.len()
    }

    pub fn is_event_driven(&self) -> bool {
        self.source.is_event_driven()
    }

    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered
    }

    pub fn command_display(&self) -> &str {
        &self.command_display
    }

    pub fn screenshot_format(&self) -> crate::screenshot::ScreenshotFormat {
        self.screenshot_format
    }

    pub fn current_label(&self) -> Option<&str> {
        self.current_metadata
            .as_ref()
            .map(|metadata| metadata.label.as_str())
    }

    pub fn is_search_mode(&self) -> bool {
        self.input_mode == InputMode::Search
    }

    pub fn filter_mode(&self) -> FilterMode {
        self.filter_mode
    }

    pub fn history_metadata(&self, index: usize) -> &AppHistoryMetadata {
        &self.metadata[index]
    }

    pub fn selected_input_summary(&self) -> Option<&str> {
        if self.follow_latest {
            self.current_metadata
                .as_ref()
                .map(|metadata| metadata.input_summary.as_str())
        } else {
            self.metadata
                .get(self.selected_index)
                .map(|metadata| metadata.input_summary.as_str())
        }
        .filter(|summary| !summary.is_empty())
    }

    pub fn inspect_cursor(&self) -> (u16, u16) {
        (self.inspect_x, self.inspect_y)
    }

    pub fn selected_resize_summary(&self) -> Option<String> {
        let metadata = if self.follow_latest {
            self.current_metadata.as_ref()
        } else {
            self.metadata.get(self.selected_index)
        }?;

        if !metadata.resized {
            return None;
        }

        Some(format!(
            "resize: {}x{} -> {}x{} ({})",
            metadata.resize_from_width,
            metadata.resize_from_height,
            metadata.resize_to_width,
            metadata.resize_to_height,
            metadata.resize_source
        ))
    }

    pub fn selected_filtered_position(&self) -> Option<usize> {
        self.filtered
            .iter()
            .position(|idx| *idx == self.selected_index)
    }

    pub fn selected_history_row(&self) -> usize {
        if self.follow_latest {
            0
        } else {
            self.selected_filtered_position()
                .map(|position| position + 1)
                .unwrap_or(0)
        }
    }

    pub(super) fn tick_timeout(&self) -> Duration {
        let interval = Duration::from_secs_f64(self.interval_secs.max(0.2));
        interval.saturating_sub(self.last_tick.elapsed())
    }

    pub(super) fn should_capture_now(&self) -> bool {
        if self.source.is_event_driven() {
            self.source.has_pending_update()
        } else {
            self.last_tick.elapsed() >= Duration::from_secs_f64(self.interval_secs.max(0.2))
        }
    }

    pub(super) fn is_source_closed_error(&self, err: &anyhow::Error) -> bool {
        err.chain().any(|cause| {
            cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
                matches!(
                    io.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::NotConnected
                ) || io.raw_os_error() == Some(5)
            })
        })
    }
}
