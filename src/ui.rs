use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, widgets::Widget};

use crate::app::{App, DiffMode, FocusPane};
use crate::screen::{Cell, ScreenSnapshot};

const HEADER_BG: Color = Color::Indexed(24);
const HEADER_SUB_BG: Color = Color::Indexed(238);
const PANEL_BG: Color = Color::Indexed(234);
const BORDER_ACTIVE: Color = Color::Indexed(39);
const BORDER_IDLE: Color = Color::Indexed(240);
const SEARCH_BG: Color = Color::Indexed(220);
const DIFF_BG: Color = Color::Rgb(238, 238, 238);
const DIFF_FG: Color = Color::Black;
const SELECTION_BG: Color = Color::Indexed(24);

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    frame.render_widget(header_line_one(app), chunks[0]);
    frame.render_widget(header_line_two(app), chunks[1]);

    draw_watch(frame, app, chunks[2]);
    draw_history_overlay(frame, app, chunks[2]);

    if app.show_help {
        draw_help(frame, centered_rect(68, 60, area));
    }
}

fn header_line_one(app: &App) -> Paragraph<'static> {
    let left = format!(
        " twatch [{}] diff:{} focus:{} input:{} interval:{:.1}s history:{}/{} ",
        if app.paused { "PAUSED" } else { "RUN" },
        app.diff_mode.label(),
        app.focus.label(),
        if app.app_input_mode { "app" } else { "twatch" },
        app.interval_secs,
        app.filtered_indices().len(),
        app.history_len()
    );
    Paragraph::new(left).style(Style::default().fg(Color::White).bg(HEADER_BG))
}

fn header_line_two(app: &App) -> Paragraph<'static> {
    let search = if app.filter_query.is_empty() {
        "/ search".to_string()
    } else {
        format!("search: {}", app.filter_query)
    };
    let help = if app.app_input_mode {
        "Ctrl-g return | keys/mouse pass through to app"
    } else {
        "Tab pane | i app input | Backspace history | d diff | h help | q quit"
    };
    let content = format!(" {search} | {help} ");
    Paragraph::new(content).style(Style::default().fg(Color::White).bg(HEADER_SUB_BG))
}

fn draw_watch(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default()
        .style(Style::default().bg(PANEL_BG))
        .borders(Borders::NONE);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(snapshot) = app.selected_snapshot() {
        let previous = app.previous_snapshot();
        WatchWidget {
            snapshot: &snapshot,
            previous: previous.as_ref(),
            diff_mode: app.diff_mode,
            diff_only: app.diff_only,
            search_query: &app.filter_query,
            vertical_scroll: app.watch_scroll,
            horizontal_scroll: app.horizontal_scroll,
        }
        .render(inner, frame.buffer_mut());
    }
}

fn draw_history_overlay(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let width = if app.show_history { 30 } else { 2 };
    let overlay = Rect::new(
        area.right().saturating_sub(width),
        area.y,
        width,
        area.height,
    );

    if !app.show_history {
        frame.render_widget(
            Paragraph::new("Hi")
                .alignment(Alignment::Center)
                .style(Style::default().fg(BORDER_IDLE).bg(PANEL_BG)),
            overlay,
        );
        return;
    }

    frame.render_widget(Clear, overlay);
    let title = if app.focus == FocusPane::History {
        " History "
    } else {
        " history "
    };

    let mut items: Vec<ListItem<'_>> =
        vec![
            ListItem::new(Line::from("  latest")).style(if app.follow_latest {
                Style::default().fg(Color::Gray).bg(BORDER_ACTIVE)
            } else {
                Style::default().fg(BORDER_ACTIVE)
            }),
        ];

    items.extend(app.filtered_indices().iter().map(|index| {
        let meta = app.history_metadata(*index);
        let style = if *index == app.selected_index {
            Style::default().fg(Color::White).bg(SELECTION_BG)
        } else if meta.changed {
            Style::default().fg(Color::Indexed(221))
        } else {
            Style::default().fg(Color::Gray)
        };
        ListItem::new(Line::from(format!(
            "{} {}",
            if *index == app.selected_index {
                ">"
            } else {
                " "
            },
            meta.label
        )))
        .style(style)
    }));

    let mut state = ListState::default();
    state.select(Some(app.selected_history_row()));
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .style(Style::default().bg(PANEL_BG))
                .borders(Borders::LEFT)
                .border_style(if app.focus == FocusPane::History {
                    Style::default().fg(BORDER_ACTIVE)
                } else {
                    Style::default().fg(BORDER_IDLE)
                }),
        )
        .highlight_symbol(">");
    frame.render_stateful_widget(list, overlay, &mut state);
}

