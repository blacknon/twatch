use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::screen::{Cell, ScreenSnapshot, Style, TermColor};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ScreenshotFormat {
    Text,
    Svg,
}

impl ScreenshotFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Svg => "svg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Svg => "svg",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Text => Self::Svg,
            Self::Svg => Self::Text,
        }
    }
}

pub fn save_snapshot(
    snapshot: &ScreenSnapshot,
    path: &Path,
    format: ScreenshotFormat,
    header_lines: &[impl AsRef<str>],
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    match format {
        ScreenshotFormat::Text => fs::write(path, render_ansi_text(snapshot, header_lines))?,
        ScreenshotFormat::Svg => fs::write(path, render_svg(snapshot))?,
    }

    Ok(())
}

pub fn render_ansi_text(snapshot: &ScreenSnapshot, header_lines: &[impl AsRef<str>]) -> String {
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

    for y in 0..snapshot.height() {
        let end = visible_line_end(snapshot, y);
        let mut current = Style::default();

        for x in 0..end {
            let cell = snapshot.cell(x, y).cloned().unwrap_or_else(Cell::blank);
            let next_style = normalized_style(cell.style);
            if next_style != current {
                out.push_str(&style_to_sgr(next_style, current));
                current = next_style;
            }

            if cell.symbol.is_empty() {
                out.push(' ');
            } else {
                out.push_str(&cell.symbol);
            }
        }

        if current != Style::default() {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }

    out
}

fn visible_line_end(snapshot: &ScreenSnapshot, y: u16) -> u16 {
    for x in (0..snapshot.width()).rev() {
        let Some(cell) = snapshot.cell(x, y) else {
            continue;
        };
        if !cell.is_blank() || normalized_style(cell.style) != Style::default() {
            return x + 1;
        }
    }
    0
}

fn normalized_style(mut style: Style) -> Style {
    if style.inverted {
        std::mem::swap(&mut style.fg, &mut style.bg);
        style.inverted = false;
    }
    style
}

fn style_to_sgr(next: Style, _current: Style) -> String {
    let mut parts = vec!["0".to_string()];

    if next.bold {
        parts.push("1".to_string());
    }
    if next.italic {
        parts.push("3".to_string());
    }
    if next.underline {
        parts.push("4".to_string());
    }

    append_color_sgr(&mut parts, next.fg, true);
    append_color_sgr(&mut parts, next.bg, false);

    format!("\x1b[{}m", parts.join(";"))
}

fn append_color_sgr(parts: &mut Vec<String>, color: TermColor, foreground: bool) {
    match color {
        TermColor::Default => {}
        TermColor::Indexed(index) if index < 8 => {
            parts.push((if foreground { 30 } else { 40 } + i32::from(index)).to_string());
        }
        TermColor::Indexed(index) if index < 16 => {
            parts.push((if foreground { 90 } else { 100 } + i32::from(index - 8)).to_string());
        }
        TermColor::Indexed(index) => {
            parts.push(if foreground { "38".into() } else { "48".into() });
            parts.push("5".into());
            parts.push(index.to_string());
        }
        TermColor::Rgb(r, g, b) => {
            parts.push(if foreground { "38".into() } else { "48".into() });
            parts.push("2".into());
            parts.push(r.to_string());
            parts.push(g.to_string());
            parts.push(b.to_string());
        }
    }
}

fn render_svg(snapshot: &ScreenSnapshot) -> String {
    let cell_w = 10u32;
    let cell_h = 18u32;
    let width = u32::from(snapshot.width()) * cell_w;
    let height = u32::from(snapshot.height()) * cell_h;

    let mut out = String::new();
    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" font-family="monospace" font-size="14">"#
    ));
    out.push_str(r##"<rect width="100%" height="100%" fill="#1c1c1c"/>"##);

    for y in 0..snapshot.height() {
        for x in 0..snapshot.width() {
            let Some(cell) = snapshot.cell(x, y) else {
                continue;
            };
            let px = u32::from(x) * cell_w;
            let py = u32::from(y) * cell_h;

            let bg = term_color_to_css(cell.style.bg, "#1c1c1c");
            if bg != "#1c1c1c" {
                out.push_str(&format!(
                    r#"<rect x="{px}" y="{py}" width="{cell_w}" height="{cell_h}" fill="{bg}"/>"#
                ));
            }

            if !cell.symbol.trim().is_empty() {
                let fg = term_color_to_css(cell.style.fg, "#d0d0d0");
                let font_weight = if cell.style.bold {
                    " font-weight=\"bold\""
                } else {
                    ""
                };
                let font_style = if cell.style.italic {
                    " font-style=\"italic\""
                } else {
                    ""
                };
                let text_decoration = if cell.style.underline {
                    " text-decoration=\"underline\""
                } else {
                    ""
                };
                out.push_str(&format!(
                    r#"<text x="{}" y="{}" fill="{}"{}{}{}>{}</text>"#,
                    px,
                    py + 14,
                    fg,
                    font_weight,
                    font_style,
                    text_decoration,
                    escape_xml(&cell.symbol),
                ));
            }
        }
    }

    out.push_str("</svg>");
    out
}

fn term_color_to_css(color: TermColor, default: &str) -> String {
    match color {
        TermColor::Default => default.to_string(),
        TermColor::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        TermColor::Indexed(index) => ansi_index_to_css(index),
    }
}

fn ansi_index_to_css(index: u8) -> String {
    const ANSI16: [&str; 16] = [
        "#000000", "#800000", "#008000", "#808000", "#000080", "#800080", "#008080", "#c0c0c0",
        "#808080", "#ff0000", "#00ff00", "#ffff00", "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
    ];

    if usize::from(index) < ANSI16.len() {
        return ANSI16[usize::from(index)].to_string();
    }

    if (16..=231).contains(&index) {
        let idx = index - 16;
        let r = idx / 36;
        let g = (idx % 36) / 6;
        let b = idx % 6;
        let convert = |n: u8| if n == 0 { 0 } else { n * 40 + 55 };
        return format!("#{:02x}{:02x}{:02x}", convert(r), convert(g), convert(b));
    }

    let gray = 8 + (index.saturating_sub(232) * 10);
    format!("#{gray:02x}{gray:02x}{gray:02x}")
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::{ScreenshotFormat, render_ansi_text, render_svg};
    use crate::screen::{Cell, ScreenSnapshot, Style, TermColor};

    #[test]
    fn cycles_formats() {
        assert_eq!(ScreenshotFormat::Text.cycle(), ScreenshotFormat::Svg);
        assert_eq!(ScreenshotFormat::Svg.cycle(), ScreenshotFormat::Text);
    }

    #[test]
    fn renders_svg_document() {
        let snapshot = ScreenSnapshot::from_text_lines(4, 1, &["abc"]);
        let svg = render_svg(&snapshot);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<text"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn renders_ansi_text_with_color_sequences() {
        let mut snapshot = ScreenSnapshot::new(2, 1);
        snapshot.set_cell(
            0,
            0,
            Cell {
                symbol: "A".to_string(),
                style: Style {
                    fg: TermColor::Indexed(2),
                    bg: TermColor::Default,
                    bold: true,
                    italic: false,
                    underline: false,
                    inverted: false,
                },
            },
        );

        let text = render_ansi_text(&snapshot, &["head1", "head2"]);
        assert!(text.contains("\x1b[0;1;32mA"));
        assert!(text.contains("\x1b[0m\n"));
    }
}
