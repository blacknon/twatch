// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use super::{App, FilterMode, FocusPane};
use crate::cli::{Cli, DiffModeArg, ScreenshotFormatArg};
use crate::logging::{LogRecord, append_record};
use crate::runner::{CaptureFrame, FrameSource, SourceEvent};
use crate::screen::ScreenSnapshot;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use crossterm::event::{KeyEvent, KeyEventState, MouseButton, MouseEvent, MouseEventKind};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct MockSource {
    frames: Vec<CaptureFrame>,
    next: usize,
    child_pause_supported: bool,
    child_paused: bool,
    mouse_passthrough_enabled: bool,
    view_snapshot_requests: Arc<Mutex<Vec<usize>>>,
    key_events: Arc<Mutex<Vec<KeyEvent>>>,
    byte_events: Arc<Mutex<Vec<Vec<u8>>>>,
    mouse_events: Arc<Mutex<Vec<MouseEvent>>>,
}

impl MockSource {
    fn new(frames: Vec<CaptureFrame>) -> Self {
        Self {
            frames,
            next: 0,
            child_pause_supported: false,
            child_paused: false,
            mouse_passthrough_enabled: true,
            view_snapshot_requests: Arc::new(Mutex::new(Vec::new())),
            key_events: Arc::new(Mutex::new(Vec::new())),
            byte_events: Arc::new(Mutex::new(Vec::new())),
            mouse_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn without_mouse_passthrough(frames: Vec<CaptureFrame>) -> Self {
        Self {
            frames,
            next: 0,
            child_pause_supported: false,
            child_paused: false,
            mouse_passthrough_enabled: false,
            view_snapshot_requests: Arc::new(Mutex::new(Vec::new())),
            key_events: Arc::new(Mutex::new(Vec::new())),
            byte_events: Arc::new(Mutex::new(Vec::new())),
            mouse_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_child_pause(frames: Vec<CaptureFrame>) -> Self {
        Self {
            frames,
            next: 0,
            child_pause_supported: true,
            child_paused: false,
            mouse_passthrough_enabled: true,
            view_snapshot_requests: Arc::new(Mutex::new(Vec::new())),
            key_events: Arc::new(Mutex::new(Vec::new())),
            byte_events: Arc::new(Mutex::new(Vec::new())),
            mouse_events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl FrameSource for MockSource {
    fn capture(&mut self, _width: u16, _height: u16) -> Result<CaptureFrame> {
        let frame = self
            .frames
            .get(self.next)
            .cloned()
            .or_else(|| self.frames.last().cloned())
            .expect("mock frame");
        self.next += 1;
        Ok(frame)
    }

    fn view_snapshot(
        &mut self,
        _width: u16,
        _height: u16,
        scrollback_offset: usize,
    ) -> Result<Option<ScreenSnapshot>> {
        self.view_snapshot_requests
            .lock()
            .unwrap()
            .push(scrollback_offset);
        let mut snapshot = self
            .frames
            .get(self.next.saturating_sub(1))
            .or_else(|| self.frames.last())
            .map(|frame| frame.snapshot.clone())
            .unwrap_or_else(|| ScreenSnapshot::from_text_lines(20, 5, &["mock"]));
        snapshot.set_screen_mode(false, scrollback_offset.min(64));
        Ok(Some(snapshot))
    }

    fn resize(&mut self, _width: u16, _height: u16) -> Result<()> {
        Ok(())
    }

    fn send_key(&mut self, key: KeyEvent) -> Result<()> {
        self.key_events.lock().unwrap().push(key);
        Ok(())
    }

    fn send_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.byte_events.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }

    fn send_mouse(&mut self, event: MouseEvent, _body_row_offset: u16) -> Result<bool> {
        if !self.mouse_passthrough_enabled {
            return Ok(false);
        }
        self.mouse_events.lock().unwrap().push(event);
        Ok(true)
    }

    fn toggle_child_pause(&mut self) -> Result<Option<bool>> {
        if !self.child_pause_supported {
            return Ok(None);
        }
        self.child_paused = !self.child_paused;
        Ok(Some(self.child_paused))
    }

    fn supports_child_pause(&self) -> bool {
        self.child_pause_supported
    }

    fn debug_status(&self) -> Option<String> {
        None
    }

    fn has_pending_update(&self) -> bool {
        false
    }

    fn is_event_driven(&self) -> bool {
        false
    }

    fn take_update_receiver(&mut self) -> Option<std::sync::mpsc::Receiver<SourceEvent>> {
        None
    }

    fn terminate(&mut self) -> Result<()> {
        Ok(())
    }
}

#[test]
fn regex_filter_stops_following_latest_when_new_frame_does_not_match() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            frame("a", &["worker-01 ok"]),
            frame("b", &["worker-02 fail"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.filter_mode = FilterMode::Regex;
    app.ui.filter_query = "worker-01".to_string();
    app.rebuild_filter().unwrap();

    assert!(!app.follow_latest);
    assert_eq!(app.filtered, vec![0]);
    assert_eq!(app.selected_index, 0);
}

#[test]
fn plain_filter_stops_following_latest_when_new_frame_does_not_match() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            frame("a", &["worker-01 ok"]),
            frame("b", &["worker-02 fail"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.filter_mode = FilterMode::Plain;
    app.ui.filter_query = "worker-01".to_string();
    app.rebuild_filter().unwrap();

    assert!(app.follow_latest);

    app.capture(20, 5).unwrap();

    assert!(!app.follow_latest);
    assert_eq!(app.filtered, vec![0]);
    assert_eq!(app.selected_index, 0);
}

#[test]
fn escape_does_not_clear_committed_filter_in_normal_mode() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["worker-01 ok"])])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.filter_mode = FilterMode::Plain;
    app.ui.filter_query = "worker-01".to_string();
    app.rebuild_filter().unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.ui.filter_query, "worker-01");
    assert!(app.filtered.is_empty());
    assert!(app.follow_latest);
}

#[test]
fn ctrl_c_clears_committed_filter_in_normal_mode() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["worker-01 ok"])])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.filter_mode = FilterMode::Plain;
    app.ui.filter_query = "worker-01".to_string();
    app.rebuild_filter().unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.ui.filter_query.is_empty());
    assert!(app.follow_latest);
    assert!(!app.ui.show_exit_confirm);
}

