// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{BORDER_ACTIVE, PANEL_BG};

pub(super) fn draw_help(frame: &mut Frame<'_>, area: Rect) {
    let area = centered_rect(68, 60, area);
    frame.render_widget(Clear, area);
    let lines = help_lines();
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

fn help_lines() -> Vec<Line<'static>> {
    vec![
        Line::from("twatch keys"),
        Line::from(""),
        Line::from("q             quit"),
        Line::from("Ctrl-c        open exit dialog"),
        Line::from("Tab           switch focus between watch/history"),
        Line::from("i             enter child app input mode"),
        Line::from("Ctrl-g        leave child app input mode"),
        Line::from("Left/Right    focus watch/history"),
        Line::from("Up/Down       child app in watch pane / move history"),
        Line::from("Mouse         passthrough only on latest"),
        Line::from("--keymap      remap twatch actions"),
        Line::from("--bind        override child TUI keys"),
        Line::from("Backspace     toggle history pane"),
        Line::from("Shift+H       toggle header"),
        Line::from("/             search history"),
        Line::from("*             regex filter history"),
        Line::from("D             delete selected history"),
        Line::from("X             clear history except selected"),
        Line::from("s             cycle snapshot format (text/svg)"),
        Line::from("Shift+S       toggle selected frame info"),
        Line::from("Ctrl+S        save selected snapshot"),
        Line::from("d / 0 1       diff mode"),
        Line::from("p             pause twatch capture"),
        Line::from("Shift+P       pause child process"),
        Line::from(", / .         auto replay reverse / forward"),
        Line::from("Space         auto replay pause / resume"),
        Line::from("[ / ]         auto replay speed down / up"),
        Line::from("Alt+Left/Right horizontal scroll"),
    ]
}

pub(super) fn draw_exit_confirm(frame: &mut Frame<'_>, area: Rect) {
    let area = centered_rect(34, 16, area);
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

pub(super) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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

#[cfg(test)]
mod tests {
    use super::help_lines;

    #[test]
    fn help_lists_toggle_header_shortcut() {
        let rendered: Vec<String> = help_lines()
            .into_iter()
            .map(|line| line.to_string())
            .collect();
        assert!(rendered.iter().any(|line| line.contains("Shift+H")));
        assert!(rendered.iter().any(|line| line.contains(", / .")));
    }
}
