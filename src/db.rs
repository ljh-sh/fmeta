//! Persistent file-metadata index (SQLite). fmeta's "database" mode.
//!
//! Default DB: `~/.local/data/ljh-sh/fmeta/sqlite.db` (override with `--db`).
//! WAL mode for concurrent readers. Files are keyed by absolute path; `mtime`
//! is the incremental-reindex cache key (a file whose mtime is unchanged since
//! the last `--index` is left untouched).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

use crate::detect::FileMeta;

/// Default DB path: `$HOME/.local/data/ljh-sh/fmeta/sqlite.db`.
pub fn default_db_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/data/ljh-sh/fmeta/sqlite.db"))
}

/// Open the DB at `path`, creating the parent directory and schema if needed,
/// and enabling WAL.
pub fn open(path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS files (
  path          TEXT PRIMARY KEY,
  depth         INTEGER,
  kind          TEXT,
  is_symlink    INTEGER,
  size          INTEGER,
  mime          TEXT,
  category      TEXT,
  encoding      TEXT,
  binary        INTEGER,
  width         INTEGER,
  height        INTEGER,
  duration_secs REAL,
  pages         INTEGER,
  tables        INTEGER,
  entries       INTEGER,
  columns       INTEGER,
  exif          TEXT,
  tags          TEXT,
  mtime         INTEGER,
  ctime         INTEGER,
  indexed_at    INTEGER
);
CREATE INDEX IF NOT EXISTS idx_files_category ON files(category);
CREATE INDEX IF NOT EXISTS idx_files_mime     ON files(mime);
CREATE INDEX IF NOT EXISTS idx_files_mtime    ON files(mtime);
";

/// Cached mtime for a path, if the row exists.
pub fn cached_mtime(conn: &Connection, path: &str) -> anyhow::Result<Option<i64>> {
    let v = conn
        .query_row("SELECT mtime FROM files WHERE path = ?", [path], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?;
    Ok(v)
}

/// Upsert (INSERT OR REPLACE) a file's metadata row.
pub fn upsert(conn: &Connection, m: &FileMeta, indexed_at: i64) -> anyhow::Result<()> {
    use rusqlite::params;
    conn.execute(
        "INSERT OR REPLACE INTO files \
         (path,depth,kind,is_symlink,size,mime,category,encoding,binary,width,height,duration_secs,\
          pages,tables,entries,columns,exif,tags,mtime,ctime,indexed_at) \
         VALUES (?,?,?,?, ?,?,?,?, ?,?,?,?, ?,?,?,?, ?,?,?,?, ?)",
        params![
            m.path,
            m.depth as i64,
            format!("{:?}", m.kind).to_ascii_lowercase(),
            m.is_symlink.unwrap_or(false),
            m.size.map(|x| x as i64),
            m.mime,
            m.category,
            m.encoding,
            m.binary,
            m.width.map(|x| x as i64),
            m.height.map(|x| x as i64),
            m.duration_secs,
            m.pages.map(|x| x as i64),
            m.tables.map(|x| x as i64),
            m.entries.map(|x| x as i64),
            m.columns.map(|x| x as i64),
            m.exif.as_ref().map(json),
            m.tags.as_ref().map(json),
            m.mtime,
            m.ctime,
            indexed_at,
        ],
    )?;
    Ok(())
}

fn json(m: &std::collections::BTreeMap<String, String>) -> String {
    serde_json::to_string(m).unwrap_or_else(|_| "{}".into())
}

/// Run arbitrary SQL and return (column names, rows of stringified values).
pub fn run_query(conn: &Connection, sql: &str) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
    use rusqlite::types::ValueRef;
    let mut stmt = conn.prepare(sql)?;
    let n = stmt.column_count();
    let cols: Vec<String> = (0..n)
        .map(|i| stmt.column_name(i).unwrap_or("").to_string())
        .collect();
    let rows = stmt
        .query_map([], |r| {
            (0..n)
                .map(|i| {
                    Ok(match r.get_ref(i)? {
                        ValueRef::Null => String::new(),
                        ValueRef::Integer(i) => i.to_string(),
                        ValueRef::Real(f) => f.to_string(),
                        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                        ValueRef::Blob(b) => format!("<blob {}B>", b.len()),
                    })
                })
                .collect::<rusqlite::Result<Vec<String>>>()
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((cols, rows))
}
