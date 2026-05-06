use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::DefaultTerminal;

use crate::cli::{Cli, DiffModeArg};
use crate::history::{HistoryMetadata, HistoryStore};
use crate::logging::{LogRecord, append_record, load_records};
use crate::runner::{CaptureFrame, FrameSource};
use crate::screen::ScreenSnapshot;
use crate::ui;

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
    pub diff_only: bool,
    pub diff_mode: DiffMode,
    pub focus: FocusPane,
    pub app_input_mode: bool,
    pub watch_scroll: usize,
    pub horizontal_scroll: usize,
    pub filter_query: String,
    pub selected_index: usize,
    pub follow_latest: bool,
    input_mode: InputMode,
    history: HistoryStore,
    metadata: Vec<AppHistoryMetadata>,
    filtered: Vec<usize>,
    limit: usize,
    checkpoint_interval: usize,
    compress: bool,
    logfile: Option<String>,
    source: Box<dyn FrameSource>,
    last_tick: Instant,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum InputMode {
    Normal,
    Search,
}

impl App {
    pub fn new(cli: &Cli, source: Box<dyn FrameSource>) -> Result<Self> {
        let mut app = Self {
            interval_secs: cli.interval,
            paused: false,
            show_history: false,
            show_help: false,
            diff_only: false,
            diff_mode: cli.differences.into(),
            focus: FocusPane::Watch,
            app_input_mode: false,
            watch_scroll: 0,
            horizontal_scroll: 0,
            filter_query: String::new(),
            selected_index: 0,
            follow_latest: true,
            input_mode: InputMode::Normal,
            history: HistoryStore::new(cli.checkpoint_interval, cli.compress),
            metadata: Vec::new(),
            filtered: Vec::new(),
            limit: cli.limit.max(1),
            checkpoint_interval: cli.checkpoint_interval.max(1),
            compress: cli.compress,
            logfile: cli.logfile.clone(),
            source,
            last_tick: Instant::now(),
        };

        app.load_history_from_log()?;
        Ok(app)
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let size = terminal.size()?;
        self.capture(size.width, size.height.saturating_sub(2))?;

        loop {
            terminal.draw(|frame| ui::draw(frame, &self))?;

            let timeout = self.event_poll_timeout();
            if event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) => {
                        if self.handle_key_event(key)? {
                            break;
                        }
                    }
                    Event::Mouse(mouse) => self.handle_mouse(mouse)?,
                    Event::Resize(width, height) => {
                        self.source.resize(width, height.saturating_sub(2))?;
                        if !self.paused {
                            self.capture(width, height.saturating_sub(2))?;
                        }
                    }
                    _ => {}
                }
            } else if !self.paused && self.should_capture_now() {
                let size = terminal.size()?;
                self.capture(size.width, size.height.saturating_sub(2))?;
            }
        }

        self.source.terminate().ok();
        Ok(())
    }

    pub fn history_len(&self) -> usize {
        self.metadata.len()
    }

    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered
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
        self.history.snapshot(self.selected_index).ok().flatten()
    }

    pub fn previous_snapshot(&self) -> Option<ScreenSnapshot> {
        if self.selected_index == 0 {
            None
        } else {
            self.history.snapshot(self.selected_index - 1).ok().flatten()
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

        self.history.push(
            snapshot,
            HistoryMetadata {
                label: label.clone(),
            },
        )?;
        self.metadata.push(AppHistoryMetadata { label, changed });
        if self.metadata.len() > self.limit {
            self.trim_history()?;
        }
        if raw_output.is_empty() && self.metadata.len() == 1 {
            self.metadata[0].changed = false;
        } else if let Some(last) = self.metadata.last_mut() {
            last.changed = changed;
        }
        if self.follow_latest {
            self.selected_index = self.history.len().saturating_sub(1);
        }
        self.rebuild_filter()?;
        self.append_log_record(changed)?;
        self.last_tick = Instant::now();
        Ok(())
    }

    fn tick_timeout(&self) -> Duration {
        let interval = Duration::from_secs_f64(self.interval_secs.max(0.2));
        interval.saturating_sub(self.last_tick.elapsed())
    }

    fn event_poll_timeout(&self) -> Duration {
        if self.source.is_event_driven() {
            Duration::from_millis(16)
        } else {
            self.tick_timeout()
        }
    }

    fn should_capture_now(&self) -> bool {
        if self.source.is_event_driven() {
            self.source.has_pending_update()
        } else {
            self.last_tick.elapsed() >= Duration::from_secs_f64(self.interval_secs.max(0.2))
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<bool> {
        if self.show_help {
            match key.code {
                KeyCode::Char('h') | KeyCode::Esc => {
                    self.show_help = false;
                }
                KeyCode::Char('q') => return Ok(true),
                _ => {}
            }
            return Ok(false);
        }

        if self.input_mode == InputMode::Search {
            return self.handle_search_key(key);
        }

        if self.app_input_mode {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g') {
                self.app_input_mode = false;
                return Ok(false);
            }
            self.source.send_key(key)?;
            return Ok(false);
        }

        if self.focus == FocusPane::Watch && self.should_passthrough_to_app(key) {
            self.source.send_key(key)?;
            return Ok(false);
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) => return Ok(true),
            (KeyCode::Char('h'), _) => self.show_help = true,
            (KeyCode::Char('i'), _) => {
                if self.focus == FocusPane::Watch {
                    self.app_input_mode = true;
                }
            }
            (KeyCode::Tab, _) => self.toggle_focus(),
            (KeyCode::Left, KeyModifiers::ALT) => {
                self.horizontal_scroll = self.horizontal_scroll.saturating_sub(4)
            }
            (KeyCode::Right, KeyModifiers::ALT) => self.horizontal_scroll += 4,
            (KeyCode::Left, _) => self.focus = FocusPane::Watch,
            (KeyCode::Right, _) => self.focus = FocusPane::History,
            (KeyCode::Backspace, _) => {
                self.show_history = !self.show_history;
                if self.show_history {
                    self.focus = FocusPane::History;
                } else if self.focus == FocusPane::History {
                    self.focus = FocusPane::Watch;
                }
            }
            (KeyCode::Char('/'), _) => self.input_mode = InputMode::Search,
            (KeyCode::Esc, _) => {
                self.filter_query.clear();
                self.rebuild_filter();
            }
            (KeyCode::Char('d'), _) => self.cycle_diff_mode(),
            (KeyCode::Char('0'), _) => self.diff_mode = DiffMode::None,
            (KeyCode::Char('1'), _) => self.diff_mode = DiffMode::Watch,
            (KeyCode::Char('p'), _) => self.paused = !self.paused,
            (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Down, _) => self.move_down(),
            (KeyCode::PageUp, _) => self.page_up(),
            (KeyCode::PageDown, _) => self.page_down(),
            (KeyCode::Home, _) => self.move_top(),
            (KeyCode::End, _) => self.move_end(),
            _ => {}
        }
        Ok(false)
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.filter_query.clear();
                self.rebuild_filter();
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.filter_query.pop();
                self.rebuild_filter();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_query.push(ch);
                self.rebuild_filter();
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        if self.app_input_mode {
            self.source.send_mouse(mouse, 2)?;
            return Ok(());
        }

        match mouse.kind {
            MouseEventKind::ScrollDown => self.move_down(),
            MouseEventKind::ScrollUp => self.move_up(),
            MouseEventKind::Down(_) => {
                if mouse.row < 2 {
                    return Ok(());
                }
                let total_width = crossterm::terminal::size()
                    .map(|(width, _)| width)
                    .unwrap_or(0);
                if self.show_history && mouse.column >= self.history_overlay_start(total_width) {
                    self.focus = FocusPane::History;
                    self.select_history_row(usize::from(mouse.row.saturating_sub(2)));
                } else {
                    self.focus = FocusPane::Watch;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn history_overlay_start(&self, total_width: u16) -> u16 {
        let overlay_width = if self.show_history { 30 } else { 2 };
        total_width.saturating_sub(overlay_width)
    }

    fn rebuild_filter(&mut self) -> Result<()> {
        self.filtered = self.history.find_by_query(&self.filter_query)?;
        self.filtered.reverse();
        if self.filtered.is_empty() && !self.history.is_empty() {
            self.selected_index = self.history.len() - 1;
        } else if !self.filtered.contains(&self.selected_index) {
            self.selected_index = *self.filtered.first().unwrap_or(&0);
        }
        Ok(())
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
            self.selected_index = 0;
        } else if self.selected_index < start {
            self.selected_index = 0;
        } else {
            self.selected_index -= start;
        }
        Ok(())
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Watch => FocusPane::History,
            FocusPane::History => FocusPane::Watch,
        };
    }

    fn cycle_diff_mode(&mut self) {
        self.diff_mode = match self.diff_mode {
            DiffMode::None => DiffMode::Watch,
            DiffMode::Watch => DiffMode::None,
        };
    }

    fn move_up(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = self.watch_scroll.saturating_sub(1),
            FocusPane::History => {
                if self.follow_latest {
                    return;
                }

                match self.selected_filtered_position() {
                    Some(0) | None => {
                        self.follow_latest = true;
                        if let Some(latest) = self.filtered.first().copied() {
                            self.selected_index = latest;
                        }
                    }
                    Some(position) => {
                        let next = position.saturating_sub(1);
                        self.selected_index = self.filtered[next];
                        self.sync_follow_latest_with_selection();
                    }
                }
            }
        }
    }

    fn move_down(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll += 1,
            FocusPane::History => {
                if self.filtered.is_empty() {
                    return;
                }

                if self.follow_latest {
                    self.follow_latest = false;
                    self.selected_index = self.filtered[0];
                    return;
                }

                if let Some(position) = self.selected_filtered_position() {
                    let next = (position + 1).min(self.filtered.len().saturating_sub(1));
                    self.selected_index = self.filtered[next];
                    self.sync_follow_latest_with_selection();
                }
            }
        }
    }

    fn page_up(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = self.watch_scroll.saturating_sub(10),
            FocusPane::History => {
                if self.follow_latest {
                    return;
                }

                if let Some(position) = self.selected_filtered_position() {
                    if position <= 10 {
                        self.follow_latest = true;
                        if let Some(latest) = self.filtered.first().copied() {
                            self.selected_index = latest;
                        }
                    } else {
                        let next = position.saturating_sub(10);
                        self.selected_index = self.filtered[next];
                        self.sync_follow_latest_with_selection();
                    }
                }
            }
        }
    }

    fn page_down(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll += 10,
            FocusPane::History => {
                if self.filtered.is_empty() {
                    return;
                }

                if self.follow_latest {
                    self.follow_latest = false;
                    self.selected_index =
                        self.filtered[(10usize).min(self.filtered.len().saturating_sub(1))];
                    return;
                }

                if let Some(position) = self.selected_filtered_position() {
                    let next = (position + 10).min(self.filtered.len().saturating_sub(1));
                    self.selected_index = self.filtered[next];
                    self.sync_follow_latest_with_selection();
                }
            }
        }
    }

    fn move_top(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = 0,
            FocusPane::History => {
                self.follow_latest = true;
                if let Some(first) = self.filtered.first().copied() {
                    self.selected_index = first;
                }
            }
        }
    }

    fn move_end(&mut self) {
        match self.focus {
            FocusPane::Watch => self.watch_scroll = usize::MAX / 2,
            FocusPane::History => {
                if let Some(last) = self.filtered.last().copied() {
                    self.selected_index = last;
                    self.follow_latest = false;
                    self.sync_follow_latest_with_selection();
                }
            }
        }
    }

    fn should_passthrough_to_app(&self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT)
        {
            return false;
        }

        matches!(
            key.code,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::PageUp
                | KeyCode::PageDown
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Enter
                | KeyCode::F(_)
        )
    }

    fn sync_follow_latest_with_selection(&mut self) {
        self.follow_latest = self
            .filtered
            .first()
            .is_some_and(|latest| *latest == self.selected_index);
    }

    fn select_history_row(&mut self, row: usize) {
        if row == 0 {
            self.follow_latest = true;
            if let Some(latest) = self.filtered.first().copied() {
                self.selected_index = latest;
            }
            return;
        }

        if let Some(index) = self.filtered.get(row - 1).copied() {
            self.follow_latest = false;
            self.selected_index = index;
            self.sync_follow_latest_with_selection();
        }
    }

    fn load_history_from_log(&mut self) -> Result<()> {
        let Some(path) = &self.logfile else {
            return Ok(());
        };

        for record in load_records(path)? {
            let (snapshot, metadata, changed) = record.into_parts();
            self.history.push(snapshot, metadata.clone())?;
            self.metadata.push(AppHistoryMetadata {
                label: metadata.label,
                changed,
            });
        }

        if !self.history.is_empty() {
            self.selected_index = self.history.len() - 1;
            self.follow_latest = true;
            self.rebuild_filter()?;
        }

        Ok(())
    }

    fn append_log_record(&self, changed: bool) -> Result<()> {
        let Some(path) = &self.logfile else {
            return Ok(());
        };
        let Some(snapshot) = self.selected_snapshot() else {
            return Ok(());
        };
        let Some(meta) = self.metadata.get(self.selected_index) else {
            return Ok(());
        };

        append_record(
            path,
            &LogRecord {
                label: meta.label.clone(),
                changed,
                snapshot,
            },
        )
    }
}
