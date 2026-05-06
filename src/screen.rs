use std::fmt;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub inverted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: Style::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenSnapshot {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

impl ScreenSnapshot {
    pub fn new(width: u16, height: u16) -> Self {
        let len = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            cells: vec![Cell::default(); len],
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
                        ch,
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
                    resized.set_cell(x, y, *cell);
                }
            }
        }

        *self = resized;
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
            .map(|y| {
                let mut line = String::with_capacity(usize::from(self.width));
                for x in 0..self.width {
                    line.push(self.cell(x, y).copied().unwrap_or_default().ch);
                }
                line.trim_end_matches(' ').to_string()
            })
            .collect()
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
                self.cells[*idx] = *cell;
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
    use super::{Cell, ScreenSnapshot, Style};

    #[test]
    fn trims_trailing_spaces_when_rendering_lines() {
        let mut snapshot = ScreenSnapshot::new(4, 1);
        snapshot.set_cell(
            0,
            0,
            Cell {
                ch: 'a',
                style: Style::default(),
            },
        );
        snapshot.set_cell(
            1,
            0,
            Cell {
                ch: 'b',
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
}
