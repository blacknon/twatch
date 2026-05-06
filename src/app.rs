use std::sync::mpsc::{self, RecvTimeoutError};
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

enum AppEvent {
    Terminal(Event),
    SourceUpdated,
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
    pub selected_index: usize,
    pub follow_latest: bool,
    input_mode: InputMode,
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
            selected_index: 0,
            follow_latest: true,
            input_mode: InputMode::Normal,
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
            command_display: if cli.demo {
                "demo".to_string()
            } else if cli.command.is_empty() {
                "<interactive>".to_string()
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

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let input_tx = tx.clone();
        std::thread::spawn(move || {
            while let Ok(event) = event::read() {
                if input_tx.send(AppEvent::Terminal(event)).is_err() {
                    break;
                }
            }
        });

        if let Some(update_rx) = self.source.take_update_receiver() {
            let update_tx = tx.clone();
            std::thread::spawn(move || {
                while update_rx.recv().is_ok() {
                    if update_tx.send(AppEvent::SourceUpdated).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let size = terminal.size()?;
        if let Err(err) = self.capture(size.width, size.height.saturating_sub(2)) {
            if self.is_source_closed_error(&err) {
                self.source.terminate().ok();
                return Ok(());
            }
            return Err(err);
        }
        let mut needs_redraw = true;

        loop {
            if needs_redraw {
                terminal.draw(|frame| ui::draw(frame, &self))?;
                needs_redraw = false;
            }

            if self.source.is_event_driven() {
                match rx.recv() {
                    Ok(AppEvent::Terminal(event)) => match event {
                        Event::Key(key) => {
                            let should_quit = match self.handle_key_event(key) {
                                Ok(value) => value,
                                Err(err) if self.is_source_closed_error(&err) => break,
                                Err(err) => return Err(err),
                            };
                            if should_quit {
                                break;
                            }
                            if !self.paused && self.source.has_pending_update() {
                                let size = terminal.size()?;
                                if let Err(err) =
                                    self.capture(size.width, size.height.saturating_sub(2))
                                {
                                    if self.is_source_closed_error(&err) {
                                        break;
                                    }
                                    return Err(err);
                                }
                            }
                            needs_redraw = true;
                        }
                        Event::Mouse(mouse) => {
                            let redraw = match self.handle_mouse(mouse) {
                                Ok(value) => value,
                                Err(err) if self.is_source_closed_error(&err) => break,
                                Err(err) => return Err(err),
                            };
                            needs_redraw = redraw || needs_redraw;
                        }
                        Event::Resize(width, height) => {
                            if let Err(err) = self.source.resize(width, height.saturating_sub(2)) {
                                if self.is_source_closed_error(&err) {
                                    break;
                                }
                                return Err(err);
                            }
                            if !self.paused {
                                if let Err(err) = self.capture(width, height.saturating_sub(2)) {
                                    if self.is_source_closed_error(&err) {
                                        break;
                                    }
                                    return Err(err);
                                }
                            }
                            needs_redraw = true;
                        }
                        _ => {}
                    },
                    Ok(AppEvent::SourceUpdated) => {
                        if !self.paused {
                            let size = terminal.size()?;
                            if let Err(err) =
                                self.capture(size.width, size.height.saturating_sub(2))
                            {
                                if self.is_source_closed_error(&err) {
                                    break;
                                }
                                return Err(err);
                            }
                            needs_redraw = true;
                        }
                    }
                    Err(_) => break,
                }
            } else {
                match rx.recv_timeout(self.tick_timeout()) {
                    Ok(AppEvent::Terminal(event)) => match event {
                        Event::Key(key) => {
                            let should_quit = match self.handle_key_event(key) {
                                Ok(value) => value,
                                Err(err) if self.is_source_closed_error(&err) => break,
                                Err(err) => return Err(err),
                            };
                            if should_quit {
                                break;
                            }
                            needs_redraw = true;
                        }
                        Event::Mouse(mouse) => {
                            let redraw = match self.handle_mouse(mouse) {
                                Ok(value) => value,
                                Err(err) if self.is_source_closed_error(&err) => break,
                                Err(err) => return Err(err),
                            };
                            needs_redraw = redraw || needs_redraw;
                        }
                        Event::Resize(width, height) => {
                            if let Err(err) = self.source.resize(width, height.saturating_sub(2)) {
                                if self.is_source_closed_error(&err) {
                                    break;
                                }
                                return Err(err);
                            }
                            if !self.paused {
                                if let Err(err) = self.capture(width, height.saturating_sub(2)) {
                                    if self.is_source_closed_error(&err) {
                                        break;
                                    }
                                    return Err(err);
                                }
                            }
                            needs_redraw = true;
                        }
                        _ => {}
                    },
                    Ok(AppEvent::SourceUpdated) => {}
                    Err(RecvTimeoutError::Timeout) => {
                        if !self.paused && self.should_capture_now() {
                            let size = terminal.size()?;
                            if let Err(err) =
                                self.capture(size.width, size.height.saturating_sub(2))
                            {
                                if self.is_source_closed_error(&err) {
                                    break;
                                }
                                return Err(err);
                            }
                            needs_redraw = true;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        }

        self.source.terminate().ok();
        Ok(())
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

    pub fn current_label(&self) -> Option<&str> {
        self.current_label.as_deref()
    }

    pub fn is_search_mode(&self) -> bool {
        self.input_mode == InputMode::Search
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

        if let (Some(previous_snapshot), Some(previous_label)) =
            (self.current_snapshot.take(), self.current_label.take())
        {
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
        }

        self.current_snapshot = Some(snapshot);
        self.current_label = Some(label);
        self.current_changed = current_changed;

        if !self.follow_latest && self.history.is_empty() {
            self.follow_latest = true;
        }
        self.rebuild_filter()?;
        self.append_log_record()?;
        self.last_tick = Instant::now();
        Ok(())
    }

    fn tick_timeout(&self) -> Duration {
        let interval = Duration::from_secs_f64(self.interval_secs.max(0.2));
        interval.saturating_sub(self.last_tick.elapsed())
    }

    fn should_capture_now(&self) -> bool {
        if self.source.is_event_driven() {
            self.source.has_pending_update()
        } else {
            self.last_tick.elapsed() >= Duration::from_secs_f64(self.interval_secs.max(0.2))
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<bool> {
        if self.should_ignore_mouse_ghost_key(key) {
            return Ok(false);
        }

        if self.show_exit_confirm {
            return self.handle_exit_confirm_key(key);
        }

        if self.show_help {
            match key.code {
                KeyCode::Char('h') | KeyCode::Esc => {
                    self.show_help = false;
                }
                KeyCode::Char('q') => self.show_exit_confirm = true,
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
            (KeyCode::Char('q'), _) => self.show_exit_confirm = true,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.show_exit_confirm = true,
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
                self.rebuild_filter()?;
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

    fn handle_exit_confirm_key(&mut self, key: KeyEvent) -> Result<bool> {
        match (key.code, key.modifiers) {
            (KeyCode::Char(ch), _) if matches!(ch, 'y' | 'Y' | 'q' | 'Q') => Ok(true),
            (KeyCode::Enter, _) => Ok(true),
            (KeyCode::Esc, _) => {
                self.show_exit_confirm = false;
                Ok(false)
            }
            (KeyCode::Char(ch), _) if matches!(ch, 'n' | 'N') => {
                self.show_exit_confirm = false;
                Ok(false)
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.show_exit_confirm = false;
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn is_source_closed_error(&self, err: &anyhow::Error) -> bool {
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

    fn handle_search_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.filter_query.clear();
                self.rebuild_filter()?;
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.filter_query.pop();
                self.rebuild_filter()?;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_query.push(ch);
                self.rebuild_filter()?;
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<bool> {
        if self.show_exit_confirm {
            return Ok(false);
        }

        self.last_mouse_input = Some(Instant::now());

        if self.app_input_mode {
            self.source.send_mouse(mouse, 2)?;
            return Ok(false);
        }

        if mouse.row < 2 {
            return Ok(false);
        }

        let total_width = crossterm::terminal::size()
            .map(|(width, _)| width)
            .unwrap_or(0);
        let over_history =
            self.show_history && mouse.column >= self.history_overlay_start(total_width);
        let previous_focus = self.focus;

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if over_history {
                    self.focus = FocusPane::History;
                    self.move_down();
                    return Ok(true);
                } else {
                    self.focus = FocusPane::Watch;
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                }
            }
            MouseEventKind::ScrollUp => {
                if over_history {
                    self.focus = FocusPane::History;
                    self.move_up();
                    return Ok(true);
                } else {
                    self.focus = FocusPane::Watch;
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                }
            }
            MouseEventKind::Down(_) => {
                if over_history {
                    self.focus = FocusPane::History;
                    self.select_history_row(usize::from(mouse.row.saturating_sub(2)));
                    return Ok(true);
                } else {
                    self.focus = FocusPane::Watch;
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                }
            }
            MouseEventKind::Moved => return Ok(false),
            MouseEventKind::Up(_) | MouseEventKind::Drag(_) => {
                if !over_history {
                    self.focus = FocusPane::Watch;
                    self.source.send_mouse(mouse, 2)?;
                    return Ok(previous_focus != self.focus);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn history_overlay_start(&self, total_width: u16) -> u16 {
        let overlay_width = if self.show_history { 30 } else { 2 };
        total_width.saturating_sub(overlay_width)
    }

    fn rebuild_filter(&mut self) -> Result<()> {
        self.filtered = self.history.find_by_query(&self.filter_query)?;
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

    fn should_ignore_mouse_ghost_key(&self, key: KeyEvent) -> bool {
        let Some(last_mouse_input) = self.last_mouse_input else {
            return false;
        };
        if last_mouse_input.elapsed() > Duration::from_millis(150) {
            return false;
        }

        matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('d'), KeyModifiers::NONE)
                | (KeyCode::Char('0'), KeyModifiers::NONE)
                | (KeyCode::Char('1'), KeyModifiers::NONE)
        )
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
        self.follow_latest = false;
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
            if let (Some(previous_snapshot), Some(previous_label)) =
                (self.current_snapshot.take(), self.current_label.take())
            {
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
            }

            self.current_snapshot = Some(snapshot);
            self.current_label = Some(metadata.label);
            self.current_changed = changed;
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
