use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, widgets::Widget};
use regex::Regex;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, DiffMode, FilterMode, FocusPane};
use crate::screen::{Cell, ScreenSnapshot};

const HEADER_BG: Color = Color::Indexed(234);
const HEADER_SUB_BG: Color = Color::Indexed(235);
const PANEL_BG: Color = Color::Indexed(234);
const BORDER_ACTIVE: Color = Color::Indexed(45);
const BORDER_IDLE: Color = Color::Indexed(240);
const SEARCH_BG: Color = Color::Indexed(220);
const DIFF_BG: Color = Color::Rgb(238, 238, 238);
const DIFF_FG: Color = Color::Black;
const SELECTION_BG: Color = Color::Indexed(24);
const COMMAND_FG: Color = Color::Indexed(47);
const COMMAND_ACCENT: Color = Color::Indexed(51);
const TIMESTAMP_FG: Color = Color::Indexed(51);
const BADGE_TEXT: Color = Color::Black;
const BADGE_DIM_TEXT: Color = Color::Indexed(252);
const BADGE_GREEN: Color = Color::Indexed(46);
const BADGE_CYAN: Color = Color::Indexed(51);
const BADGE_BLUE: Color = Color::Indexed(39);
const BADGE_MAGENTA: Color = Color::Indexed(201);
const BADGE_GREY: Color = Color::Indexed(238);

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

    draw_header_line_one(frame, app, chunks[0]);
    draw_header_line_two(frame, app, chunks[1]);

    draw_watch(frame, app, chunks[2]);
    draw_history_overlay(frame, app, chunks[2]);

    if app.show_help {
        draw_help(frame, centered_rect(68, 60, area));
    }
    if app.show_exit_confirm {
        draw_exit_confirm(frame, centered_rect(34, 16, area));
    }
}

fn draw_header_line_one(frame: &mut Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(HEADER_BG)),
        area,
    );

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(24)])
        .split(area);

    let cadence = if app.is_event_driven() {
        "Event".to_string()
    } else {
        format!("Every {:>4.3}", app.interval_secs)
    };
    let command_line = Line::from(vec![
        Span::styled(" ", Style::default().bg(HEADER_BG)),
        Span::styled(cadence, Style::default().fg(Color::White).bg(HEADER_BG)),
        Span::styled(" ", Style::default().bg(HEADER_BG)),
        Span::styled(
            app.command_display().to_string(),
            Style::default()
                .fg(COMMAND_FG)
                .bg(HEADER_BG)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let timestamp_line = Line::from(vec![Span::styled(
        format!(
            " {} ",
            app.current_label().unwrap_or("---- -- -- --:--:--.---")
        ),
        Style::default()
            .fg(TIMESTAMP_FG)
            .bg(HEADER_BG)
            .add_modifier(Modifier::BOLD),
    )]);

    frame.render_widget(Paragraph::new(command_line), chunks[0]);
    frame.render_widget(
        Paragraph::new(timestamp_line).alignment(Alignment::Right),
        chunks[1],
    );
}

fn draw_header_line_two(frame: &mut Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(HEADER_SUB_BG)),
        area,
    );

    let filter_label = if app.is_search_mode() {
        if app.filter_query.is_empty() {
            format!(
                "Filter: {}",
                match app.filter_mode() {
                    FilterMode::Plain => "/",
                    FilterMode::Regex => "*",
                }
            )
        } else {
            format!(
                "Filter: {}{}",
                match app.filter_mode() {
                    FilterMode::Plain => "/",
                    FilterMode::Regex => "*",
                },
                app.filter_query
            )
        }
    } else if app.filter_query.is_empty() {
        "Filter".to_string()
    } else {
        format!(
            "Filter: {}{}",
            match app.filter_mode() {
                FilterMode::Plain => "/",
                FilterMode::Regex => "*",
            },
            app.filter_query
        )
    };
    let input_hint_long = if app.app_input_mode {
        " | Input: app mode, Ctrl-g to return"
    } else {
        " | Input: press i to control app"
    };
    let input_hint_short = if app.app_input_mode {
        " | Ctrl-g to return"
    } else {
        " | i to control app"
    };

    let hist_label = format!(
        "Hist {}/{} {} {}",
        format!("{:05}", app.filtered_indices().len()),
        format!("{:05}", app.history_len()),
        if app.show_history { "Open" } else { "Close" },
        if app.follow_latest { "Latest" } else { "Hold" }
    );
    let input_label = if app.app_input_mode {
        "Input On".to_string()
    } else {
        "Input Off".to_string()
    };
    let diff_label = app.diff_mode.label().to_string();
    let right_width =
        badge_width(&hist_label) + 1 + badge_width(&input_label) + 1 + badge_width(&diff_label);

    let available_left = usize::from(area.width).saturating_sub(right_width.saturating_add(1));
    let resize_summary = app.selected_resize_summary();
    let left_text = fit_header_left(
        &filter_label,
        input_hint_long,
        input_hint_short,
        app.status_message
            .as_deref()
            .or(resize_summary.as_deref())
            .or_else(|| app.selected_input_summary()),
        available_left.saturating_sub(1),
    );
    let left = Line::from(vec![Span::styled(
        format!(" {}", left_text),
        Style::default()
            .fg(if app.filter_query.is_empty() && !app.is_search_mode() {
                Color::Indexed(246)
            } else {
                COMMAND_ACCENT
            })
            .bg(HEADER_SUB_BG),
    )]);

    let right = Line::from(vec![
        status_badge(hist_label, BADGE_CYAN, true),
        gap(HEADER_SUB_BG),
        status_badge(input_label, BADGE_GREEN, true),
        gap(HEADER_SUB_BG),
        status_badge(
            diff_label,
            match app.diff_mode {
                DiffMode::None => BADGE_MAGENTA,
                DiffMode::Watch => BADGE_BLUE,
            },
            true,
        ),
    ]);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(right_width.min(usize::from(area.width)) as u16),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(left), chunks[0]);
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), chunks[1]);
}

