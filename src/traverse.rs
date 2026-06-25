//! Filesystem traversal, backed by the `ignore` crate.
//!
//! `ignore` (the engine ripgrep uses) gives us correct `.gitignore` /
//! `.ignore` / global-gitignore / hidden-file handling for free, which is
//! exactly what an agent-oriented `find` needs. We map each `ignore::DirEntry`
//! into fmeta's small `Entry` so the rest of the pipeline (detect + render) is
//! unchanged. Output stays deterministic: entries are sorted by file name
//! within each directory (pre-order).

use std::fs;
use std::path::PathBuf;

use ignore::{DirEntry, WalkBuilder};

use crate::cli::Cli;

/// A single entry produced by the walker.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub depth: usize,
    pub is_symlink: bool,
    pub file_type: fs::FileType,
}

/// Walk the configured roots and yield entries in a deterministic, pre-order
/// traversal. Roots that cannot be read are reported on stderr but do not
/// abort the walk.
pub fn walk(roots: &[PathBuf], opts: &Cli) -> Vec<Entry> {
    let mut out = Vec::new();
    for root in roots {
        for dent in configure(WalkBuilder::new(root), opts).build() {
            match dent {
                Ok(dent) => {
                    if let Some(entry) = entry_from(&dent) {
                        out.push(entry);
                    }
                }
                Err(err) => {
                    eprintln!("fmeta: walk error near {}: {err}", root.display());
                }
            }
        }
    }
    out
}

/// Configure a builder with fmeta's options. `.gitignore` / `.ignore` /
/// hidden-file filtering is ON by default: `-a/--all` reveals hidden files and
/// `--no-ignore` disables the ignore-file filters.
fn configure(mut b: WalkBuilder, opts: &Cli) -> WalkBuilder {
    b.hidden(!opts.all);
    b.follow_links(opts.follow_links);
    b.max_depth(if opts.max_depth == usize::MAX {
        None
    } else {
        Some(opts.max_depth)
    });
    // Honour `.gitignore` files whether or not the tree is a git worktree — a
    // bare `.gitignore` in a non-git directory is still meaningful to users.
    b.require_git(false);
    // Deterministic output: sort each directory's children by name.
    b.sort_by_file_name(|a, c| a.cmp(c));
    if opts.no_ignore {
        b.ignore(false)
            .git_ignore(false)
            .git_exclude(false)
            .git_global(false)
            .parents(false);
    }
    b
}

/// Map an `ignore::DirEntry` to fmeta's `Entry`, falling back to the entry's
/// metadata when its file type is not directly available.
fn entry_from(dent: &DirEntry) -> Option<Entry> {
    let file_type = dent
        .file_type()
        .or_else(|| dent.metadata().ok().map(|m| m.file_type()))?;
    Some(Entry {
        path: dent.path().to_path_buf(),
        depth: dent.depth(),
        is_symlink: file_type.is_symlink(),
        file_type,
    })
}
