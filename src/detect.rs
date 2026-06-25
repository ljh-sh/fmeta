//! Metadata detection: size, mime type, text encoding.
//!
//! For each visited file we read up to `sniff` bytes once and reuse the same
//! buffer for both mime (via `infer`) and encoding (via `chardetng`)
//! detection. Non-files (symlinks, directories) are skipped — they have no
//! useful content metadata in v0.

use std::fs;
use std::io::Read;
use std::path::Path;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};

/// Per-file metadata collected by `fmeta`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileMeta {
    pub path: String,
    pub depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_symlink: Option<bool>,
    pub kind: EntryKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary: Option<bool>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

impl FileMeta {
    pub fn for_entry(
        path: &Path,
        depth: usize,
        is_symlink: bool,
        file_type: fs::FileType,
        sniff: usize,
        paths_only: bool,
    ) -> Self {
        let kind = if file_type.is_file() {
            EntryKind::File
        } else if file_type.is_dir() {
            EntryKind::Dir
        } else if is_symlink {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        };

        let mut meta = FileMeta {
            path: path.to_string_lossy().into_owned(),
            depth,
            is_symlink: if is_symlink { Some(true) } else { None },
            kind,
            size: None,
            mime: None,
            encoding: None,
            binary: None,
        };

        if paths_only || kind != EntryKind::File {
            return meta;
        }

        // Size: prefer metadata; fall back to None if unreadable.
        meta.size = fs::metadata(path).map(|m| m.len()).ok();

        // Sniff content for mime + encoding. Files we cannot open get no
        // content metadata (size may still be present).
        let Ok(mut f) = fs::File::open(path) else {
            return meta;
        };
        let mut buf = vec![0u8; sniff];
        let n = match f.read(&mut buf) {
            Ok(n) => n,
            Err(_) => return meta,
        };
        buf.truncate(n);
        if buf.is_empty() {
            return meta;
        }

        // Mime first — `infer` is purely signature based and very fast.
        let mime = infer::get(&buf).map(|t| t.mime_type().to_string());
        let is_text = mime.as_deref().map(is_text_mime).unwrap_or(false);

        // Encoding detection only makes sense for text. For unknown mime we
        // also try chardetng — many text files have no magic signature.
        let (encoding, binary) = if is_text || mime.is_none() {
            detect_encoding(&buf)
        } else {
            (None, Some(true))
        };

        meta.mime = mime;
        meta.encoding = encoding;
        meta.binary = binary;
        meta
    }
}

fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime == "application/json"
        || mime == "application/xml"
        || mime == "application/javascript"
        || mime == "application/x-sh"
        || mime == "application/x-yaml"
        || mime == "application/yaml"
        || mime.ends_with("+xml")
        || mime.ends_with("+json")
}

/// Returns `(encoding label, is_binary)`. `is_binary` is true when the buffer
/// contains a NUL byte (heuristic used by `git` and `grep -I`).
fn detect_encoding(buf: &[u8]) -> (Option<String>, Option<bool>) {
    let binary = buf.contains(&0u8);
    if binary {
        return (None, Some(true));
    }

    let mut det = EncodingDetector::new(Iso2022JpDetection::Allow);
    let _confidence = det.feed(buf, true);
    let enc = det.guess(None, Utf8Detection::Allow);
    let name = enc.name().to_string();
    // chardetng returns "utf-8" as a safe default; only report when there is
    // something non-trivial to say. We keep utf-8 as it is the most useful
    // signal for downstream AI consumers.
    (Some(name), Some(false))
}
