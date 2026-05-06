use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Parser)]
#[command(author, version, about = "watch for TUI apps", long_about = None)]
pub struct Cli {
    #[arg(short = 'n', long, default_value_t = 2.0)]
    pub interval: f64,

    #[arg(short = 'b', long)]
    pub batch: bool,

    #[arg(short = 'A', long)]
    pub aftercommand: Option<String>,

    #[arg(short = 'C', long)]
    pub compress: bool,

    #[arg(short = 'l', long)]
    pub logfile: Option<String>,

    #[arg(long, default_value = "/tmp")]
    pub screenshot_dir: String,

    #[arg(long, value_enum, default_value_t = ScreenshotFormatArg::Text)]
    pub screenshot_format: ScreenshotFormatArg,

    #[arg(short = 's', long, default_value = "sh -c")]
    pub shell: String,

    #[arg(short = 'd', long, value_enum, default_value_t = DiffModeArg::None)]
    pub differences: DiffModeArg,

    #[arg(short = 'L', long, default_value_t = 500)]
    pub limit: usize,

    #[arg(long, default_value_t = 12)]
    pub checkpoint_interval: usize,

    #[arg(long)]
    pub demo: bool,

    #[arg()]
    pub command: Vec<String>,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum DiffModeArg {
    #[default]
    None,
    Watch,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ScreenshotFormatArg {
    #[default]
    Text,
    Svg,
}