#[test]
fn ctrl_c_opens_exit_when_filter_is_empty() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["worker-01 ok"])])),
    )
    .unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.ui.show_exit_confirm);
}

#[test]
fn ctrl_c_release_does_not_close_exit_dialog() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["worker-01 ok"])])),
    )
    .unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();
    app.handle_key_event(KeyEvent {
        code: KeyCode::Char('c'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Release,
        state: KeyEventState::empty(),
    })
    .unwrap();

    assert!(app.ui.show_exit_confirm);
}

#[test]
fn ignores_diff_ghost_key_immediately_after_mouse_scroll() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["worker-01 ok"])])),
    )
    .unwrap();

    app.last_mouse_scroll_input = Some(Instant::now());
    app.handle_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.diff_mode, super::DiffMode::None);

    app.last_mouse_scroll_input = Some(Instant::now());
    app.handle_key_event(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT))
        .unwrap();

    assert_eq!(app.diff_mode, super::DiffMode::None);
}

#[test]
fn backspace_release_does_not_retoggle_history_pane() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["worker-01 ok"])])),
    )
    .unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
        .unwrap();
    app.handle_key_event(KeyEvent {
        code: KeyCode::Backspace,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Release,
        state: KeyEventState::empty(),
    })
    .unwrap();

    assert!(app.ui.show_history);
}

#[test]
fn p_toggles_capture_pause_state() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["worker-01 ok"])])),
    )
    .unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
        .unwrap();
    assert!(app.paused);

    app.handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))
        .unwrap();
    assert!(!app.paused);
}

#[test]
fn does_not_ignore_diff_key_after_scroll_guard_window() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["worker-01 ok"])])),
    )
    .unwrap();

    app.last_mouse_scroll_input = Some(Instant::now() - Duration::from_millis(300));
    app.handle_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.diff_mode, super::DiffMode::Watch);
}

