use std::path::PathBuf;
use std::time::Instant;

use crate::cli::{DiffModeArg, ScreenshotFormatArg};
use crate::history::HistoryStore;
use crate::runner::FrameSource;
use crate::screen::ScreenSnapshot;
use crate::screenshot::ScreenshotFormat;

mod input;
mod runtime;
mod state;
#[cfg(test)]
mod tests;

enum AppEvent {
    Terminal(crossterm::event::Event),
    SourceUpdated,
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
    pub changed: bool,
}

pub struct App {
    pub interval_secs: f64,
    pub paused: bool,
    pub show_history: bool,
    pub show_help: bool,
    pub show_exit_confirm: bool,
    pub diff_only: bool,
    pub diff_mode: DiffMode,
    pub focus: FocusPane,
    pub app_input_mode: bool,
    pub watch_scroll: usize,
    pub horizontal_scroll: usize,
    pub filter_query: String,
    pub status_message: Option<String>,
    pub selected_index: usize,
    pub follow_latest: bool,
    input_mode: InputMode,
    filter_mode: FilterMode,
    current_snapshot: Option<ScreenSnapshot>,
    current_label: Option<String>,
    current_changed: bool,
    history: HistoryStore,
    metadata: Vec<AppHistoryMetadata>,
    filtered: Vec<usize>,
    limit: usize,
    checkpoint_interval: usize,
    compress: bool,
    logfile: Option<String>,
    screenshot_dir: PathBuf,
    screenshot_format: ScreenshotFormat,
    command_display: String,
    source: Box<dyn FrameSource>,
    last_tick: Instant,
    last_mouse_input: Option<Instant>,
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
