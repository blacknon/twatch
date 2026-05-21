// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use anyhow::Result;
use std::sync::mpsc;

use super::config::AppConfig;
use super::history_ops::build_replay_replace_state;
use super::{App, AppHistoryMetadata, FilterMode, InputMode, ReplayLoadMessage};
use crate::cli::Cli;
use crate::history::HistoryStore;
use crate::runner::FrameSource;

pub(super) const REPLAY_INITIAL_LOAD_FRAMES: usize = 16;
pub(super) const REPLAY_SYNC_SMALL_SPILL_MAX_BYTES: u64 = 128 * 1024;

pub(super) fn replay_prefetch_record_count() -> usize {
    REPLAY_INITIAL_LOAD_FRAMES.saturating_add(1)
}

impl App {
    pub fn new(cli: &Cli, source: Box<dyn FrameSource>) -> Result<Self> {
        let config = AppConfig::from_cli(cli, source.supports_child_pause())?;
        let mut app = Self {
            debug: config.debug,
            hide_header: config.hide_header,
            paused: false,
            child_paused: false,
            child_pause_supported: config.child_pause_supported,
            diff_only: false,
            diff_mode: config.diff_mode,
            selected_index: 0,
            follow_latest: true,
            input_mode: InputMode::Normal,
            filter_mode: FilterMode::Plain,
            current_snapshot: None,
            current_metadata: None,
            history: HistoryStore::new(config.checkpoint_interval, config.compress),
            metadata: Vec::new(),
            filtered: Vec::new(),
            limit: config.history_limit,
            checkpoint_interval: config.checkpoint_interval,
            compress: config.compress,
            logfile: config.logfile,
            replay_mode: config.replay_mode,
            screenshot_dir: config.screenshot_dir,
            screenshot_format: config.screenshot_format,
            snapshot_on: config.snapshot_on,
            snapshot_on_regex: config.snapshot_on_regex,
            snapshot_on_change_cells: config.snapshot_on_change_cells,
            snapshot_once: config.snapshot_once,
            snapshot_trigger_fired: false,
            aftercommand_runtime: config.aftercommand_runtime,
            command_display: config.command_display,
            keymap: config.keymap,
            child_bindings: config.child_bindings,
            source,
            replay_loader: None,
            replay_reload_path: None,
            replay_deferred_path: None,
            replay_loader_rx: None,
            replay_loading: false,
            last_mouse_input: None,
            last_mouse_scroll_input: None,
            pending_mouse_escape: None,
            live_scrollback_offset: 0,
            live_scrollback_snapshot: None,
            next_frame_seq: 1,
            trace: super::TraceState::new(),
            view: super::ViewState::new(),
            ui: super::UiState::new(),
        };

        if config.replay_mode {
            app.load_history_from_replay_log_prefetch()?;
        } else {
            app.load_history_from_log()?;
        }
        Ok(app)
    }

    pub(crate) fn header_rows(&self) -> u16 {
        if self.hide_header { 0 } else { 2 }
    }

    pub(crate) fn content_height(&self, total_height: u16) -> u16 {
        total_height.saturating_sub(self.header_rows())
    }

    pub(super) fn start_replay_loader(&mut self) {
        if let Some(path) = self.replay_reload_path.take() {
            self.start_replay_replace_loader_for_path(path);
            return;
        }

        let Some(mut loader) = self.replay_loader.take() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.replay_loader_rx = Some(rx);
        self.replay_loading = true;
        std::thread::spawn(move || {
            const CHUNK_SIZE: usize = 256;
            loop {
                let mut chunk = Vec::with_capacity(CHUNK_SIZE);
                for _ in 0..CHUNK_SIZE {
                    match loader.next_record() {
                        Ok(Some(record)) => chunk.push(record),
                        Ok(None) => {
                            if !chunk.is_empty()
                                && tx.send(ReplayLoadMessage::Records(chunk)).is_err()
                            {
                                return;
                            }
                            let _ = tx.send(ReplayLoadMessage::Finished);
                            return;
                        }
                        Err(err) => {
                            let _ = tx.send(ReplayLoadMessage::Failed(err.to_string()));
                            return;
                        }
                    }
                }

                if tx.send(ReplayLoadMessage::Records(chunk)).is_err() {
                    return;
                }
            }
        });
    }

    pub(super) fn start_deferred_replay_loader(&mut self) -> bool {
        let Some(path) = self.replay_deferred_path.take() else {
            return false;
        };
        self.start_replay_replace_loader_for_path(path);
        true
    }

    fn start_replay_replace_loader_for_path(&mut self, path: String) {
        let (tx, rx) = mpsc::channel();
        self.replay_loader_rx = Some(rx);
        self.replay_loading = true;
        let checkpoint_interval = self.checkpoint_interval;
        let compress = self.compress;
        std::thread::spawn(move || {
            match build_replay_replace_state(&path, checkpoint_interval, compress) {
                Ok(state) => {
                    if tx
                        .send(ReplayLoadMessage::ReplaceState(Box::new(state)))
                        .is_err()
                    {
                        return;
                    }
                    let _ = tx.send(ReplayLoadMessage::Finished);
                }
                Err(err) => {
                    let _ = tx.send(ReplayLoadMessage::Failed(err.to_string()));
                }
            }
        });
    }

    pub(super) fn command_display_from_cli(cli: &Cli) -> String {
        if let Some(path) = &cli.replay {
            format!("replay: {path}")
        } else if cli.command.is_empty() {
            "demo".to_string()
        } else {
            cli.command.join(" ")
        }
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

    pub fn source_debug_status(&self) -> Option<String> {
        if self.debug {
            self.source.debug_status()
        } else {
            None
        }
    }

    pub fn selected_snapshot_is_alternate_screen(&self) -> bool {
        self.selected_snapshot()
            .map(|snapshot| snapshot.alternate_screen())
            .unwrap_or(false)
    }

    pub fn selected_snapshot_has_mouse_reporting(&self) -> bool {
        self.selected_snapshot()
            .map(|snapshot| snapshot.mouse_reporting())
            .unwrap_or(false)
    }

    pub(crate) fn should_render_terminal_cursor(&self) -> bool {
        self.follow_latest && self.live_scrollback_offset == 0 && self.ui.watch_scroll == 0
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

    pub(super) fn trim_slack(&self) -> usize {
        if self.limit == 0 {
            return 0;
        }
        self.limit.min(self.checkpoint_interval.max(32))
    }

    pub(super) fn trim_trigger_len(&self) -> usize {
        if self.limit == 0 {
            return usize::MAX;
        }
        self.limit.saturating_add(self.trim_slack())
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