#[test]
fn delete_selected_history_removes_entry() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            frame("a", &["one"]),
            frame("b", &["two"]),
            frame("c", &["three"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.ui.focus = FocusPane::History;
    app.follow_latest = false;
    app.selected_index = 0;
    app.rebuild_filter().unwrap();

    app.delete_selected_history().unwrap();

    assert_eq!(app.history_len(), 1);
}

#[test]
fn clear_history_except_selected_keeps_only_target() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            frame("a", &["one"]),
            frame("b", &["two"]),
            frame("c", &["three"]),
            frame("d", &["four"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.ui.focus = FocusPane::History;
    app.follow_latest = false;
    app.selected_index = 1;
    app.rebuild_filter().unwrap();

    app.clear_history_except_selected().unwrap();

    assert_eq!(app.history_len(), 1);
    assert!(!app.follow_latest);
    assert_eq!(app.selected_index, 0);
    assert_eq!(app.history_metadata(0).label, "b");
}

#[test]
fn unchanged_frame_does_not_create_new_history_entry() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            frame("a", &["same"]),
            unchanged_frame("b", &["same"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();

    assert_eq!(app.history_len(), 0);
    assert_eq!(app.current_label(), Some("a"));
}

#[test]
fn stores_frame_metadata_for_history_entries() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            frame("a", &["one"]),
            frame("b", &["two"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();

    let meta = app.history_metadata(0);
    assert_eq!(meta.label, "a");
    assert_eq!(meta.frame_seq, 1);
    assert_eq!(meta.width, 20);
    assert_eq!(meta.height, 5);
    assert!(meta.changed);
    assert!(meta.changed_cell_count > 0);
}

#[test]
fn forwarded_input_is_recorded_in_frame_metadata() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            frame("a", &["one"]),
            frame("b", &["two"]),
            frame("c", &["three"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();

    let meta = app.history_metadata(1);
    assert_eq!(meta.input_event_count_since_prev, 1);
    assert!(meta.input_summary.contains("Enter"));
}

#[test]
fn resize_event_is_recorded_in_frame_metadata() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            frame("a", &["one"]),
            frame("b", &["two"]),
            frame("c", &["three"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.note_resize_event(30, 8, "terminal");
    app.capture(30, 8).unwrap();
    app.capture(30, 8).unwrap();

    let meta = app.history_metadata(1);
    assert!(meta.resized);
    assert_eq!(meta.resize_from_width, 20);
    assert_eq!(meta.resize_from_height, 5);
    assert_eq!(meta.resize_to_width, 30);
    assert_eq!(meta.resize_to_height, 8);
    assert_eq!(meta.resize_source, "terminal");
}

#[test]
fn inspector_toggles_and_moves() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["one", "two"])])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.handle_key_event(KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT))
        .unwrap();
    assert!(app.ui.show_inspector);

    app.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT))
        .unwrap();

    assert_eq!(app.inspect_cursor(), (1, 0));
}

#[test]
fn shift_s_toggles_history_details_and_opens_history() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["one"])])),
    )
    .unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT))
        .unwrap();

    assert!(app.ui.show_history);
    assert!(app.ui.show_history_details);
    assert_eq!(app.ui.focus, FocusPane::History);

    app.handle_key_event(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT))
        .unwrap();
    assert!(!app.ui.show_history_details);
}

#[test]
fn closing_history_hides_history_details() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["one"])])),
    )
    .unwrap();

    app.ui.show_history = true;
    app.ui.show_history_details = true;
    app.ui.focus = FocusPane::History;

    app.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
        .unwrap();

    assert!(!app.ui.show_history);
    assert!(!app.ui.show_history_details);
}

#[test]
fn history_overlay_window_tracks_selected_row_without_rendering_all_items() {
    let mut cli = test_cli();
    cli.limit = 800;
    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(
            (0..700)
                .map(|i| frame(&format!("{i:04}"), &["x"]))
                .collect(),
        )),
    )
    .unwrap();

    for _ in 0..700 {
        app.capture(20, 5).unwrap();
    }
    app.follow_latest = false;
    app.selected_index = 620;

    let (start, end) = app.history_overlay_window(12);
    assert!(end - start <= 12);
    assert!(start > 0);
    assert!(app.selected_history_row() >= start);
    assert!(app.selected_history_row() < end);
}

#[test]
fn history_overlay_row_selection_accounts_for_window_offset() {
    let mut cli = test_cli();
    cli.limit = 120;
    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(
            (0..80).map(|i| frame(&format!("{i:04}"), &["x"])).collect(),
        )),
    )
    .unwrap();

    for _ in 0..80 {
        app.capture(20, 5).unwrap();
    }
    app.follow_latest = false;
    app.selected_index = 40;

    let (start, _) = app.history_overlay_window(10);
    app.select_history_overlay_row(3, 10);

    if start + 3 == 0 {
        assert!(app.follow_latest);
    } else {
        assert_eq!(app.selected_index, app.filtered_indices()[start + 2]);
    }
}

#[test]
fn mouse_passthrough_is_blocked_when_not_following_latest() {
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let mouse_events = source.mouse_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.follow_latest = false;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();

    assert!(mouse_events.lock().unwrap().is_empty());
}

#[test]
fn mouse_passthrough_is_allowed_when_following_latest() {
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let mouse_events = source.mouse_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();

    assert_eq!(mouse_events.lock().unwrap().len(), 1);
}

