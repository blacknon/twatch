use super::{App, FilterMode, FocusPane};
use crate::cli::{Cli, DiffModeArg, ScreenshotFormatArg};
use crate::logging::{LogRecord, append_record};
use crate::runner::{CaptureFrame, FrameSource, SourceEvent};
use crate::screen::ScreenSnapshot;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::event::{KeyEvent, MouseEvent};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct MockSource {
    frames: Vec<CaptureFrame>,
    next: usize,
}

impl MockSource {
    fn new(frames: Vec<CaptureFrame>) -> Self {
        Self { frames, next: 0 }
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
    app.filter_query = "worker-01".to_string();
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
    app.filter_query = "worker-01".to_string();
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
    app.filter_query = "worker-01".to_string();
    app.rebuild_filter().unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(app.filter_query, "worker-01");
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
    app.filter_query = "worker-01".to_string();
    app.rebuild_filter().unwrap();

    app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(app.filter_query.is_empty());
    assert!(app.follow_latest);
    assert!(!app.show_exit_confirm);
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

    assert!(app.show_exit_confirm);
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
    app.focus = FocusPane::History;
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
    app.focus = FocusPane::History;
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
    assert!(app.show_inspector);

    app.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT))
        .unwrap();

    assert_eq!(app.inspect_cursor(), (1, 0));
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
    assert!(app.status_message.as_deref().unwrap_or("").contains(".svg"));

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
        app.status_message
            .as_deref()
            .unwrap_or("")
            .contains("snapshot format: svg")
    );

    app.cycle_screenshot_format();
    assert_eq!(app.screenshot_format.label(), "text");
}

fn test_cli() -> Cli {
    Cli {
        interval: 2.0,
        batch: false,
        batch_count: None,
        batch_size: None,
        batch_crop: None,
        batch_diff_only: false,
        batch_no_color: false,
        aftercommand: None,
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
