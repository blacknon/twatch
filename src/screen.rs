// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::fmt;

use ratatui::style::{Color, Modifier, Style as TuiStyle};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Symbol {
    #[default]
    Empty,
    Inline {
        len: u8,
        bytes: [u8; 4],
    },
    Heap(Box<str>),
}

impl Symbol {
    pub fn new(value: &str) -> Self {
        if value.is_empty() {
            return Self::Empty;
        }
        if value.len() <= 4 {
            let mut bytes = [0u8; 4];
            bytes[..value.len()].copy_from_slice(value.as_bytes());
            return Self::Inline {
                len: value.len() as u8,
                bytes,
            };
        }
        Self::Heap(value.into())
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Empty => "",
            Self::Inline { len, bytes } => std::str::from_utf8(&bytes[..usize::from(*len)])
                .expect("inline symbol must be valid utf-8"),
            Self::Heap(value) => value,
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub fn is_blank(&self) -> bool {
        self.as_str().trim().is_empty()
    }
}

impl From<&str> for Symbol {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Symbol {
    fn from(value: String) -> Self {
        Self::new(&value)
    }
}

impl From<char> for Symbol {
    fn from(value: char) -> Self {
        let mut buffer = [0u8; 4];
        let encoded = value.encode_utf8(&mut buffer);
        Self::new(encoded)
    }
}

impl Serialize for Symbol {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Symbol {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from(value))
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub symbol: Symbol,
    pub style: Style,
}

impl Cell {
    pub fn blank() -> Self {
        Self {
            symbol: Symbol::from(' '),
            style: Style::default(),
        }
    }