#[test]
fn main_screen_mouse_scroll_uses_live_scrollback_view() {
    let mut snapshot = ScreenSnapshot::from_text_lines(20, 5, &["one"]);
    snapshot.set_screen_mode(false, 0);
    let source = MockSource::new(vec![CaptureFrame {
        label: "a".to_string(),
        timestamp_unix_ms: 1,
        snapshot,
        raw_output: "one".to_string(),
        changed: true,
    }]);
    let requests = source.view_snapshot_requests.clone();
    let mouse_events = source.mouse_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();

    assert_eq!(requests.lock().unwrap().as_slice(), &[3]);
    assert!(mouse_events.lock().unwrap().is_empty());
    assert_eq!(app.selected_snapshot().unwrap().scrollback_offset(), 3);
}

#[test]
fn primary_screen_mouse_tracking_passthroughs_wheel_events() {
    let mut snapshot = ScreenSnapshot::from_text_lines(20, 5, &["one"]);
    snapshot.set_screen_mode(false, 0);
    snapshot.set_mouse_reporting(true);
    let source = MockSource::new(vec![CaptureFrame {
        label: "a".to_string(),
        timestamp_unix_ms: 1,
        snapshot,
        raw_output: "one".to_string(),
        changed: true,
    }]);
    let requests = source.view_snapshot_requests.clone();
    let mouse_events = source.mouse_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();

    assert!(requests.lock().unwrap().is_empty());
    assert_eq!(mouse_events.lock().unwrap().len(), 1);
}

#[test]
fn alternate_screen_mouse_scroll_still_passthroughs_to_child() {
    let mut snapshot = ScreenSnapshot::from_text_lines(20, 5, &["one"]);
    snapshot.set_screen_mode(true, 0);
    let source = MockSource::new(vec![CaptureFrame {
        label: "a".to_string(),
        timestamp_unix_ms: 1,
        snapshot,
        raw_output: "one".to_string(),
        changed: true,
    }]);
    let mouse_events = source.mouse_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();

    assert_eq!(mouse_events.lock().unwrap().len(), 1);
}

#[test]
fn main_screen_mouse_scroll_works_in_app_input_mode() {
    let mut snapshot = ScreenSnapshot::from_text_lines(20, 5, &["one"]);
    snapshot.set_screen_mode(false, 0);
    let source = MockSource::new(vec![CaptureFrame {
        label: "a".to_string(),
        timestamp_unix_ms: 1,
        snapshot,
        raw_output: "one".to_string(),
        changed: true,
    }]);
    let requests = source.view_snapshot_requests.clone();
    let mouse_events = source.mouse_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    app.ui.app_input_mode = true;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();

    assert_eq!(requests.lock().unwrap().as_slice(), &[3]);
    assert!(mouse_events.lock().unwrap().is_empty());
    assert_eq!(app.selected_snapshot().unwrap().scrollback_offset(), 3);
}

#[test]
fn primary_screen_mouse_tracking_passthroughs_wheel_events_in_app_input_mode() {
    let mut snapshot = ScreenSnapshot::from_text_lines(20, 5, &["one"]);
    snapshot.set_screen_mode(false, 0);
    snapshot.set_mouse_reporting(true);
    let source = MockSource::new(vec![CaptureFrame {
        label: "a".to_string(),
        timestamp_unix_ms: 1,
        snapshot,
        raw_output: "one".to_string(),
        changed: true,
    }]);
    let requests = source.view_snapshot_requests.clone();
    let mouse_events = source.mouse_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    app.ui.app_input_mode = true;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();

    assert!(requests.lock().unwrap().is_empty());
    assert_eq!(mouse_events.lock().unwrap().len(), 1);
}

#[test]
fn main_screen_mouse_scroll_clamps_at_top_without_panicking() {
    let mut snapshot = ScreenSnapshot::from_text_lines(20, 5, &["one"]);
    snapshot.set_screen_mode(false, 0);
    let source = MockSource::new(vec![CaptureFrame {
        label: "a".to_string(),
        timestamp_unix_ms: 1,
        snapshot,
        raw_output: "one".to_string(),
        changed: true,
    }]);
    let requests = source.view_snapshot_requests.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    assert!(app.scroll_main_screen_view(9999).unwrap());
    let first = app.selected_snapshot().unwrap().scrollback_offset();
    assert_eq!(first, 64);

    assert!(!app.scroll_main_screen_view(9999).unwrap());
    let second = app.selected_snapshot().unwrap().scrollback_offset();
    assert_eq!(second, 64);
    assert_eq!(requests.lock().unwrap().as_slice(), &[9999, 10063]);
}

