// Copyright (c) 2026 Blacknon. All rights reserved.
// Use of this source code is governed by an MIT license
// that can be found in the LICENSE file.

use std::fmt;
use std::str::FromStr;

use clap::{Parser, ValueEnum};

#[cfg(windows)]
pub fn default_shell() -> String {
    "cmd /C".to_string()
}

#[cfg(not(windows))]
pub fn default_shell() -> String {
    "sh -c".to_string()
}

#[derive(Debug, Clone, Parser)]
#[command(author, version, about = "twatch - adds rewindable history to existing TUI applications.", long_about = None)]
pub struct Cli {
    #[arg(short = 'b', long)]
    pub batch: bool,

    #[arg(long, help = "Stop after emitting this many batch frames")]
    pub batch_count: Option<usize>,

    #[arg(
        long,
        value_name = "WIDTH,HEIGHT",
        help = "Use a fixed PTY size such as 80,24"
    )]
    pub batch_size: Option<SizeSpec>,

    #[arg(
        long,
        value_name = "X,Y,WIDTH,HEIGHT",
        help = "Crop batch output to a rectangle such as 10,5,40,12"
    )]
    pub batch_crop: Option<CropSpec>,

    #[arg(long, help = "Print only added content for batch diff output")]
    pub batch_diff_only: bool,

    #[arg(long, help = "Disable ANSI color sequences in batch output")]
    pub batch_no_color: bool,

    #[arg(short = 'A', long)]
    pub aftercommand: Option<String>,

    #[arg(
        short = 'K',
        long,
        help = "Remap twatch keys with KEY=ACTION",
        value_name = "KEY=ACTION"
    )]
    pub keymap: Vec<String>,

    #[arg(
        short = 'k',
        long,
        help = "Override child TUI keys with FROM=TO",
        value_name = "FROM=TO"
    )]
    pub bind: Vec<String>,

    #[arg(long, help = "Only run aftercommand when output matches this regex")]
    pub aftercommand_regex: Option<String>,

    #[arg(
        long,
        help = "Only run aftercommand when changed cell count reaches this threshold"
    )]
    pub aftercommand_change_cells: Option<usize>,

    #[arg(long, help = "Only run aftercommand on every Nth changed frame")]
    pub aftercommand_every: Option<usize>,

    #[arg(long, help = "Debounce aftercommand for this many milliseconds")]
    pub aftercommand_debounce_ms: Option<u64>,

    #[arg(
        long,
        default_value_t = 3000,
        help = "Kill aftercommand if it exceeds this timeout in milliseconds"
    )]
    pub aftercommand_timeout_ms: u64,

    #[arg(short = 'C', long)]
    pub compress: bool,

    #[arg(short = 'l', long)]
    pub logfile: Option<String>,

    #[arg(long, help = "Replay a saved JSONL trace in read-only mode")]
    pub replay: Option<String>,

    #[arg(long, default_value = "/tmp")]
    pub screenshot_dir: String,

    #[arg(long, value_enum, default_value_t = ScreenshotFormatArg::Text)]
    pub screenshot_format: ScreenshotFormatArg,

    #[arg(
        long,
        help = "Auto-save a snapshot when the screen contains this string"
    )]
    pub snapshot_on: Option<String>,

    #[arg(long, help = "Auto-save a snapshot when the screen matches this regex")]
    pub snapshot_on_regex: Option<String>,

    #[arg(
        long,
        help = "Auto-save a snapshot when changed cell count reaches this threshold"
    )]
    pub snapshot_on_change_cells: Option<usize>,

    #[arg(long, help = "Only trigger automatic snapshot once")]
    pub snapshot_once: bool,

    #[arg(short = 's', long, default_value_t = default_shell())]
    pub shell: String,

    #[arg(
        short = 'd',
        long,
        value_enum,
        default_value_t = DiffModeArg::None,
        help = "Diff mode: watch for TUI mode, list/word for batch mode"
    )]
    pub differences: DiffModeArg,

    #[arg(short = 'L', long, default_value_t = 500)]
    pub limit: usize,

    #[arg(long, default_value_t = 12)]
    pub checkpoint_interval: usize,

    #[arg(long, help = "Show debug diagnostics in the interactive UI")]
    pub debug: bool,

    #[arg()]
    pub command: Vec<String>,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum DiffModeArg {
    #[default]
    None,
    Watch,
    #[value(alias = "line")]
    List,
    Word,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ScreenshotFormatArg {
    #[default]
    Text,
    Svg,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SizeSpec {
    pub width: u16,
    pub height: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CropSpec {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl SizeSpec {
    pub fn new(width: u16, height: u16) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("size must be greater than zero".to_string());
        }
        Ok(Self { width, height })
    }
}

impl FromStr for SizeSpec {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (width, height) = value
            .trim()
            .split_once(',')
            .ok_or_else(|| "size must use WIDTH,HEIGHT".to_string())?;
        let width = width
            .parse::<u16>()
            .map_err(|_| "width must be a positive integer".to_string())?;
        let height = height
            .parse::<u16>()
            .map_err(|_| "height must be a positive integer".to_string())?;
        Self::new(width, height)
    }
}

impl fmt::Display for SizeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.width, self.height)
    }
}

impl CropSpec {
    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("crop size must be greater than zero".to_string());
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }
}

impl FromStr for CropSpec {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.trim().split(',');
        let x = parts
            .next()
            .ok_or_else(|| "crop must use X,Y,WIDTH,HEIGHT".to_string())?
            .parse::<u16>()
            .map_err(|_| "crop x must be a non-negative integer".to_string())?;
        let y = parts
            .next()
            .ok_or_else(|| "crop must use X,Y,WIDTH,HEIGHT".to_string())?
            .parse::<u16>()
            .map_err(|_| "crop y must be a non-negative integer".to_string())?;
        let width = parts
            .next()
            .ok_or_else(|| "crop must use X,Y,WIDTH,HEIGHT".to_string())?
            .parse::<u16>()
            .map_err(|_| "crop width must be a positive integer".to_string())?;
        let height = parts
            .next()
            .ok_or_else(|| "crop must use X,Y,WIDTH,HEIGHT".to_string())?
            .parse::<u16>()
            .map_err(|_| "crop height must be a positive integer".to_string())?;
        if parts.next().is_some() {
            return Err("crop must use X,Y,WIDTH,HEIGHT".to_string());
        }
        Self::new(x, y, width, height)
    }
}

impl fmt::Display for CropSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{}", self.x, self.y, self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::{CropSpec, DiffModeArg, SizeSpec, default_shell};
    use clap::ValueEnum;

    #[test]
    fn parses_size_spec() {
        assert_eq!(
            "80,24".parse::<SizeSpec>().unwrap(),
            SizeSpec {
                width: 80,
                height: 24,
            }
        );
    }

    #[test]
    fn parses_crop_spec() {
        assert_eq!(
            "10,5,40,12".parse::<CropSpec>().unwrap(),
            CropSpec {
                x: 10,
                y: 5,
                width: 40,
                height: 12,
            }
        );
    }

    #[test]
    fn batch_diff_accepts_line_alias() {
        let names: Vec<_> = DiffModeArg::value_variants()
            .iter()
            .map(|value| value.to_possible_value().unwrap().get_name().to_string())
            .collect();
        assert!(names.contains(&"list".to_string()));
    }

    #[test]
    fn default_shell_matches_platform() {
        #[cfg(windows)]
        assert_eq!(default_shell(), "cmd /C");

        #[cfg(not(windows))]
        assert_eq!(default_shell(), "sh -c");
    }
}
