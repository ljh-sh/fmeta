//! Output rendering: TSV (default), aligned table, or JSON Lines.

use std::io::Write;

use crate::cli::OutputFormat;
use crate::detect::FileMeta;

/// Render `entries` using the requested format to stdout.
pub fn render(entries: &[FileMeta], format: OutputFormat) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    render_to(&mut out, entries, format)
}

/// Render `entries` to `out`. Factored out so tests can capture output.
pub fn render_to<W: Write>(
    out: &mut W,
    entries: &[FileMeta],
    format: OutputFormat,
) -> anyhow::Result<()> {
    match format {
        OutputFormat::Tsv => render_tsv(out, entries),
        OutputFormat::Table => render_table(out, entries),
        OutputFormat::Json => render_json(out, entries),
    }
}

/// Default format: one tab-separated row per entry, no header.
///
/// Columns: `size  kind  encoding  mime  mime_hint  path` — `path` is last so
/// paths containing spaces or tabs don't break column-aware consumers (`awk`,
/// `cut`, LLMs). Missing values render as `-`.
fn render_tsv<W: Write>(out: &mut W, entries: &[FileMeta]) -> anyhow::Result<()> {
    for e in entries {
        let size = e
            .size
            .map(|s| s.to_string())
            .unwrap_or_else(|| "-".to_string());
        let kind = format!("{:?}", e.kind).to_ascii_lowercase();
        let encoding = e.encoding.clone().unwrap_or_else(|| "-".to_string());
        let mime = e.mime.clone().unwrap_or_else(|| "-".to_string());
        let hint = e.category.clone().unwrap_or_else(|| "-".to_string());
        writeln!(
            out,
            "{size}\t{kind}\t{encoding}\t{mime}\t{hint}\t{}",
            e.path
        )?;
    }
    Ok(())
}

fn render_json<W: Write>(out: &mut W, entries: &[FileMeta]) -> anyhow::Result<()> {
    for entry in entries {
        serde_json::to_writer(&mut *out, entry)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn render_table<W: Write>(out: &mut W, entries: &[FileMeta]) -> anyhow::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    // Columns: depth  size  kind  mime  encoding  mime_hint  path
    // We compute per-column widths for alignment, capping every column except
    // the last (path) so a long path doesn't push the others off-screen.
    let header = [
        "depth",
        "size",
        "kind",
        "mime",
        "encoding",
        "mime_hint",
        "path",
    ];
    let mut widths = header.map(|h| h.len());

    let formatted: Vec<[String; 7]> = entries
        .iter()
        .map(|e| {
            [
                e.depth.to_string(),
                e.size
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                format!("{:?}", e.kind).to_ascii_lowercase(),
                e.mime.clone().unwrap_or_else(|| "-".to_string()),
                e.encoding.clone().unwrap_or_else(|| "-".to_string()),
                e.category.clone().unwrap_or_else(|| "-".to_string()),
                e.path.clone(),
            ]
        })
        .collect();

    for row in &formatted {
        for (i, cell) in row.iter().enumerate() {
            if i < 6 {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    let n = header.len();
    let last = n - 1;

    // Header
    for (i, h) in header.iter().enumerate() {
        if i == last {
            write!(out, "  {h}")?;
        } else {
            write!(out, "{:width$}  ", h, width = widths[i])?;
        }
    }
    writeln!(out)?;

    // Separator
    for (i, w) in widths.iter().enumerate() {
        if i == last {
            write!(out, "  {}", "-".repeat((*w).min(40)))?;
        } else {
            write!(out, "{}  ", "-".repeat(*w))?;
        }
    }
    writeln!(out)?;

    // Rows
    for row in &formatted {
        for (i, cell) in row.iter().enumerate() {
            if i == last {
                write!(out, "  {cell}")?;
            } else {
                write!(out, "{:width$}  ", cell, width = widths[i])?;
            }
        }
        writeln!(out)?;
    }

    Ok(())
}
