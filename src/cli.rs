use std::path::PathBuf;

use clap::Parser;

/// fmeta — find alternative that emits files with rich metadata.
///
/// Walks one or more roots and prints each file alongside detected metadata
/// columns (size, kind, encoding, mime, mime hint). Output is TSV by default
/// (agent/pipeline friendly); `--format table` for a human-aligned table or
/// `--format json` for JSON Lines.
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

    /// Disable `.gitignore` / `.ignore` filtering (hidden files still controlled by -a).
    #[arg(long)]
    pub no_ignore: bool,

    /// Follow symbolic links.
    #[arg(short = 'L', long = "follow")]
    pub follow_links: bool,

    /// Output format.
    #[arg(
        short = 'o',
        long = "format",
        value_enum,
        default_value_t = OutputFormat::Tsv,
    )]
    pub format: OutputFormat,

    /// Number of bytes to read from each file for mime/encoding detection.
    /// Larger values improve detection accuracy at the cost of IO.
    #[arg(long, value_name = "BYTES", default_value_t = 8192)]
    pub sniff: usize,

    /// Skip metadata detection entirely; emit only paths (fast).
    #[arg(long)]
    pub paths_only: bool,

    /// Bounded walk: only read `--sniff` bytes per file and skip whole-file
    /// extractors (PDF pages, audio duration/tags). The DEFAULT is "deep"
    /// (reads whole files for rich metadata); pass `--fast` for a quick pass
    /// over huge trees.
    #[arg(long)]
    pub fast: bool,

    /// **Index mode**: walk the given roots and upsert metadata into the fmeta
    /// index DB (default `~/.local/data/ljh-sh/fmeta/sqlite.db`, override with
    /// `--db`). Incremental: files whose mtime is unchanged are left untouched.
    #[arg(long)]
    pub index: bool,

    /// **Query mode**: run raw SQL against the index DB and print rows as TSV.
    /// Example: `fmeta --sql "SELECT path,size FROM files WHERE category='image'
    /// ORDER BY size DESC LIMIT 20"`.
    #[arg(long, value_name = "SQL")]
    pub sql: Option<String>,

    /// Override the index DB path (default `~/.local/data/ljh-sh/fmeta/sqlite.db`).
    #[arg(long, value_name = "PATH")]
    pub db: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// Tab-separated values, one row per file (default; for agents/pipelines).
    Tsv,
    /// Human-readable aligned table.
    Table,
    /// One JSON object per line (JSON Lines / NDJSON).
    Json,
}