#[test]
fn terminal_cursor_is_hidden_while_live_scrollback_is_active() {
    let mut snapshot = ScreenSnapshot::from_text_lines(20, 5, &["one"]);
    snapshot.set_cursor_state(3, 1, true);
    snapshot.set_screen_mode(false, 0);
    let source = MockSource::new(vec![CaptureFrame {
        label: "a".to_string(),
        timestamp_unix_ms: 1,
        snapshot,
        raw_output: "one".to_string(),
        changed: true,
    }]);
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    assert!(app.should_render_terminal_cursor());

    app.scroll_main_screen_view(3).unwrap();
    assert!(!app.should_render_terminal_cursor());
}

#[test]
fn entering_app_input_mode_resets_live_viewport() {
    let mut snapshot = ScreenSnapshot::from_text_lines(20, 5, &["one"]);
    snapshot.set_screen_mode(false, 0);
    let source = MockSource::new(vec![CaptureFrame {
        label: "a".to_string(),
        timestamp_unix_ms: 1,
        snapshot,
        raw_output: "one".to_string(),
        changed: true,
    }]);
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    app.ui.watch_scroll = 7;
    app.ui.horizontal_scroll = 4;
    app.scroll_main_screen_view(3).unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();

    assert!(app.ui.app_input_mode);
    assert_eq!(app.ui.watch_scroll, 0);
    assert_eq!(app.ui.horizontal_scroll, 0);
    assert_eq!(app.live_scrollback_offset, 0);
    assert!(app.should_render_terminal_cursor());
}

#[test]
fn latest_screen_mode_transition_resets_watch_viewport() {
    let mut main = ScreenSnapshot::from_text_lines(20, 5, &["shell"]);
    main.set_screen_mode(false, 0);
    let mut alt = ScreenSnapshot::from_text_lines(20, 5, &["vim"]);
    alt.set_screen_mode(true, 0);

    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            CaptureFrame {
                label: "a".to_string(),
                timestamp_unix_ms: 1,
                snapshot: main,
                raw_output: "shell".to_string(),
                changed: true,
            },
            CaptureFrame {
                label: "b".to_string(),
                timestamp_unix_ms: 2,
                snapshot: alt,
                raw_output: "vim".to_string(),
                changed: true,
            },
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.ui.watch_scroll = 5;
    app.ui.horizontal_scroll = 8;

    app.capture(20, 5).unwrap();

    assert_eq!(app.ui.watch_scroll, 0);
    assert_eq!(app.ui.horizontal_scroll, 0);
}

#[test]
fn history_watch_pane_mouse_scroll_moves_snapshot_view() {
    let source = MockSource::new(vec![
        frame("a", &["one", "two", "three", "four", "five"]),
        frame("b", &["six", "seven", "eight", "nine", "ten"]),
    ]);
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.follow_latest = false;
    app.selected_index = 0;
    app.ui.focus = FocusPane::Watch;

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();

    assert_eq!(app.ui.watch_scroll, 3);

    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();

    assert_eq!(app.ui.watch_scroll, 0);
}

#[test]
fn mouse_input_is_not_recorded_when_child_does_not_accept_mouse() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::without_mouse_passthrough(vec![
            frame("a", &["one"]),
            frame("b", &["two"]),
            frame("c", &["three"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 3,
        modifiers: KeyModifiers::NONE,
    })
    .unwrap();
    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();

    let meta = app.history_metadata(1);
    assert_eq!(meta.input_event_count_since_prev, 0);
    assert!(meta.input_summary.is_empty());
}

#[test]
fn broken_sgr_mouse_tail_is_not_forwarded_as_key_input() {
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let key_events = source.key_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.ui.app_input_mode = true;
    app.last_mouse_input = Some(Instant::now());

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    for ch in ['[', '<', '3', '5', ';', '3', '3', ';', '8', 'M'] {
        app.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .unwrap();
    }

    let sent = key_events.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].code, KeyCode::Esc);
}

#[test]
fn broken_sgr_mouse_tail_with_shifted_characters_is_not_forwarded() {
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let key_events = source.key_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.ui.app_input_mode = true;
    app.last_mouse_input = Some(Instant::now());

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    app.handle_key_event(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE))
        .unwrap();
    app.handle_key_event(KeyEvent::new(KeyCode::Char('<'), KeyModifiers::SHIFT))
        .unwrap();
    for ch in ['3', '5', ';', '3', '3', ';', '8'] {
        app.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .unwrap();
    }
    app.handle_key_event(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT))
        .unwrap();

    let sent = key_events.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].code, KeyCode::Esc);
}

