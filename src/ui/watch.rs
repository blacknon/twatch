use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, widgets::Widget};
use regex::Regex;

use crate::app::{App, DiffMode, FilterMode};
use crate::screen::{Cell, ScreenSnapshot};

use super::{BORDER_ACTIVE, DIFF_BG, DIFF_FG, PANEL_BG, SEARCH_BG};

pub(super) fn draw_watch(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let debug_height = if app.debug && app.source_debug_status().is_some() {
        1
    } else {
        0
    };
    let inspector_height = if app.ui.show_inspector { 6 } else { 0 };
    let watch_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(inspector_height),
            Constraint::Length(debug_height),
        ])
        .split(area);

    let block = Block::default()
        .style(Style::default().bg(PANEL_BG))
        .borders(Borders::NONE);
    let inner = block.inner(watch_chunks[0]);
    frame.render_widget(block, watch_chunks[0]);

    if let Some(snapshot) = app.selected_snapshot() {
        let previous = app.previous_snapshot();
        let (inspect_x, inspect_y) = app.inspect_cursor();
        WatchWidget {
            snapshot: &snapshot,
            previous: previous.as_ref(),
            diff_mode: app.diff_mode,
            diff_only: app.diff_only,
            search_query: &app.ui.filter_query,
            filter_mode: app.filter_mode(),
            vertical_scroll: app.ui.watch_scroll,
            horizontal_scroll: app.ui.horizontal_scroll,
            inspect_cell: if app.ui.show_inspector {
                Some((inspect_x, inspect_y))
            } else {
                None
            },
        }
        .render(inner, frame.buffer_mut());

        if app.ui.show_inspector {
            draw_inspector(
                frame,
                watch_chunks[1],
                &snapshot,
                previous.as_ref(),
                inspect_x,
                inspect_y,
            );
        }

        if let Some(position) = watch_cursor_position(app, &snapshot, previous.as_ref(), inner) {
            frame.set_cursor_position(position);
        }
    }

    if debug_height > 0 {
        draw_debug_status(frame, watch_chunks[2], app.source_debug_status().as_deref());
    }
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
    inspect_cell: Option<(u16, u16)>,
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
            if self.inspect_cell == Some((src_col as u16, row)) {
                style = style
                    .bg(Color::Indexed(208))
                    .fg(Color::Black)
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

fn draw_inspector(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &ScreenSnapshot,
    previous: Option<&ScreenSnapshot>,
    inspect_x: u16,
    inspect_y: u16,
) {
    let cell = snapshot
        .cell(inspect_x, inspect_y)
        .cloned()
        .unwrap_or_else(Cell::blank);
    let previous_cell = previous
        .and_then(|snapshot| snapshot.cell(inspect_x, inspect_y))
        .cloned();
    let same_as_previous = previous_cell.as_ref().is_some_and(|before| before == &cell);
    let symbol_label = if cell.symbol == " " {
        "<space>".to_string()
    } else if cell.symbol.is_empty() {
        "<empty>".to_string()
    } else {
        cell.symbol.clone()
    };
    let previous_symbol = previous_cell
        .as_ref()
        .map(|cell| {
            if cell.symbol == " " {
                "<space>".to_string()
            } else if cell.symbol.is_empty() {
                "<empty>".to_string()
            } else {
                cell.symbol.clone()
            }
        })
        .unwrap_or_else(|| "<none>".to_string());

    let lines = vec![
        Line::from(format!(
            "x={}, y={} | symbol={} | blank={}",
            inspect_x,
            inspect_y,
            symbol_label,
            cell.is_blank()
        )),
        Line::from(format!(
            "fg={:?} bg={:?} | bold={} italic={} underline={} inverted={}",
            cell.style.fg,
            cell.style.bg,
            cell.style.bold,
            cell.style.italic,
            cell.style.underline,
            cell.style.inverted
        )),
        Line::from(format!(
            "previous symbol={} | same_as_previous={}",
            previous_symbol, same_as_previous
        )),
        Line::from("Shift+Arrow: move inspector cursor | I: toggle"),
    ];

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Inspector ")
                .borders(Borders::TOP)
                .border_style(Style::default().fg(BORDER_ACTIVE))
                .style(Style::default().bg(PANEL_BG)),
        )
        .style(Style::default().fg(Color::White).bg(PANEL_BG));
    frame.render_widget(widget, area);
}

fn draw_debug_status(frame: &mut Frame<'_>, area: Rect, debug_status: Option<&str>) {
    let line = debug_status.unwrap_or("");
    let widget = Paragraph::new(Line::from(line.to_string()))
        .style(Style::default().fg(Color::Indexed(244)).bg(PANEL_BG));
    frame.render_widget(widget, area);
}

fn watch_cursor_position(
    app: &App,
    snapshot: &ScreenSnapshot,
    previous: Option<&ScreenSnapshot>,
    area: Rect,
) -> Option<Position> {
    if !snapshot.cursor_visible()
        || !app.should_render_terminal_cursor()
        || app.ui.show_help
        || app.ui.show_exit_confirm
        || app.is_search_mode()
        || area.width == 0
        || area.height == 0
    {
        return None;
    }

    let (cursor_x, cursor_y) = snapshot.cursor_position();
    let cursor_x = usize::from(cursor_x);
    let cursor_y = usize::from(cursor_y);

    if cursor_x < app.ui.horizontal_scroll {
        return None;
    }

    let visible_x = cursor_x - app.ui.horizontal_scroll;
    if visible_x >= usize::from(area.width) {
        return None;
    }

    let start_row = app.ui.watch_scroll.min(usize::from(snapshot.height()));
    if cursor_y < start_row {
        return None;
    }

    let visible_y = if app.diff_only {
        let widget = WatchWidget {
            snapshot,
            previous,
            diff_mode: app.diff_mode,
            diff_only: app.diff_only,
            search_query: &app.ui.filter_query,
            filter_mode: app.filter_mode(),
            vertical_scroll: app.ui.watch_scroll,
            horizontal_scroll: app.ui.horizontal_scroll,
            inspect_cell: None,
        };
        if !widget.row_changed(cursor_y as u16) {
            return None;
        }
        let mut visible = 0usize;
        for row in start_row..cursor_y {
            if widget.row_changed(row as u16) {
                visible += 1;
            }
        }
        visible
    } else {
        cursor_y - start_row
    };

    if visible_y >= usize::from(area.height) {
        return None;
    }

    Some(Position::new(
        area.x + visible_x as u16,
        area.y + visible_y as u16,
    ))
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
