//! fmeta library entry point.
//!
//! `fmeta` is a `find` alternative that emits each visited file alongside
//! rich metadata (size, mime type, text encoding). It is designed to be easy
//! for AI agents to consume: deterministic ordering, machine-readable JSON
//! output, and a small, stable schema.

pub mod cli;
pub mod detect;
pub mod output;
pub mod traverse;

use std::path::PathBuf;

use clap::Parser;

use cli::{Cli, OutputFormat};
use detect::FileMeta;
use output::render;
use traverse::walk;

/// Run the CLI and return the top-level result.
pub fn run() -> anyhow::Result<()> {
    let opts = Cli::parse();

    let roots: Vec<PathBuf> = if opts.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        opts.paths.clone()
    };

    // Validate roots up front so a typo does not produce a silent empty walk.
    for root in &roots {
        if !root.exists() {
            anyhow::bail!("path does not exist: {}", root.display());
        }
    }

    let entries = walk(&roots, &opts);

    let metas: Vec<FileMeta> = entries
        .iter()
        .map(|e| {
            FileMeta::for_entry(
                &e.path,
                e.depth,
                e.is_symlink,
                e.file_type,
                opts.sniff,
                opts.paths_only,
            )
        })
        .collect();

    if matches!(opts.format, OutputFormat::Table) && metas.is_empty() {
        // Nothing to print; keep it quiet rather than emitting a lone header.
        return Ok(());
    }

    render(&metas, opts.format)?;
    Ok(())
}