fn draw_help(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Clear, area);
    let lines = vec![
        Line::from("twatch keys"),
        Line::from(""),
        Line::from("q             quit"),
        Line::from("Tab           toggle watch/history pane focus"),
        Line::from("i             enter child app input mode"),
        Line::from("Ctrl-g        leave child app input mode"),
        Line::from("Left/Right    focus watch/history"),
        Line::from("Up/Down       child app in watch pane / move history"),
        Line::from("Backspace     toggle history pane"),
        Line::from("/             search history"),
        Line::from("d / 0 1       diff mode"),
        Line::from("p             pause history capture"),
        Line::from("Alt+Left/Right horizontal scroll"),
    ];
    let help = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .style(Style::default().bg(PANEL_BG)),
        )
        .style(Style::default().fg(Color::White).bg(PANEL_BG));
    frame.render_widget(help, area);
}

struct WatchWidget<'a> {
    snapshot: &'a ScreenSnapshot,
    previous: Option<&'a ScreenSnapshot>,
    diff_mode: DiffMode,
    diff_only: bool,
    search_query: &'a str,
    vertical_scroll: usize,
    horizontal_scroll: usize,
}

impl Widget for WatchWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let max_rows = usize::from(area.height);
        let start_row = self
            .vertical_scroll
            .min(usize::from(self.snapshot.height()));
        let end_row = (start_row + max_rows).min(usize::from(self.snapshot.height()));
        let search = if self.search_query.is_empty() {
            None
        } else {
            Some(self.search_query.to_lowercase())
        };

        let mut visible_row = 0u16;
        for src_row in start_row..end_row {
            if self.diff_only && !self.row_changed(src_row as u16) {
                continue;
            }
            if visible_row >= area.height {
                break;
            }
            self.render_row(area, buf, src_row as u16, visible_row, search.as_deref());
            visible_row += 1;
        }
    }
}

impl WatchWidget<'_> {
    fn render_row(
        &self,
        area: Rect,
        buf: &mut Buffer,
        row: u16,
        visible_row: u16,
        search: Option<&str>,
    ) {
        let plain = self.snapshot.plain_line(row);
        let search_matches = search
            .map(|needle| find_char_matches(&plain, needle))
            .unwrap_or_default();

        let mut x = 0u16;
        for src_col in self.horizontal_scroll..usize::from(self.snapshot.width()) {
            if x >= area.width {
                break;
            }
            let cell = self
                .snapshot
                .cell(src_col as u16, row)
                .cloned()
                .unwrap_or_else(Cell::blank);
            let mut style = cell.style.to_ratatui();
            if self.cell_changed(src_col as u16, row) && matches!(self.diff_mode, DiffMode::Watch) {
                style = style.bg(DIFF_BG).fg(DIFF_FG);
            }
            if search_matches.contains(&src_col) {
                style = style
                    .fg(Color::Black)
                    .bg(SEARCH_BG)
                    .add_modifier(Modifier::BOLD);
            }
            let symbol = if cell.symbol.is_empty() {
                " "
            } else {
                &cell.symbol
            };
            buf[(area.x + x, area.y + visible_row)]
                .set_symbol(symbol)
                .set_style(style);
            x += 1;
        }
        while x < area.width {
            buf[(area.x + x, area.y + visible_row)]
                .set_symbol(" ")
                .set_style(Style::default().bg(PANEL_BG));
            x += 1;
        }
    }

    fn cell_changed(&self, col: u16, row: u16) -> bool {
        self.previous
            .and_then(|prev| prev.cell(col, row))
            .map(|before| {
                self.snapshot
                    .cell(col, row)
                    .map(|after| before != after)
                    .unwrap_or(true)
            })
            .unwrap_or(true)
    }

    fn row_changed(&self, row: u16) -> bool {
        match self.diff_mode {
            DiffMode::None => true,
            DiffMode::Watch => {
                for col in 0..self.snapshot.width() {
                    if self.cell_changed(col, row) {
                        return true;
                    }
                }
                false
            }
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn find_char_matches(line: &str, needle: &str) -> Vec<usize> {
    let lower = line.to_lowercase();
    let mut result = Vec::new();
    let mut byte_offset = 0usize;

    while let Some(found) = lower[byte_offset..].find(needle) {
        let start = byte_offset + found;
        let end = start + needle.len();
        let start_char = lower[..start].chars().count();
        let end_char = lower[..end].chars().count();
        result.extend(start_char..end_char);
        byte_offset = end;
    }

    result
}
