use std::path::PathBuf;

use clap::Parser;

use crate::output::OutputFormat;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "annotui",
    version,
    about = "Comment on files and piped text in a mouse-first terminal UI"
)]
pub struct Cli {
    /// File to review, or - to read standard input
    #[arg(value_name = "FILE", conflicts_with = "buffer")]
    pub input: Option<PathBuf>,

    /// Review this literal text instead of a file or standard input
    #[arg(long, value_name = "TEXT", conflicts_with = "input")]
    pub buffer: Option<String>,

    /// Display name used for standard input or --buffer
    #[arg(long, value_name = "NAME")]
    pub source_name: Option<String>,

    /// Load and save comments in this JSON sidecar
    #[arg(long, value_name = "PATH")]
    pub comments: Option<PathBuf>,

    /// Final output format written after the TUI closes
    #[arg(long, value_enum, default_value_t)]
    pub format: OutputFormat,

    /// Write final output to a file instead of standard output
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Disable mouse capture and use keyboard selection only
    #[arg(long)]
    pub no_mouse: bool,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn comments_is_the_default_format() {
        let cli = Cli::try_parse_from(["annotui", "input.txt"]).unwrap();
        assert_eq!(cli.format, OutputFormat::Comments);
    }

    #[test]
    fn buffer_conflicts_with_a_file() {
        assert!(Cli::try_parse_from(["annotui", "input.txt", "--buffer", "hello"]).is_err());
    }
}
