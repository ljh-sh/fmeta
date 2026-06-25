//! Filesystem traversal.
//!
//! A small, dependency-free recursive walker. Unlike `jwalk`/`ignore`, this
//! stays in-tree and gives us full control over ordering, hidden files, link
//! following, and depth limits — which is all `fmeta` needs for v0.

use std::fs;
use std::path::{Path, PathBuf};

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
        walk_one(root, 0, opts, &mut out);
    }
    out
}

fn walk_one(path: &Path, depth: usize, opts: &Cli, out: &mut Vec<Entry>) {
    let meta = match symlink_metadata(path) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("fmeta: cannot stat {}: {err}", path.display());
            return;
        }
    };

    let is_symlink = meta.file_type().is_symlink();
    let resolved = if is_symlink {
        match resolve_symlink(path, opts) {
            Ok(m) => m,
            Err(err) => {
                eprintln!("fmeta: broken symlink {}: {err}", path.display());
                // Still record the symlink itself as an entry.
                out.push(Entry {
                    path: path.to_path_buf(),
                    depth,
                    is_symlink: true,
                    file_type: meta.file_type(),
                });
                return;
            }
        }
    } else {
        meta.file_type()
    };

    // Emit the entry itself (files, dirs, and symlinked files). We do not emit
    // directory entries when they will be recursed into — mirrors `find`'s
    // default behaviour of printing every path it visits.
    out.push(Entry {
        path: path.to_path_buf(),
        depth,
        is_symlink,
        file_type: resolved,
    });

    if !resolved.is_dir() {
        return;
    }
    if depth >= opts.max_depth {
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("fmeta: cannot read dir {}: {err}", path.display());
            return;
        }
    };

    let mut children: Vec<PathBuf> = entries
        .filter_map(|res| res.ok())
        .map(|e| e.path())
        .collect();
    children.sort();

    for child in children {
        if !opts.all && is_hidden(&child) {
            continue;
        }
        walk_one(&child, depth + 1, opts, out);
    }
}

fn symlink_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    fs::symlink_metadata(path)
}

fn resolve_symlink(path: &Path, opts: &Cli) -> std::io::Result<fs::FileType> {
    if opts.follow_links {
        fs::metadata(path).map(|m| m.file_type())
    } else {
        fs::symlink_metadata(path).map(|m| m.file_type())
    }
}

fn is_hidden(path: &Path) -> bool {
    // A path is considered hidden if any of its file name components starts
    // with a dot. This mirrors `find`'s default (no `-a`) and `ignore`'s
    // convention without pulling in gitignore semantics.
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.') && n != "." && n != "..")
        .unwrap_or(false)
}