#[test]
fn broken_legacy_mouse_tail_is_not_forwarded_as_key_input() {
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let key_events = source.key_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.ui.app_input_mode = true;
    app.last_mouse_input = Some(Instant::now());

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    for ch in ['[', 'M', '#', '!', '$'] {
        app.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .unwrap();
    }

    let sent = key_events.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].code, KeyCode::Esc);
}

#[test]
fn lone_escape_after_mouse_is_forwarded_to_child() {
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let key_events = source.key_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.ui.app_input_mode = true;
    app.last_mouse_input = Some(Instant::now());

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    let sent = key_events.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].code, KeyCode::Esc);
}

#[test]
fn ordinary_text_still_passes_through_without_recent_mouse_input() {
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let key_events = source.key_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.ui.app_input_mode = true;

    for ch in ['<', '3', '5'] {
        app.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .unwrap();
    }

    let sent = key_events.lock().unwrap();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0].code, KeyCode::Char('<'));
    assert_eq!(sent[1].code, KeyCode::Char('3'));
    assert_eq!(sent[2].code, KeyCode::Char('5'));
}

#[test]
fn key_passthrough_is_blocked_when_not_following_latest() {
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let key_events = source.key_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.follow_latest = false;
    app.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();

    assert!(key_events.lock().unwrap().is_empty());
}

#[test]
fn non_latest_arrow_keys_move_history_even_when_watch_has_focus() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            frame("a", &["one"]),
            frame("b", &["two"]),
            frame("c", &["three"]),
            frame("d", &["four"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();

    app.follow_latest = false;
    app.ui.focus = FocusPane::Watch;
    app.ui.watch_scroll = 7;
    app.selected_index = app.filtered_indices()[1];

    app.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.ui.watch_scroll, 7);
    assert_eq!(app.selected_index, app.filtered_indices()[2]);
}

#[test]
fn key_passthrough_is_allowed_when_following_latest() {
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let key_events = source.key_events.clone();
    let mut app = App::new(&test_cli(), Box::new(source)).unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(key_events.lock().unwrap().len(), 1);
}

#[test]
fn custom_keymap_can_override_watch_passthrough() {
    let mut cli = test_cli();
    cli.keymap = vec!["down=history_pane_down".to_string()];
    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(vec![
            frame("a", &["one"]),
            frame("b", &["two"]),
            frame("c", &["three"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.ui.focus = FocusPane::Watch;

    app.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();

    assert!(!app.follow_latest);
    assert_eq!(app.selected_index, 1);
}

#[test]
fn child_bindings_remap_passthrough_keys_to_raw_bytes() {
    let mut cli = test_cli();
    cli.bind = vec!["j=down".to_string()];
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let byte_events = source.byte_events.clone();
    let key_events = source.key_events.clone();
    let mut app = App::new(&cli, Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    app.ui.app_input_mode = true;
    app.handle_key_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
        .unwrap();

    assert!(key_events.lock().unwrap().is_empty());
    assert_eq!(
        byte_events.lock().unwrap().as_slice(),
        &[b"\x1b[B".to_vec()]
    );
}

#[test]
fn custom_leave_app_input_mode_key_works_with_child_bindings() {
    let mut cli = test_cli();
    cli.keymap = vec!["ctrl-t=leave_app_input_mode".to_string()];
    cli.bind = vec!["ctrl-g=text:gg".to_string()];
    let source = MockSource::new(vec![frame("a", &["one"])]);
    let byte_events = source.byte_events.clone();
    let mut app = App::new(&cli, Box::new(source)).unwrap();

    app.capture(20, 5).unwrap();
    app.ui.app_input_mode = true;

    app.handle_key_event(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(byte_events.lock().unwrap().as_slice(), &[b"gg".to_vec()]);
    assert!(app.ui.app_input_mode);

    app.handle_key_event(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
        .unwrap();
    assert!(!app.ui.app_input_mode);
}

#[test]
fn app_input_mode_is_available_only_on_latest() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["one"])])),
    )
    .unwrap();

    app.follow_latest = false;
    app.handle_key_event(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();

    assert!(!app.ui.app_input_mode);
    assert_eq!(
        app.ui.status_message.as_deref(),
        Some("app input mode is available only on latest")
    );
}

#[test]
fn shift_p_toggles_child_process_pause() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::with_child_pause(vec![frame("a", &["one"])])),
    )
    .unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT))
        .unwrap();
    assert!(app.child_paused);
    assert!(
        app.ui
            .status_message
            .as_deref()
            .unwrap_or("")
            .contains("child process paused")
    );

    app.handle_key_event(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT))
        .unwrap();
    assert!(!app.child_paused);
}

