//! Output rendering: aligned table or JSON Lines.

use crate::cli::OutputFormat;
use crate::detect::FileMeta;

/// Render `entries` using the requested format to stdout.
pub fn render(entries: &[FileMeta], format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Table => render_table(entries),
        OutputFormat::Json => render_json(entries),
    }
}

fn render_json(entries: &[FileMeta]) -> anyhow::Result<()> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for entry in entries {
        serde_json::to_writer(&mut out, entry)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn render_table(entries: &[FileMeta]) -> anyhow::Result<()> {
    use std::io::Write;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if entries.is_empty() {
        return Ok(());
    }

    // Columns: depth  size    kind   mime                     encoding     path
    // We compute per-column widths for alignment, capping path-less columns.
    let header = ["depth", "size", "kind", "mime", "encoding", "path"];
    let mut widths = header.map(|h| h.len());

    let formatted: Vec<[String; 6]> = entries
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
                e.path.clone(),
            ]
        })
        .collect();

    for row in &formatted {
        for (i, cell) in row.iter().enumerate() {
            // Cap the first five columns so a long path doesn't push them out.
            if i < 5 {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    // Header
    for (i, h) in header.iter().enumerate() {
        if i == 5 {
            write!(out, "  {h}")?;
        } else {
            write!(out, "{:width$}  ", h, width = widths[i])?;
        }
    }
    writeln!(out)?;

    // Separator
    for (i, _) in header.iter().enumerate() {
        if i == 5 {
            write!(out, "  {}", "-".repeat(widths[i].min(40)))?;
        } else {
            write!(out, "{}  ", "-".repeat(widths[i]))?;
        }
    }
    writeln!(out)?;

    // Rows
    for row in &formatted {
        for (i, cell) in row.iter().enumerate() {
            if i == 5 {
                write!(out, "  {cell}")?;
            } else {
                write!(out, "{:width$}  ", cell, width = widths[i])?;
            }
        }
        writeln!(out)?;
    }

    Ok(())
}
