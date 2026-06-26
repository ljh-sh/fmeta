//! Metadata detection: size, mime type, text encoding, and (for images)
//! pixel dimensions and EXIF tags.
//!
//! For each visited file we read up to `sniff` bytes once and reuse the same
//! buffer for mime (via `infer`), encoding (via `chardetng`), and — for
//! images — dimensions (`imagesize`) and EXIF (`kamadak-exif`) detection.
//! Non-files (symlinks, directories) are skipped — they have no useful content
//! metadata.

use std::collections::BTreeMap;
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
    /// Coarse content category for quick agent decisions: `text`, `image`,
    /// `audio`, `video`, `archive`, `binary`, or `data`. Absent for non-files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Pixel width, for images (`imagesize`). Absent for non-images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Pixel height, for images (`imagesize`). Absent for non-images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// EXIF tags parsed from JPEG/TIFF/HEIF/etc. images (`kamadak-exif`).
    /// Map of tag name → display value. Absent when no EXIF is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exif: Option<BTreeMap<String, String>>,
    /// Page count, for PDF documents (`lopdf`). Absent for non-PDFs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    /// Duration in seconds, for audio/video (`lofty`). Absent for non-media.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    /// Media tags (artist/album/title/…) for audio (`lofty`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<BTreeMap<String, String>>,
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
        deep: bool,
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
            category: None,
            width: None,
            height: None,
            exif: None,
            pages: None,
            duration_secs: None,
            tags: None,
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

        meta.mime = mime.clone();
        meta.encoding = encoding;
        meta.binary = binary;
        meta.category = Some(categorize(mime.as_deref(), binary.unwrap_or(false)).to_string());

        // For images, read pixel dimensions and EXIF from the sniff buffer.
        if mime
            .as_deref()
            .map(|m| m.starts_with("image/"))
            .unwrap_or(false)
        {
            if let Some((w, h)) = image_dimensions(&buf) {
                meta.width = Some(w);
                meta.height = Some(h);
            }
            meta.exif = extract_exif(&buf);
        }

        // Whole-file ("deep") extractors: these read beyond the sniff buffer, so
        // they only run when not in --fast mode. `--fast` keeps the walk bounded
        // to the sniff read; the default (deep) trades IO for rich metadata.
        if deep {
            // PDF page count. `lopdf` reads the cross-reference table + page
            // tree; encrypted/malformed PDFs yield no count.
            if mime.as_deref() == Some("application/pdf") {
                meta.pages = pdf_page_count(path);
            }
            // Audio duration + tags. `lofty` reads the whole file.
            if mime
                .as_deref()
                .map(|m| m.starts_with("audio/"))
                .unwrap_or(false)
            {
                if let Some((secs, tags)) = audio_meta(path) {
                    meta.duration_secs = Some(secs);
                    meta.tags = if tags.is_empty() { None } else { Some(tags) };
                }
            }
        }
        meta
    }
}

/// Read audio duration (seconds) + a few common tags via `lofty`. Returns None
/// when the file can't be parsed (best-effort).
fn audio_meta(path: &Path) -> Option<(f64, BTreeMap<String, String>)> {
    use lofty::prelude::*;

    let mut f = fs::File::open(path).ok()?;
    let tagged = lofty::read_from(&mut f).ok()?;
    let secs = tagged.properties().duration().as_secs_f64();

    let mut tags = BTreeMap::new();
    if let Some(tag) = tagged.primary_tag() {
        for (key, value) in [
            ("artist", tag.artist()),
            ("album", tag.album()),
            ("title", tag.title()),
            ("genre", tag.genre()),
        ] {
            if let Some(v) = value {
                tags.insert(key.to_string(), v.to_string());
            }
        }
        if let Some(year) = tag.year() {
            tags.insert("year".to_string(), year.to_string());
        }
    }
    Some((secs, tags))
}

/// Count pages in a PDF via `lopdf`. Returns None for encrypted or malformed
/// PDFs (best-effort, like EXIF).
fn pdf_page_count(path: &Path) -> Option<u32> {
    let doc = lopdf::Document::load(path).ok()?;
    let count = doc.get_pages().len() as u32;
    if count == 0 {
        None
    } else {
        Some(count)
    }
}

/// Pixel dimensions from the sniff buffer via `imagesize`. Returns None for
/// non-images or when the sniff window is too short to read the header.
fn image_dimensions(buf: &[u8]) -> Option<(u32, u32)> {
    let size = imagesize::blob_size(buf).ok()?;
    Some((size.width as u32, size.height as u32))
}

/// A dump of EXIF tags from a JPEG/TIFF/HEIF/etc. image, as tag → display
/// value. Returns None when the buffer has no parseable EXIF.
fn extract_exif(buf: &[u8]) -> Option<BTreeMap<String, String>> {
    use std::io::Cursor;
    let exif = exif::Reader::new()
        .read_from_container(&mut Cursor::new(buf))
        .ok()?;
    let mut map = BTreeMap::new();
    for field in exif.fields() {
        // kamadak-exif wraps ASCII strings in quotes for display; strip a
        // single surrounding pair so consumers get the raw value.
        let mut val = field.display_value().to_string();
        if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
            val = val[1..val.len() - 1].to_string();
        }
        map.insert(field.tag.to_string(), val);
    }
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

/// Derive a coarse content category from the detected mime and binary flag.
/// Used as the `mime_hint` column so agents can filter ("just the images")
/// without parsing mime types themselves.
///
/// Recognised media/archive types win over the raw `binary` flag — a PNG is an
/// `image` even though it contains NUL bytes. `binary` is reserved for opaque
/// blobs that are neither text nor a known media/archive format.
fn categorize(mime: Option<&str>, binary: bool) -> &'static str {
    match mime {
        Some(m) if m.starts_with("image/") => "image",
        Some(m) if m.starts_with("audio/") => "audio",
        Some(m) if m.starts_with("video/") => "video",
        Some(m) if is_archive_mime(m) => "archive",
        Some(m) if m.starts_with("text/") || is_text_mime(m) => "text",
        None if !binary => "text",
        _ if binary => "binary",
        _ => "data",
    }
}

fn is_archive_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/zip"
            | "application/gzip"
            | "application/x-gzip"
            | "application/x-tar"
            | "application/x-xz"
            | "application/zstd"
            | "application/x-zstd"
            | "application/x-bzip2"
            | "application/x-7z-compressed"
            | "application/x-rar-compressed"
            | "application/java-archive"
            | "application/x-iso9660-image"
    )
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