#[test]
fn snapshot_trigger_saves_on_string_match() {
    let mut cli = test_cli();
    let dir = unique_temp_dir("twatch-trigger-string");
    cli.screenshot_dir = dir.to_string_lossy().into_owned();
    cli.snapshot_on = Some("panic".to_string());

    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(vec![frame("snap", &["panic: boom"])])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();

    let path = dir.join("twatch-auto-0001-snap.txt");
    assert!(path.exists());

    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn snapshot_trigger_saves_on_changed_cell_threshold() {
    let mut cli = test_cli();
    let dir = unique_temp_dir("twatch-trigger-cells");
    cli.screenshot_dir = dir.to_string_lossy().into_owned();
    cli.snapshot_on_change_cells = Some(2);

    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(vec![frame("snap", &["ab"])])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();

    let path = dir.join("twatch-auto-0001-snap.txt");
    assert!(path.exists());

    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn snapshot_trigger_once_only_saves_first_match() {
    let mut cli = test_cli();
    let dir = unique_temp_dir("twatch-trigger-once");
    cli.screenshot_dir = dir.to_string_lossy().into_owned();
    cli.snapshot_on = Some("panic".to_string());
    cli.snapshot_once = true;

    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(vec![
            frame("a", &["panic: one"]),
            frame("b", &["panic: two"]),
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();

    assert!(dir.join("twatch-auto-0001-a.txt").exists());
    assert!(!dir.join("twatch-auto-0002-b.txt").exists());

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn replay_mode_loads_existing_log() {
    let mut cli = test_cli();
    let dir = unique_temp_dir("twatch-replay");
    let path = dir.join("trace.jsonl");
    fs::create_dir_all(&dir).unwrap();
    cli.replay = Some(path.to_string_lossy().into_owned());

    append_record(
        cli.replay.as_deref().unwrap(),
        &LogRecord {
            label: "a".to_string(),
            changed: true,
            timestamp_unix_ms: 1,
            frame_seq: 1,
            width: 20,
            height: 5,
            changed_cell_count: 3,
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
        cli.replay.as_deref().unwrap(),
        &LogRecord {
            label: "b".to_string(),
            changed: true,
            timestamp_unix_ms: 2,
            frame_seq: 2,
            width: 20,
            height: 5,
            changed_cell_count: 2,
            input_event_count_since_prev: 1,
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

    let app = App::new(
        &cli,
        Box::new(MockSource::new(vec![frame("unused", &["x"])])),
    )
    .unwrap();

    assert_eq!(app.history_len(), 1);
    assert_eq!(app.current_label(), Some("b"));
    assert_eq!(
        app.command_display(),
        &format!("replay: {}", path.to_string_lossy())
    );
    assert_eq!(app.selected_lines()[0], "two".to_string());

    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn save_snapshot_uses_configured_directory_and_format() {
    let mut cli = test_cli();
    let dir = unique_temp_dir("twatch-shot-test");
    cli.screenshot_dir = dir.to_string_lossy().into_owned();
    cli.screenshot_format = ScreenshotFormatArg::Svg;

    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(vec![frame("snap", &["hello"])])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.save_snapshot().unwrap();

    let path = dir.join("twatch-snap.svg");
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.starts_with("<svg"));
    assert!(
        app.ui
            .status_message
            .as_deref()
            .unwrap_or("")
            .contains(".svg")
    );

    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn cycle_screenshot_format_toggles_and_updates_status() {
    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![frame("a", &["x"])])),
    )
    .unwrap();

    app.cycle_screenshot_format();
    assert_eq!(app.screenshot_format.label(), "svg");
    assert!(
        app.ui
            .status_message
            .as_deref()
            .unwrap_or("")
            .contains("snapshot format: svg")
    );

    app.cycle_screenshot_format();
    assert_eq!(app.screenshot_format.label(), "text");
}

#[test]
fn history_trimming_is_batched_after_limit_boundary() {
    let mut cli = test_cli();
    cli.limit = 5;
    cli.checkpoint_interval = 2;
    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(
            (0..12).map(|i| frame(&format!("{i:04}"), &["x"])).collect(),
        )),
    )
    .unwrap();

    for _ in 0..7 {
        app.capture(20, 5).unwrap();
    }
    assert_eq!(app.history_len(), 6);
    assert_eq!(app.visible_history_len(), 5);
    assert_eq!(app.filtered_indices().len(), 5);
    assert_eq!(app.filtered_indices()[0], 5);

    for _ in 7..12 {
        app.capture(20, 5).unwrap();
    }
    assert_eq!(app.history_len(), 5);
}

#[test]
fn selecting_oldest_visible_history_advances_to_next_oldest_when_window_shifts() {
    let mut cli = test_cli();
    cli.limit = 5;
    cli.checkpoint_interval = 2;
    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(
            (0..8).map(|i| frame(&format!("{i:04}"), &["x"])).collect(),
        )),
    )
    .unwrap();

    for _ in 0..6 {
        app.capture(20, 5).unwrap();
    }

    app.follow_latest = false;
    app.selected_index = app.filtered_indices().last().copied().unwrap();
    let previously_selected = app.selected_index;

    app.capture(20, 5).unwrap();

    assert!(!app.follow_latest);
    assert_eq!(previously_selected + 1, app.selected_index);
    assert_eq!(
        app.filtered_indices().last().copied().unwrap(),
        app.selected_index
    );
}

#[test]
fn trimming_keeps_latest_selected_when_following_latest() {
    let mut cli = test_cli();
    cli.limit = 5;
    cli.checkpoint_interval = 2;
    let mut app = App::new(
        &cli,
        Box::new(MockSource::new(
            (0..12).map(|i| frame(&format!("{i:04}"), &["x"])).collect(),
        )),
    )
    .unwrap();

    for _ in 0..12 {
        app.capture(20, 5).unwrap();
    }

    assert!(app.follow_latest);
    assert_eq!(app.selected_history_row(), 0);
    assert_eq!(app.current_label(), Some("0011"));
}

#[test]
fn history_snapshots_preserve_cursor_state() {
    let mut first = ScreenSnapshot::from_text_lines(20, 5, &["one"]);
    first.set_cursor_state(3, 1, true);
    let mut second = ScreenSnapshot::from_text_lines(20, 5, &["two"]);
    second.set_cursor_state(5, 2, true);

    let mut app = App::new(
        &test_cli(),
        Box::new(MockSource::new(vec![
            CaptureFrame {
                label: "a".to_string(),
                timestamp_unix_ms: 1,
                snapshot: first,
                raw_output: "one".to_string(),
                changed: true,
            },
            CaptureFrame {
                label: "b".to_string(),
                timestamp_unix_ms: 2,
                snapshot: second,
                raw_output: "two".to_string(),
                changed: true,
            },
        ])),
    )
    .unwrap();

    app.capture(20, 5).unwrap();
    app.capture(20, 5).unwrap();
    app.follow_latest = false;
    app.selected_index = 0;

    let snapshot = app.selected_snapshot().unwrap();
    assert_eq!(snapshot.cursor_position(), (3, 1));
    assert!(snapshot.cursor_visible());
}

fn test_cli() -> Cli {
    Cli {
        batch: false,
        batch_count: None,
        batch_size: None,
        batch_crop: None,
        batch_diff_only: false,
        batch_no_color: false,
        aftercommand: None,
        keymap: Vec::new(),
        bind: Vec::new(),
        aftercommand_regex: None,
        aftercommand_change_cells: None,
        aftercommand_every: None,
        aftercommand_debounce_ms: None,
        aftercommand_timeout_ms: 3000,
        compress: false,
        logfile: None,
        replay: None,
        screenshot_dir: "/tmp".to_string(),
        screenshot_format: ScreenshotFormatArg::Text,
        snapshot_on: None,
        snapshot_on_regex: None,
        snapshot_on_change_cells: None,
        snapshot_once: false,
        shell: "sh -c".to_string(),
        differences: DiffModeArg::None,
        limit: 500,
        checkpoint_interval: 12,
        debug: false,
        command: vec!["mock".to_string()],
    }
}

fn frame(label: &str, lines: &[&str]) -> CaptureFrame {
    CaptureFrame {
        label: label.to_string(),
        timestamp_unix_ms: 1,
        snapshot: ScreenSnapshot::from_text_lines(20, 5, lines),
        raw_output: lines.join("\n"),
        changed: true,
    }
}

fn unchanged_frame(label: &str, lines: &[&str]) -> CaptureFrame {
    CaptureFrame {
        label: label.to_string(),
        timestamp_unix_ms: 2,
        snapshot: ScreenSnapshot::from_text_lines(20, 5, lines),
        raw_output: lines.join("\n"),
        changed: false,
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{id}"))
}