fn status_badge<T: Into<String>>(label: T, bg: Color, active: bool) -> Span<'static> {
    let fg = if active { BADGE_TEXT } else { BADGE_DIM_TEXT };
    let bg = if active { bg } else { BADGE_GREY };
    Span::styled(
        format!("[{}]", label.into()),
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    )
}

fn gap(bg: Color) -> Span<'static> {
    Span::styled(" ", Style::default().bg(bg))
}

fn badge_width(label: &str) -> usize {
    UnicodeWidthStr::width(label) + 2
}

fn fit_header_left(
    filter_label: &str,
    long_hint: &str,
    short_hint: &str,
    status_hint: Option<&str>,
    max_width: usize,
) -> String {
    if let Some(status_hint) = status_hint {
        let status = format!("{filter_label} | {status_hint}");
        if display_width(&status) <= max_width {
            return status;
        }
    }

    let long = format!("{filter_label}{long_hint}");
    if display_width(&long) <= max_width {
        return long;
    }

    let short = format!("{filter_label}{short_hint}");
    if display_width(&short) <= max_width {
        return short;
    }

    truncate_text(filter_label, max_width)
}

fn truncate_text(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 1 {
        return String::new();
    }

    let keep = max_width.saturating_sub(1);
    let mut out = String::new();
    for ch in text.chars() {
        if display_width(&out) + UnicodeWidthChar::width(ch).unwrap_or(0) > keep {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
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
            filter_mode: app.filter_mode(),
            vertical_scroll: app.watch_scroll,
            horizontal_scroll: app.horizontal_scroll,
        }
        .render(inner, frame.buffer_mut());
    }
}

fn draw_history_overlay(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let width = if app.show_history { 48 } else { 2 };
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
                Style::default().fg(Color::White).bg(SELECTION_BG)
            } else {
                Style::default().fg(BORDER_ACTIVE)
            }),
        ];

    items.extend(app.filtered_indices().iter().map(|index| {
        let meta = app.history_metadata(*index);
        let style = if !app.follow_latest && *index == app.selected_index {
            Style::default().fg(Color::White).bg(SELECTION_BG)
        } else if meta.changed {
            Style::default().fg(Color::Indexed(221))
        } else {
            Style::default().fg(Color::Gray)
        };
        ListItem::new(Line::from(format!(
            "{} {:04} +{:03} i{:02} {}x{} {}{}",
            if !app.follow_latest && *index == app.selected_index {
                ">"
            } else {
                " "
            },
            meta.frame_seq,
            meta.changed_cell_count,
            meta.input_event_count_since_prev,
            meta.width,
            meta.height,
            if meta.resized { "R " } else { "" },
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
        Line::from("Ctrl-c        open exit dialog"),
        Line::from("Tab           switch focus between watch/history"),
        Line::from("i             enter child app input mode"),
        Line::from("Ctrl-g        leave child app input mode"),
        Line::from("Left/Right    focus watch/history"),
        Line::from("Up/Down       child app in watch pane / move history"),
        Line::from("Backspace     toggle history pane"),
        Line::from("/             search history"),
        Line::from("*             regex filter history"),
        Line::from("D             delete selected history"),
        Line::from("X             clear history except selected"),
        Line::from("s             cycle snapshot format (text/svg)"),
        Line::from("S             save selected snapshot"),
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

fn draw_exit_confirm(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Clear, area);
    let lines = vec![
        Line::from("Exit twatch?"),
        Line::from(""),
        Line::from("Press 'Y' or 'Q' : Quit."),
        Line::from("Press 'N' or 'Esc': Stay."),
    ];
    let dialog = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Exit ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_ACTIVE))
                .style(Style::default().bg(PANEL_BG)),
        )
        .style(Style::default().fg(Color::White).bg(PANEL_BG));
    frame.render_widget(dialog, area);
}

struct WatchWidget<'a> {
    snapshot: &'a ScreenSnapshot,
    previous: Option<&'a ScreenSnapshot>,
    diff_mode: DiffMode,
    diff_only: bool,
    search_query: &'a str,
    filter_mode: FilterMode,
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
            Some(self.search_query)
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
            .map(|needle| find_char_matches(&plain, needle, self.filter_mode))
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

fn find_char_matches(line: &str, needle: &str, filter_mode: FilterMode) -> Vec<usize> {
    match filter_mode {
        FilterMode::Plain => {
            let lower = line.to_lowercase();
            let needle = needle.to_lowercase();
            let mut result = Vec::new();
            let mut byte_offset = 0usize;

            while let Some(found) = lower[byte_offset..].find(&needle) {
                let start = byte_offset + found;
                let end = start + needle.len();
                let start_char = lower[..start].chars().count();
                let end_char = lower[..end].chars().count();
                result.extend(start_char..end_char);
                byte_offset = end;
            }

            result
        }
        FilterMode::Regex => {
            let Ok(regex) = Regex::new(needle) else {
                return Vec::new();
            };
            let mut result = Vec::new();
            for found in regex.find_iter(line) {
                let start_char = line[..found.start()].chars().count();
                let end_char = line[..found.end()].chars().count();
                result.extend(start_char..end_char);
            }
            result
        }
    }
}
