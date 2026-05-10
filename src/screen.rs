use std::fmt;

use ratatui::style::{Color, Modifier, Style as TuiStyle};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum TermColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl TermColor {
    pub fn to_ratatui(self) -> Color {
        match self {
            Self::Default => Color::Reset,
            Self::Indexed(index) => Color::Indexed(index),
            Self::Rgb(r, g, b) => Color::Rgb(r, g, b),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverted: bool,
}

impl Style {
    pub fn to_ratatui(self) -> TuiStyle {
        let mut style = TuiStyle::default()
            .fg(self.fg.to_ratatui())
            .bg(self.bg.to_ratatui());
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        if self.inverted {
            style = style.add_modifier(Modifier::REVERSED);
        }
        style
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub symbol: String,
    pub style: Style,
}

impl Cell {
    pub fn blank() -> Self {
        Self {
            symbol: " ".to_string(),
            style: Style::default(),
        }
    }

    pub fn is_blank(&self) -> bool {
        self.symbol.trim().is_empty()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenSnapshot {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
    cursor_x: u16,
    cursor_y: u16,
    cursor_visible: bool,
    alternate_screen: bool,
    scrollback_offset: usize,
}

impl ScreenSnapshot {
    pub fn new(width: u16, height: u16) -> Self {
        let len = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            cells: vec![Cell::blank(); len],
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: false,
            alternate_screen: false,
            scrollback_offset: 0,
        }
    }

    pub fn from_text_lines(width: u16, height: u16, lines: &[impl AsRef<str>]) -> Self {
        let mut snapshot = Self::new(width, height);

        for (y, line) in lines.iter().take(usize::from(height)).enumerate() {
            for (x, ch) in line.as_ref().chars().take(usize::from(width)).enumerate() {
                snapshot.set_cell(
                    x as u16,
                    y as u16,
                    Cell {
                        symbol: ch.to_string(),
                        style: Style::default(),
                    },
                );
            }
        }

        snapshot
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        if self.width == width && self.height == height {
            return;
        }

        let mut resized = ScreenSnapshot::new(width, height);
        let max_width = self.width.min(width);
        let max_height = self.height.min(height);

        for y in 0..max_height {
            for x in 0..max_width {
                if let Some(cell) = self.cell(x, y) {
                    resized.set_cell(x, y, cell.clone());
                }
            }
        }

        *self = resized;
    }

    pub fn cropped(&self, x: u16, y: u16, width: u16, height: u16) -> Self {
        let mut cropped = ScreenSnapshot::new(width, height);

        for dst_y in 0..height {
            for dst_x in 0..width {
                let src_x = x.saturating_add(dst_x);
                let src_y = y.saturating_add(dst_y);
                if let Some(cell) = self.cell(src_x, src_y) {
                    cropped.set_cell(dst_x, dst_y, cell.clone());
                }
            }
        }

        cropped
    }

    pub fn cell(&self, x: u16, y: u16) -> Option<&Cell> {
        self.index(x, y).map(|idx| &self.cells[idx])
    }

    pub fn set_cell(&mut self, x: u16, y: u16, cell: Cell) {
        if let Some(idx) = self.index(x, y) {
            self.cells[idx] = cell;
        }
    }

    pub fn lines(&self) -> Vec<String> {
        (0..self.height)
            .map(|y| self.plain_line(y).trim_end_matches(' ').to_string())
            .collect()
    }

    pub fn cursor_position(&self) -> (u16, u16) {
        (self.cursor_x, self.cursor_y)
    }

    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    pub fn alternate_screen(&self) -> bool {
        self.alternate_screen
    }

    pub fn scrollback_offset(&self) -> usize {
        self.scrollback_offset
    }

    pub fn set_cursor_state(&mut self, x: u16, y: u16, visible: bool) {
        self.cursor_x = x.min(self.width.saturating_sub(1));
        self.cursor_y = y.min(self.height.saturating_sub(1));
        self.cursor_visible = visible;
    }

    pub fn set_screen_mode(&mut self, alternate_screen: bool, scrollback_offset: usize) {
        self.alternate_screen = alternate_screen;
        self.scrollback_offset = scrollback_offset;
    }

    pub fn plain_line(&self, y: u16) -> String {
        let mut line = String::with_capacity(usize::from(self.width));
        for x in 0..self.width {
            let cell = self.cell(x, y).cloned().unwrap_or_else(Cell::blank);
            if cell.symbol.is_empty() {
                line.push(' ');
            } else {
                line.push_str(&cell.symbol);
            }
        }
        line
    }

    pub fn batch_render(&self, header_lines: &[impl AsRef<str>]) -> String {
        let mut out = String::new();

        for line in header_lines.iter().take(2) {
            out.push_str(line.as_ref());
            out.push('\n');
        }

        if header_lines.len() < 2 {
            for _ in header_lines.len()..2 {
                out.push('\n');
            }
        }

        for line in self.lines() {
            out.push_str(&line);
            out.push('\n');
        }

        out
    }

    fn index(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }

        Some(usize::from(y) * usize::from(self.width) + usize::from(x))
    }

    pub(crate) fn apply_changes(&mut self, changes: &[(usize, Cell)]) {
        for (idx, cell) in changes {
            if *idx < self.cells.len() {
                self.cells[*idx] = cell.clone();
            }
        }
    }

    pub fn changed_cell_count_since(&self, previous: Option<&ScreenSnapshot>) -> usize {
        if previous.is_none() {
            return self
                .cells
                .iter()
                .filter(|cell| **cell != Cell::blank())
                .count();
        }

        let width = self
            .width
            .max(previous.map(|snapshot| snapshot.width).unwrap_or(0));
        let height = self
            .height
            .max(previous.map(|snapshot| snapshot.height).unwrap_or(0));

        let mut changed = 0usize;
        for y in 0..height {
            for x in 0..width {
                let before = previous
                    .and_then(|snapshot| snapshot.cell(x, y))
                    .cloned()
                    .unwrap_or_else(Cell::blank);
                let after = self.cell(x, y).cloned().unwrap_or_else(Cell::blank);
                if before != after {
                    changed += 1;
                }
            }
        }
        changed
    }
}

impl fmt::Display for ScreenSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for line in self.lines() {
            writeln!(f, "{line}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, ScreenSnapshot, Style};

    #[test]
    fn trims_trailing_spaces_when_rendering_lines() {
        let mut snapshot = ScreenSnapshot::new(4, 1);
        snapshot.set_cell(
            0,
            0,
            Cell {
                symbol: "a".to_string(),
                style: Style::default(),
            },
        );
        snapshot.set_cell(
            1,
            0,
            Cell {
                symbol: "b".to_string(),
                style: Style::default(),
            },
        );

        assert_eq!(snapshot.lines(), vec!["ab".to_string()]);
    }

    #[test]
    fn preserves_existing_region_on_resize() {
        let mut snapshot = ScreenSnapshot::from_text_lines(4, 2, &["abcd", "wxyz"]);
        snapshot.resize(5, 3);

        assert_eq!(
            snapshot.lines(),
            vec!["abcd".to_string(), "wxyz".to_string(), "".to_string()]
        );
    }

    #[test]
    fn crops_snapshot_region() {
        let snapshot = ScreenSnapshot::from_text_lines(5, 2, &["abcde", "fghij"]);
        let cropped = snapshot.cropped(1, 0, 3, 2);
        assert_eq!(cropped.lines(), vec!["bcd".to_string(), "ghi".to_string()]);
    }

    #[test]
    fn counts_changed_cells_against_previous_snapshot() {
        let before = ScreenSnapshot::from_text_lines(4, 2, &["ab", ""]);
        let after = ScreenSnapshot::from_text_lines(4, 2, &["ax", "z"]);

        assert_eq!(after.changed_cell_count_since(Some(&before)), 2);
    }

    #[test]
    fn counts_changed_cells_against_blank_when_no_previous_snapshot() {
        let snapshot = ScreenSnapshot::from_text_lines(4, 1, &["ab"]);

        assert_eq!(snapshot.changed_cell_count_since(None), 2);
    }

    #[test]
    fn does_not_count_blank_cells_added_by_resize_as_changed() {
        let before = ScreenSnapshot::from_text_lines(2, 1, &[""]);
        let after = ScreenSnapshot::from_text_lines(4, 2, &[""]);

        assert_eq!(after.changed_cell_count_since(Some(&before)), 0);
    }

    #[test]
    fn stores_cursor_state() {
        let mut snapshot = ScreenSnapshot::new(4, 3);
        snapshot.set_cursor_state(2, 1, true);

        assert_eq!(snapshot.cursor_position(), (2, 1));
        assert!(snapshot.cursor_visible());
    }
}
