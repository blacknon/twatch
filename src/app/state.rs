use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use regex::Regex;

use super::{App, AppHistoryMetadata, FilterMode, FocusPane, InputMode};
use crate::cli::Cli;
use crate::history::{HistoryMetadata, HistoryStore};
use crate::logging::{append_record, load_records, LogRecord};
use crate::runner::{CaptureFrame, FrameSource};
use crate::screen::ScreenSnapshot;
use crate::screenshot::save_snapshot;

impl App {
    pub fn new(cli: &Cli, source: Box<dyn FrameSource>) -> Result<Self> {
        let mut app = Self {
            interval_secs: cli.interval,
            paused: false,
            show_history: false,
            show_help: false,
            show_exit_confirm: false,
            diff_only: false,
            diff_mode: cli.differences.into(),
            focus: FocusPane::Watch,
            app_input_mode: false,
            watch_scroll: 0,
            horizontal_scroll: 0,
            filter_query: String::new(),
            status_message: None,
            selected_index: 0,
            follow_latest: true,
            input_mode: InputMode::Normal,
            filter_mode: FilterMode::Plain,
            current_snapshot: None,
            current_label: None,
            current_changed: false,
            history: HistoryStore::new(cli.checkpoint_interval, cli.compress),
            metadata: Vec::new(),
            filtered: Vec::new(),
            limit: cli.limit.max(1),
            checkpoint_interval: cli.checkpoint_interval.max(1),
            compress: cli.compress,
            logfile: cli.logfile.clone(),
            screenshot_dir: PathBuf::from(&cli.screenshot_dir),
            screenshot_format: cli.screenshot_format.into(),
            command_display: if cli.command.is_empty() {
                "demo".to_string()
            } else {
                cli.command.join(" ")
            },
            source,
            last_tick: Instant::now(),
            last_mouse_input: None,
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
        self.current_label.as_deref()
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

    pub fn selected_snapshot(&self) -> Option<ScreenSnapshot> {
        if self.follow_latest {
            self.current_snapshot.clone()
        } else {
            self.history.snapshot(self.selected_index).ok().flatten()
        }
    }

    pub fn previous_snapshot(&self) -> Option<ScreenSnapshot> {
        if self.follow_latest {
            if self.history.is_empty() {
                None
            } else {
                self.history.snapshot(self.history.len() - 1).ok().flatten()
            }
        } else if self.selected_index == 0 {
            None
        } else {
            self.history
                .snapshot(self.selected_index - 1)
                .ok()
                .flatten()
        }
    }

    pub fn selected_lines(&self) -> Vec<String> {
        self.selected_snapshot()
            .map(|snapshot| snapshot.lines())
            .unwrap_or_default()
    }

    pub fn previous_lines(&self) -> Option<Vec<String>> {
        self.previous_snapshot().map(|snapshot| snapshot.lines())
    }

    pub fn capture(&mut self, width: u16, height: u16) -> Result<()> {
        let CaptureFrame {
            label,
            snapshot,
            raw_output,
            changed,
        } = self.source.capture(width, height)?;

        if !changed && self.current_snapshot.is_some() {
            self.last_tick = Instant::now();
            return Ok(());
        }

        let current_changed = if raw_output.is_empty() && self.current_snapshot.is_none() {
            false
        } else {
            changed
        };

        self.archive_current_snapshot()?;
        self.set_current_snapshot(snapshot, label, current_changed);

        if !self.follow_latest && self.history.is_empty() {
            self.follow_latest = true;
        }
        self.rebuild_filter()?;
        self.append_log_record()?;
        self.last_tick = Instant::now();
        Ok(())
    }

    fn archive_current_snapshot(&mut self) -> Result<()> {
        let (Some(previous_snapshot), Some(previous_label)) =
            (self.current_snapshot.take(), self.current_label.take())
        else {
            return Ok(());
        };

        self.history.push(
            previous_snapshot,
            HistoryMetadata {
                label: previous_label.clone(),
            },
        )?;
        self.metadata.push(AppHistoryMetadata {
            label: previous_label,
            changed: self.current_changed,
        });
        if self.metadata.len() > self.limit {
            self.trim_history()?;
        }
        Ok(())
    }

    fn set_current_snapshot(&mut self, snapshot: ScreenSnapshot, label: String, changed: bool) {
        self.current_snapshot = Some(snapshot);
        self.current_label = Some(label);
        self.current_changed = changed;
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

    pub(super) fn rebuild_filter(&mut self) -> Result<()> {
        self.filtered = match self.filter_mode {
            FilterMode::Plain => self.history.find_by_query(&self.filter_query)?,
            FilterMode::Regex => {
                if self.filter_query.is_empty() {
                    (0..self.history.len()).collect()
                } else {
                    match Regex::new(&self.filter_query) {
                        Ok(regex) => self.history.find_by_regex(&regex)?,
                        Err(err) => {
                            self.status_message = Some(format!("regex error: {err}"));
                            Vec::new()
                        }
                    }
                }
            }
        };
        self.filtered.reverse();
        if self.filter_query.is_empty() {
            if self.filtered.is_empty() {
                self.follow_latest = true;
            } else if !self.follow_latest && !self.filtered.contains(&self.selected_index) {
                self.selected_index = *self.filtered.first().unwrap_or(&0);
            }
            return Ok(());
        }

        if self.filtered.is_empty() {
            self.follow_latest = true;
        } else if !self.follow_latest && !self.filtered.contains(&self.selected_index) {
            self.selected_index = *self.filtered.first().unwrap_or(&0);
        }
        Ok(())
    }

    pub(super) fn delete_selected_history(&mut self) -> Result<()> {
        if self.focus != FocusPane::History || self.follow_latest {
            self.status_message = Some("delete works on selected history".to_string());
            return Ok(());
        }
        let selected = self.selected_index;
        self.rebuild_history_retaining(|index| index != selected)?;
        self.follow_latest = true;
        self.status_message = Some("history deleted".to_string());
        Ok(())
    }

    pub(super) fn clear_history_except_selected(&mut self) -> Result<()> {
        if self.follow_latest {
            self.rebuild_history_retaining(|_| false)?;
            self.follow_latest = true;
            self.status_message = Some("history cleared; latest kept".to_string());
            return Ok(());
        }

        let selected = self.selected_index;
        self.rebuild_history_retaining(|index| index == selected)?;
        if !self.history.is_empty() {
            self.follow_latest = false;
            self.selected_index = 0;
        } else {
            self.follow_latest = true;
        }
        self.status_message = Some("history cleared except selected".to_string());
        Ok(())
    }

    fn rebuild_history_retaining<F>(&mut self, mut keep: F) -> Result<()>
    where
        F: FnMut(usize) -> bool,
    {
        let mut rebuilt = HistoryStore::new(self.checkpoint_interval, self.compress);
        let mut rebuilt_meta = Vec::new();

        for index in 0..self.history.len() {
            if !keep(index) {
                continue;
            }
            if let Some(snapshot) = self.history.snapshot(index)? {
                let meta = self.metadata[index].clone();
                rebuilt.push(
                    snapshot,
                    HistoryMetadata {
                        label: meta.label.clone(),
                    },
                )?;
                rebuilt_meta.push(meta);
            }
        }

        self.history = rebuilt;
        self.metadata = rebuilt_meta;
        self.selected_index = self.history.len().saturating_sub(1);
        self.rebuild_filter()?;
        Ok(())
    }

    pub(super) fn save_snapshot(&mut self) -> Result<()> {
        let Some(snapshot) = self.selected_snapshot() else {
            self.status_message = Some("no snapshot to save".to_string());
            return Ok(());
        };

        let label = self
            .current_label()
            .unwrap_or("snapshot")
            .chars()
            .map(|ch| match ch {
                '0'..='9' | 'A'..='Z' | 'a'..='z' | '-' | '_' => ch,
                _ => '_',
            })
            .collect::<String>();
        let path = self.screenshot_dir.join(format!(
            "twatch-{label}.{}",
            self.screenshot_format.extension()
        ));
        let header = [
            format!("twatch snapshot | {}", self.command_display()),
            format!(
                "filter: {}{}",
                match self.filter_mode {
                    FilterMode::Plain => "/",
                    FilterMode::Regex => "*",
                },
                self.filter_query
            ),
        ];
        save_snapshot(&snapshot, &path, self.screenshot_format, &header)?;
        self.status_message = Some(format!(
            "snapshot saved: {} ({})",
            display_tmp_path(&path),
            self.screenshot_format.label()
        ));
        Ok(())
    }

    pub(super) fn cycle_screenshot_format(&mut self) {
        self.screenshot_format = self.screenshot_format.cycle();
        self.status_message = Some(format!(
            "snapshot format: {} -> {}",
            self.screenshot_format.label(),
            display_tmp_path(&self.screenshot_dir)
        ));
    }

    fn trim_history(&mut self) -> Result<()> {
        let total = self.history.len();
        if total <= self.limit {
            return Ok(());
        }

        let start = total - self.limit;
        let mut rebuilt = HistoryStore::new(self.checkpoint_interval, self.compress);
        let mut rebuilt_meta = Vec::with_capacity(self.limit);

        for index in start..total {
            if let Some(snapshot) = self.history.snapshot(index)? {
                let meta = self.metadata[index].clone();
                rebuilt.push(
                    snapshot,
                    HistoryMetadata {
                        label: meta.label.clone(),
                    },
                )?;
                rebuilt_meta.push(meta);
            }
        }

        self.history = rebuilt;
        self.metadata = rebuilt_meta;
        if self.history.is_empty() {
            self.follow_latest = true;
            self.selected_index = 0;
        } else if self.selected_index < start {
            self.selected_index = 0;
            self.sync_follow_latest_with_selection();
        } else {
            self.selected_index -= start;
        }
        Ok(())
    }

    fn load_history_from_log(&mut self) -> Result<()> {
        let Some(path) = &self.logfile else {
            return Ok(());
        };

        for record in load_records(path)? {
            let (snapshot, metadata, changed) = record.into_parts();
            self.archive_current_snapshot()?;
            self.set_current_snapshot(snapshot, metadata.label, changed);
        }

        if self.metadata.len() > self.limit {
            self.trim_history()?;
        }

        if self.current_snapshot.is_some() {
            self.selected_index = self.history.len().saturating_sub(1);
            self.follow_latest = true;
            self.rebuild_filter()?;
        }

        Ok(())
    }

    fn append_log_record(&self) -> Result<()> {
        let Some(path) = &self.logfile else {
            return Ok(());
        };
        let Some(snapshot) = &self.current_snapshot else {
            return Ok(());
        };
        let Some(label) = &self.current_label else {
            return Ok(());
        };

        append_record(
            path,
            &LogRecord {
                label: label.clone(),
                changed: self.current_changed,
                snapshot: snapshot.clone(),
            },
        )
    }
}

fn display_tmp_path(path: &PathBuf) -> String {
    path.to_string_lossy().into_owned()
}
