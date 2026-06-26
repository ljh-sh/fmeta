//! fmeta library entry point.
//!
//! `fmeta` is a `find` alternative that emits each visited file alongside
//! rich metadata (size, mime type, text encoding). It is designed to be easy
//! for AI agents to consume: deterministic ordering, machine-readable JSON
//! output, and a small, stable schema.

pub mod cli;
pub mod db;
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

    // DB query mode: run raw SQL against the index.
    if let Some(sql) = &opts.sql {
        return run_query(&opts, sql);
    }

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

    // DB index mode: walk + incremental upsert into the index.
    if opts.index {
        return run_index(&opts, &roots);
    }

    // Default: one-shot walk → TSV/table/JSON.
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
                !opts.fast,
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

/// Resolve the DB path: `--db` override, else the default global location.
fn db_path(opts: &Cli) -> anyhow::Result<PathBuf> {
    if let Some(p) = &opts.db {
        return Ok(p.clone());
    }
    db::default_db_path()
        .ok_or_else(|| anyhow::anyhow!("could not resolve default DB path ($HOME unset)"))
}

/// `--sql "<sql>"`: run raw SQL on the index, print rows as TSV.
fn run_query(opts: &Cli, sql: &str) -> anyhow::Result<()> {
    let path = db_path(opts)?;
    let conn = db::open(&path)?;
    let (cols, rows) = db::run_query(&conn, sql)?;
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if !opts.format.is_json() {
        // Header line for tsv/table (skip for json — SQL results aren't FileMeta).
        writeln!(out, "{}", cols.join("\t"))?;
    }
    for row in &rows {
        writeln!(out, "{}", row.join("\t"))?;
    }
    Ok(())
}

/// `--index [path...]`: walk + upsert into the index DB. Incremental: a file
/// whose mtime matches the cached row is left untouched.
fn run_index(opts: &Cli, roots: &[PathBuf]) -> anyhow::Result<()> {
    let path = db_path(opts)?;
    let conn = db::open(&path)?;
    let indexed_at = now_epoch().unwrap_or(0);

    let mut indexed = 0usize;
    let mut cached = 0usize;
    let mut errors = 0usize;
    for root in roots {
        for e in walk(std::slice::from_ref(root), opts) {
            // Cache check: skip deep extraction if mtime is unchanged.
            let cur_mtime = std::fs::metadata(&e.path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(epoch);
            if let Some(cur) = cur_mtime {
                if let Ok(Some(prev)) = db::cached_mtime(&conn, &e.path.to_string_lossy()) {
                    if prev == cur {
                        cached += 1;
                        continue;
                    }
                }
            }
            let meta = FileMeta::for_entry(
                &e.path,
                e.depth,
                e.is_symlink,
                e.file_type,
                opts.sniff,
                opts.paths_only,
                !opts.fast,
            );
            match db::upsert(&conn, &meta, indexed_at) {
                Ok(()) => indexed += 1,
                Err(_) => errors += 1,
            }
        }
    }
    eprintln!(
        "fmeta: indexed {} (cached {}, errors {}) into {}",
        indexed,
        cached,
        errors,
        path.display()
    );
    Ok(())
}

fn now_epoch() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

fn epoch(t: std::time::SystemTime) -> Option<i64> {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => Some(d.as_secs() as i64),
        Err(e) => Some(-(e.duration().as_secs() as i64)),
    }
}

impl OutputFormat {
    fn is_json(&self) -> bool {
        matches!(self, OutputFormat::Json)
    }
}
