use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use crate::aftercommand::AfterCommandRuntime;
use crate::cli::{DiffModeArg, ScreenshotFormatArg};
use crate::history::HistoryMetadata;
use crate::history::HistoryStore;
use crate::runner::FrameSource;
use crate::screen::ScreenSnapshot;
use crate::screenshot::ScreenshotFormat;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use regex::Regex;

mod config;
mod filter;
mod history_ops;
mod input;
mod runtime;
mod snapshot;
mod state;
#[cfg(test)]
mod tests;
mod trigger;
mod view;

enum AppEvent {
    Terminal(crossterm::event::Event),
    SourceUpdated,
    SourceClosed(Option<String>),
}

enum LoopControl {
    Continue(bool),
    Break,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FocusPane {
    Watch,
    History,
}

impl FocusPane {
    pub fn label(self) -> &'static str {
        match self {
            Self::Watch => "watch",
            Self::History => "history",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DiffMode {
    None,
    Watch,
}

impl DiffMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Watch => "watch",
        }
    }
}

impl From<DiffModeArg> for DiffMode {
    fn from(value: DiffModeArg) -> Self {
        match value {
            DiffModeArg::None => Self::None,
            DiffModeArg::Watch => Self::Watch,
            DiffModeArg::List | DiffModeArg::Word => Self::None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppHistoryMetadata {
    pub label: String,
    pub timestamp_unix_ms: u64,
    pub frame_seq: u64,
    pub changed: bool,
    pub width: u16,
    pub height: u16,
    pub changed_cell_count: usize,
    pub input_event_count_since_prev: usize,
    pub resized: bool,
    pub input_summary: String,
    pub resize_from_width: u16,
    pub resize_from_height: u16,
    pub resize_to_width: u16,
    pub resize_to_height: u16,
    pub resize_source: String,
}

impl AppHistoryMetadata {
    fn from_history_metadata(value: HistoryMetadata) -> Self {
        Self {
            label: value.label,
            timestamp_unix_ms: value.timestamp_unix_ms,
            frame_seq: value.frame_seq,
            changed: value.changed,
            width: value.width,
            height: value.height,
            changed_cell_count: value.changed_cell_count,
            input_event_count_since_prev: value.input_event_count_since_prev,
            resized: value.resized,
            input_summary: String::new(),
            resize_from_width: value.resize_from_width,
            resize_from_height: value.resize_from_height,
            resize_to_width: value.resize_to_width,
            resize_to_height: value.resize_to_height,
            resize_source: value.resize_source,
        }
    }

    fn to_history_metadata(&self) -> HistoryMetadata {
        HistoryMetadata {
            label: self.label.clone(),
            timestamp_unix_ms: self.timestamp_unix_ms,
            frame_seq: self.frame_seq,
            changed: self.changed,
            width: self.width,
            height: self.height,
            changed_cell_count: self.changed_cell_count,
            input_event_count_since_prev: self.input_event_count_since_prev,
            resized: self.resized,
            resize_from_width: self.resize_from_width,
            resize_from_height: self.resize_from_height,
            resize_to_width: self.resize_to_width,
            resize_to_height: self.resize_to_height,
            resize_source: self.resize_source.clone(),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct InputTraceEvent {
    seq: u64,
    timestamp_unix_ms: u64,
    kind: InputTraceKind,
    target_focus: InputTargetFocus,
}

#[derive(Clone, Debug)]
enum InputTraceKind {
    Key {
        code: KeyCode,
        modifiers: KeyModifiers,
    },
    Mouse {
        kind: MouseEventKind,
        column: u16,
        row: u16,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputTargetFocus {
    Child,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct ResizeTraceEvent {
    timestamp_unix_ms: u64,
    old_width: u16,
    old_height: u16,
    new_width: u16,
    new_height: u16,
    source: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ViewCacheKey {
    follow_latest: bool,
    selected_index: usize,
    history_len: usize,
    current_frame_seq: u64,
}

#[derive(Clone, Debug)]
struct ViewCache {
    key: ViewCacheKey,
    selected: Option<ScreenSnapshot>,
    previous: Option<ScreenSnapshot>,
}

struct TraceState {
    input_trace: VecDeque<InputTraceEvent>,
    pending_input_events: Vec<InputTraceEvent>,
    next_input_seq: u64,
    resize_trace: VecDeque<ResizeTraceEvent>,
    pending_resize_event: Option<ResizeTraceEvent>,
}

impl TraceState {
    fn new() -> Self {
        Self {
            input_trace: VecDeque::with_capacity(256),
            pending_input_events: Vec::new(),
            next_input_seq: 1,
            resize_trace: VecDeque::with_capacity(64),
            pending_resize_event: None,
        }
    }
}

struct ViewState {
    cache: RefCell<Option<ViewCache>>,
}

impl ViewState {
    fn new() -> Self {
        Self {
            cache: RefCell::new(None),
        }
    }
}

pub(crate) struct UiState {
    pub(crate) show_history: bool,
    pub(crate) show_history_details: bool,
    pub(crate) show_help: bool,
    pub(crate) show_exit_confirm: bool,
    pub(crate) show_inspector: bool,
    pub(crate) focus: FocusPane,
    pub(crate) app_input_mode: bool,
    pub(crate) watch_scroll: usize,
    pub(crate) horizontal_scroll: usize,
    pub(crate) inspect_x: u16,
    pub(crate) inspect_y: u16,
    pub(crate) filter_query: String,
    pub(crate) status_message: Option<String>,
}

impl UiState {
    fn new() -> Self {
        Self {
            show_history: false,
            show_history_details: false,
            show_help: false,
            show_exit_confirm: false,
            show_inspector: false,
            focus: FocusPane::Watch,
            app_input_mode: false,
            watch_scroll: 0,
            horizontal_scroll: 0,
            inspect_x: 0,
            inspect_y: 0,
            filter_query: String::new(),
            status_message: None,
        }
    }
}

impl InputTraceEvent {
    fn summary(&self) -> String {
        match &self.kind {
            InputTraceKind::Key { code, modifiers } => {
                let prefix = format_modifiers(*modifiers);
                format!("{prefix}{}", format_key_code(code))
            }
            InputTraceKind::Mouse { kind, column, row } => {
                format!("{}@{},{}", format_mouse_kind(kind), column, row)
            }
        }
    }
}

pub struct App {
    pub interval_secs: f64,
    pub paused: bool,
    pub child_paused: bool,
    pub child_pause_supported: bool,
    pub diff_only: bool,
    pub diff_mode: DiffMode,
    pub selected_index: usize,
    pub follow_latest: bool,
    input_mode: InputMode,
    filter_mode: FilterMode,
    current_snapshot: Option<ScreenSnapshot>,
    current_metadata: Option<AppHistoryMetadata>,
    history: HistoryStore,
    metadata: Vec<AppHistoryMetadata>,
    filtered: Vec<usize>,
    limit: usize,
    checkpoint_interval: usize,
    compress: bool,
    logfile: Option<String>,
    replay_mode: bool,
    screenshot_dir: PathBuf,
    screenshot_format: ScreenshotFormat,
    snapshot_on: Option<String>,
    snapshot_on_regex: Option<Regex>,
    snapshot_on_change_cells: Option<usize>,
    snapshot_once: bool,
    snapshot_trigger_fired: bool,
    aftercommand_runtime: Option<AfterCommandRuntime>,
    command_display: String,
    source: Box<dyn FrameSource>,
    last_tick: Instant,
    last_mouse_input: Option<Instant>,
    last_mouse_scroll_input: Option<Instant>,
    next_frame_seq: u64,
    trace: TraceState,
    view: ViewState,
    pub(crate) ui: UiState,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum InputMode {
    Normal,
    Search,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FilterMode {
    Plain,
    Regex,
}

impl From<ScreenshotFormatArg> for ScreenshotFormat {
    fn from(value: ScreenshotFormatArg) -> Self {
        match value {
            ScreenshotFormatArg::Text => Self::Text,
            ScreenshotFormatArg::Svg => Self::Svg,
        }
    }
}

fn format_modifiers(modifiers: KeyModifiers) -> String {
    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt");
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift");
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{}+", parts.join("+"))
    }
}

fn format_key_code(code: &KeyCode) -> String {
    match code {
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::F(value) => format!("F{value}"),
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::Null => "Null".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        _ => "Key".to_string(),
    }
}

fn format_mouse_kind(kind: &MouseEventKind) -> String {
    match kind {
        MouseEventKind::Down(button) => format!("Down({})", format_mouse_button(*button)),
        MouseEventKind::Up(button) => format!("Up({})", format_mouse_button(*button)),
        MouseEventKind::Drag(button) => format!("Drag({})", format_mouse_button(*button)),
        MouseEventKind::Moved => "Moved".to_string(),
        MouseEventKind::ScrollDown => "ScrollDown".to_string(),
        MouseEventKind::ScrollUp => "ScrollUp".to_string(),
        MouseEventKind::ScrollLeft => "ScrollLeft".to_string(),
        MouseEventKind::ScrollRight => "ScrollRight".to_string(),
    }
}

fn format_mouse_button(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "Left",
        MouseButton::Right => "Right",
        MouseButton::Middle => "Middle",
    }
}
