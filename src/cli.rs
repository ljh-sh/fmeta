use std::path::PathBuf;

use clap::Parser;

/// fmeta — find alternative that emits files with rich metadata.
///
/// Walks one or more roots and prints each file alongside detected metadata
/// columns (size, mime type, text encoding). Output is available as a plain
/// table (default) or as JSON.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "fmeta",
    version,
    propagate_version = true,
    max_term_width = 100
)]
pub struct Cli {
    /// Root directories to walk. Defaults to the current directory.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Maximum recursion depth. 0 means only the root entries themselves.
    #[arg(short = 'd', long, default_value_t = usize::MAX)]
    pub max_depth: usize,

    /// Include hidden files and directories.
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Follow symbolic links.
    #[arg(short = 'L', long = "follow")]
    pub follow_links: bool,

    /// Output format.
    #[arg(
        short = 'o',
        long = "format",
        value_enum,
        default_value_t = OutputFormat::Table,
    )]
    pub format: OutputFormat,

    /// Number of bytes to read from each file for mime/encoding detection.
    /// Larger values improve detection accuracy at the cost of IO.
    #[arg(long, value_name = "BYTES", default_value_t = 8192)]
    pub sniff: usize,

    /// Skip metadata detection entirely; emit only paths (fast).
    #[arg(long)]
    pub paths_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human readable aligned table (default).
    Table,
    /// One JSON object per line (JSON Lines / NDJSON).
    Json,
}