    pub fn is_blank(&self) -> bool {
        self.symbol.is_blank()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct DeltaCell {
    pub symbol: Symbol,
    pub style_id: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CompactCell {
    symbol: Symbol,
    style_id: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct SerializableCell {
    symbol: Symbol,
    style_id: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SnapshotRunSerde {
    style_id: u32,
    text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    symbols: Vec<Symbol>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RowSnapshotSerde {
    width: u16,
    height: u16,
    rows: Vec<Vec<SnapshotRunSerde>>,
    styles: Vec<Style>,
    cursor_x: u16,
    cursor_y: u16,
    cursor_visible: bool,
    alternate_screen: bool,
    scrollback_offset: usize,
    mouse_reporting: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CompactSnapshotSerde {
    width: u16,
    height: u16,
    cells: Vec<SerializableCell>,
    styles: Vec<Style>,
    cursor_x: u16,
    cursor_y: u16,
    cursor_visible: bool,
    alternate_screen: bool,
    scrollback_offset: usize,
    mouse_reporting: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LegacySnapshotSerde {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
    cursor_x: u16,
    cursor_y: u16,
    cursor_visible: bool,
    alternate_screen: bool,
    scrollback_offset: usize,
    mouse_reporting: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(untagged)]
enum SnapshotSerde {
    Rows(RowSnapshotSerde),
    Compact(CompactSnapshotSerde),
    Legacy(LegacySnapshotSerde),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScreenSnapshot {
    width: u16,
    height: u16,
    cells: Vec<CompactCell>,
    styles: Vec<Style>,
    cursor_x: u16,
    cursor_y: u16,
    cursor_visible: bool,
    alternate_screen: bool,
    scrollback_offset: usize,
    mouse_reporting: bool,
}

impl ScreenSnapshot {
    pub fn new(width: u16, height: u16) -> Self {
        let len = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            cells: vec![
                CompactCell {
                    symbol: Symbol::from(' '),
                    style_id: 0,
                };
                len
            ],
            styles: vec![Style::default()],
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: false,
            alternate_screen: false,
            scrollback_offset: 0,
            mouse_reporting: false,
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
                        symbol: Symbol::from(ch),
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
                    resized.set_cell(x, y, cell);
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
                    cropped.set_cell(dst_x, dst_y, cell);
                }
            }
        }

        cropped
    }

    pub fn cell(&self, x: u16, y: u16) -> Option<Cell> {
        self.index(x, y).map(|idx| self.decode_cell(idx))
    }

    pub(crate) fn delta_cell(&self, x: u16, y: u16) -> Option<DeltaCell> {
        self.index(x, y).map(|idx| {
            let cell = &self.cells[idx];
            DeltaCell {
                symbol: cell.symbol.clone(),
                style_id: cell.style_id,
            }
        })
    }

    pub fn set_cell(&mut self, x: u16, y: u16, cell: Cell) {
        if let Some(idx) = self.index(x, y) {
            self.cells[idx] = CompactCell {
                symbol: cell.symbol,
                style_id: self.intern_style(cell.style),
            };
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

    pub fn mouse_reporting(&self) -> bool {
        self.mouse_reporting
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

    pub fn set_mouse_reporting(&mut self, mouse_reporting: bool) {
        self.mouse_reporting = mouse_reporting;
    }

    pub fn plain_line(&self, y: u16) -> String {
        let mut line = String::with_capacity(usize::from(self.width));
        for x in 0..self.width {
            let cell = self.cell(x, y).unwrap_or_else(Cell::blank);
            if cell.symbol.is_empty() {
                line.push(' ');
            } else {
                line.push_str(cell.symbol.as_str());
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
                self.cells[*idx] = CompactCell {
                    symbol: cell.symbol.clone(),
                    style_id: self.intern_style(cell.style),
                };
            }
        }
    }

    pub(crate) fn apply_delta_changes(&mut self, changes: &[(usize, DeltaCell)], styles: &[Style]) {
        for (idx, cell) in changes {
            if *idx < self.cells.len() {
                let style = styles
                    .get(cell.style_id as usize)
                    .copied()
                    .unwrap_or_default();
                self.cells[*idx] = CompactCell {
                    symbol: cell.symbol.clone(),
                    style_id: self.intern_style(style),
                };
            }
        }
    }

    pub fn changed_cell_count_since(&self, previous: Option<&ScreenSnapshot>) -> usize {
        if previous.is_none() {
            return self
                .cells
                .iter()
                .map(|cell| Cell {
                    symbol: cell.symbol.clone(),
                    style: self.styles[cell.style_id as usize],
                })
                .filter(|cell| *cell != Cell::blank())
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
                    .unwrap_or_else(Cell::blank);
                let after = self.cell(x, y).unwrap_or_else(Cell::blank);
                if before != after {
                    changed += 1;
                }
            }
        }
        changed
    }

    fn decode_cell(&self, idx: usize) -> Cell {
        let cell = &self.cells[idx];
        Cell {
            symbol: cell.symbol.clone(),
            style: self.styles[cell.style_id as usize],
        }
    }

    fn intern_style(&mut self, style: Style) -> u32 {
        if let Some((index, _)) = self
            .styles
            .iter()
            .enumerate()
            .find(|(_, item)| **item == style)
        {
            return index as u32;
        }
        self.styles.push(style);
        (self.styles.len() - 1) as u32
    }
}

impl Serialize for ScreenSnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut rows = Vec::with_capacity(self.height as usize);
        for y in 0..self.height {
            let mut row_runs = Vec::new();
            let mut current_run: Option<SnapshotRunSerde> = None;
            for x in 0..self.width {
                let cell = &self.cells[usize::from(y) * usize::from(self.width) + usize::from(x)];
                let symbol_text = cell.symbol.as_str();
                let can_use_text = symbol_text.chars().count() == 1 && !cell.symbol.is_empty();

                match &mut current_run {
                    Some(run)
                        if run.style_id == cell.style_id
                            && ((run.symbols.is_empty() && can_use_text)
                                || (!run.symbols.is_empty() && !can_use_text)) =>
                    {
                        if can_use_text {
                            run.text.push_str(symbol_text);
                        } else {
                            run.symbols.push(cell.symbol.clone());
                        }
                    }
                    Some(run) => {
                        row_runs.push(std::mem::take(run));
                        let mut next_run = SnapshotRunSerde {
                            style_id: cell.style_id,
                            text: String::new(),
                            symbols: Vec::new(),
                        };
                        if can_use_text {
                            next_run.text.push_str(symbol_text);
                        } else {
                            next_run.symbols.push(cell.symbol.clone());
                        }
                        *run = next_run;
                    }
                    None => {
                        let mut run = SnapshotRunSerde {
                            style_id: cell.style_id,
                            text: String::new(),
                            symbols: Vec::new(),
                        };
                        if can_use_text {
                            run.text.push_str(symbol_text);
                        } else {
                            run.symbols.push(cell.symbol.clone());
                        }
                        current_run = Some(run);
                    }
                }
            }
            if let Some(run) = current_run.take() {
                row_runs.push(run);
            }
            rows.push(row_runs);
        }

        RowSnapshotSerde {
            width: self.width,
            height: self.height,
            rows,
            styles: self.styles.clone(),
            cursor_x: self.cursor_x,
            cursor_y: self.cursor_y,
            cursor_visible: self.cursor_visible,
            alternate_screen: self.alternate_screen,
            scrollback_offset: self.scrollback_offset,
            mouse_reporting: self.mouse_reporting,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ScreenSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match SnapshotSerde::deserialize(deserializer)? {
            SnapshotSerde::Rows(value) => {
                let mut snapshot = Self::new(value.width, value.height);
                snapshot.cursor_x = value.cursor_x;
                snapshot.cursor_y = value.cursor_y;
                snapshot.cursor_visible = value.cursor_visible;
                snapshot.alternate_screen = value.alternate_screen;
                snapshot.scrollback_offset = value.scrollback_offset;
                snapshot.mouse_reporting = value.mouse_reporting;
                snapshot.styles = if value.styles.is_empty() {
                    vec![Style::default()]
                } else {
                    value.styles
                };

                for (y, row) in value.rows.into_iter().enumerate() {
                    if y >= usize::from(snapshot.height) {
                        break;
                    }
                    let mut x = 0usize;
                    for run in row {
                        let symbols: Vec<Symbol> = if run.symbols.is_empty() {
                            run.text.chars().map(Symbol::from).collect()
                        } else {
                            run.symbols
                        };
                        for symbol in symbols {
                            if x >= usize::from(snapshot.width) {
                                break;
                            }
                            let idx = y * usize::from(snapshot.width) + x;
                            snapshot.cells[idx] = CompactCell {
                                symbol,
                                style_id: run.style_id,
                            };
                            x += 1;
                        }
                    }
                }

                Ok(snapshot)
            }
            SnapshotSerde::Compact(value) => Ok(Self {
                width: value.width,
                height: value.height,
                cells: value
                    .cells
                    .into_iter()
                    .map(|cell| CompactCell {
                        symbol: cell.symbol,
                        style_id: cell.style_id,
                    })
                    .collect(),
                styles: if value.styles.is_empty() {
                    vec![Style::default()]
                } else {
                    value.styles
                },
                cursor_x: value.cursor_x,
                cursor_y: value.cursor_y,
                cursor_visible: value.cursor_visible,
                alternate_screen: value.alternate_screen,
                scrollback_offset: value.scrollback_offset,
                mouse_reporting: value.mouse_reporting,
            }),
            SnapshotSerde::Legacy(value) => {
                let mut snapshot = Self::new(value.width, value.height);
                snapshot.cursor_x = value.cursor_x;
                snapshot.cursor_y = value.cursor_y;
                snapshot.cursor_visible = value.cursor_visible;
                snapshot.alternate_screen = value.alternate_screen;
                snapshot.scrollback_offset = value.scrollback_offset;
                snapshot.mouse_reporting = value.mouse_reporting;
                for (idx, cell) in value.cells.into_iter().enumerate() {
                    if idx >= snapshot.cells.len() {
                        break;
                    }
                    snapshot.cells[idx] = CompactCell {
                        symbol: cell.symbol,
                        style_id: snapshot.intern_style(cell.style),
                    };
                }
                Ok(snapshot)
            }
        }
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
    use super::{Cell, ScreenSnapshot, Style, Symbol};

    #[test]
    fn trims_trailing_spaces_when_rendering_lines() {
        let mut snapshot = ScreenSnapshot::new(4, 1);
        snapshot.set_cell(
            0,
            0,
            Cell {
                symbol: Symbol::from('a'),
                style: Style::default(),
            },
        );
        snapshot.set_cell(
            1,
            0,
            Cell {
                symbol: Symbol::from('b'),
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

    #[test]
    fn serializes_snapshots_as_row_runs() {
        let snapshot = ScreenSnapshot::from_text_lines(6, 2, &["aa  bb", "cccccc"]);

        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"rows\""));
        assert!(json.contains("\"styles\""));
        assert!(!json.contains("\"cells\""));

        let restored: ScreenSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.lines(), snapshot.lines());
    }
}
