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
    /// Last-modified time, Unix epoch seconds (`fs::metadata`). Also the cache
    /// key for fmeta's index DB (a file whose mtime is unchanged is reused).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<i64>,
    /// Creation (birth) time, Unix epoch seconds (`fs::metadata`). Absent on
    /// filesystems that don't record birth time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctime: Option<i64>,
    /// Column count for CSV/TSV (delimiter-separated first row). Naive count;
    /// does not handle quoted delimiters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<usize>,
    /// Number of contained entries for archives (zip / tar / tar.gz) — and the
    /// internal file count for Office docs (which are zip containers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<usize>,
    /// Number of user tables in a SQLite database (`rusqlite`, read-only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<usize>,
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
            mtime: None,
            ctime: None,
            columns: None,
            entries: None,
            tables: None,
        };

        // mtime/ctime apply to every entry (files and dirs alike); fetch the
        // metadata once. (Size + content detection below reuse this for files.)
        if let Ok(m) = fs::metadata(path) {
            meta.mtime = epoch(m.modified());
            meta.ctime = epoch(m.created());
            if kind == EntryKind::File {
                meta.size = Some(m.len());
            }
        }

        if paths_only || kind != EntryKind::File {
            return meta;
        }

        // Sniff content for mime + encoding. Files we cannot open get no
        // content metadata (size/mtime may still be present from the stat above).
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

        // CSV/TSV column count: extension-based (no magic signature). Sniff-
        // bounded, so always-on (not a deep extractor).
        if meta.columns.is_none() {
            meta.columns = csv_columns(path, &buf);
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
            // Video dimensions + duration (`mp4parse`, MPL-2.0). Reads the
            // whole file. ISO BMFF only (mp4/m4v/mov); mkv/webm yield none.
            if mime
                .as_deref()
                .map(|m| m.starts_with("video/"))
                .unwrap_or(false)
            {
                if let Some((w, h, secs)) = video_meta(path) {
                    meta.width = Some(w);
                    meta.height = Some(h);
                    meta.duration_secs = secs;
                }
            }
            // Office Open XML (docx/xlsx/pptx): ZIP containers whose
            // docProps/core.xml holds Dublin Core properties. Extension-based
            // (infer reports them as application/zip).
            if meta.tags.is_none() {
                if let Some(tags) = office_meta(path) {
                    meta.tags = Some(tags);
                }
            }
            // EPUB spine count (reading-order length, ≈ pages) — checked before
            // the generic archive count so an .epub reports spine items, not the
            // raw zip entry count. Reuses zip.
            if meta.entries.is_none() {
                meta.entries = epub_spine_count(path);
            }
            // Archive entry count (zip / tar / tar.gz); also the internal file
            // count of Office docs (zip containers).
            if meta.entries.is_none() {
                meta.entries = archive_entries(path, mime.as_deref());
            }
            // SQLite user-table count (read-only open via `rusqlite`).
            if meta.tables.is_none() && is_sqlite(&buf) {
                meta.tables = sqlite_table_count(path);
            }
            // Font family / full name (ttf/otf/ttc via `ttf-parser`).
            if meta.tags.is_none() {
                if let Some(tags) = font_meta(path) {
                    meta.tags = Some(tags);
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

/// Read video pixel dimensions + duration (seconds) via `mp4parse` (ISO BMFF:
/// mp4/m4v/mov). `tkhd.width`/`height` are 16.16 fixed-point. Returns None for
/// non-ISO-BMFF video (mkv/webm) or unparseable files (best-effort).
fn video_meta(path: &Path) -> Option<(u32, u32, Option<f64>)> {
    let mut f = fs::File::open(path).ok()?;
    let context = mp4parse::read_mp4(&mut f).ok()?;
    let track = context
        .tracks
        .iter()
        .find(|t| matches!(t.track_type, mp4parse::TrackType::Video))?;
    let tkhd = track.tkhd.as_ref()?;
    let width = tkhd.width >> 16; // 16.16 fixed-point → pixels
    let height = tkhd.height >> 16;

    // Duration: prefer the movie-scaled edited duration; fall back to the
    // track's own timescale.
    let secs = match (&track.edited_duration, &context.timescale) {
        (Some(d), Some(ts)) => Some(d.0 as f64 / ts.0 as f64),
        _ => match (&track.duration, &track.timescale) {
            (Some(d), Some(ts)) => Some(d.0 as f64 / ts.0 as f64),
            _ => None,
        },
    };
    Some((width, height, secs))
}

/// Office Open XML core properties (docx/xlsx/pptx) via `zip`. The container's
/// `docProps/core.xml` holds Dublin Core fields (title/author/created/modified).
/// Extension-based: infer reports these files as `application/zip`.
fn office_meta(path: &Path) -> Option<BTreeMap<String, String>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())?;
    if !matches!(ext.as_str(), "docx" | "xlsx" | "pptx") {
        return None;
    }
    let mut za = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let mut buf = Vec::new();
    za.by_name("docProps/core.xml")
        .ok()?
        .read_to_end(&mut buf)
        .ok()?;
    let xml = String::from_utf8_lossy(&buf);
    let mut tags = BTreeMap::new();
    for (key, tag) in [
        ("title", "dc:title"),
        ("author", "dc:creator"),
        ("created", "dcterms:created"),
        ("modified", "dcterms:modified"),
        ("last_modified_by", "cp:lastModifiedBy"),
        ("language", "dc:language"),
    ] {
        if let Some(v) = xml_tag_text(&xml, tag) {
            tags.insert(key.to_string(), v);
        }
    }
    if tags.is_empty() {
        None
    } else {
        Some(tags)
    }
}

/// Extract the inner text of the first `<tag ...>text</tag>` occurrence. Naive
/// but sufficient for the small, well-structured docProps/core.xml.
fn xml_tag_text(xml: &str, tag: &str) -> Option<String> {
    let start = xml.find(&format!("<{tag}"))?;
    let after_open = xml[start..].find('>')? + start + 1;
    let close = xml[after_open..].find('<')? + after_open;
    let val = xml[after_open..close].trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// Extract an attribute value from the first `<tag ... attr="value" ...>`
/// occurrence (e.g. `full-path` on `<rootfile>` in EPUB container.xml).
fn xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let open = format!("<{tag}");
    // Find `<tag` followed by a tag-name delimiter (space/>//tab/nl), so that
    // `<rootfile` doesn't match `<rootfiles>`.
    let mut search = 0;
    let start = loop {
        let s = xml[search..].find(&open)? + search;
        let next = xml.as_bytes().get(s + open.len()).copied();
        if matches!(next, Some(b' ' | b'>' | b'/' | b'\t' | b'\n')) {
            break s;
        }
        search = s + open.len();
    };
    let end = xml[start..].find('>')? + start;
    let slice = &xml[start..end];
    let needle = format!("{attr}=\"");
    let a = slice.find(&needle)? + needle.len();
    let b = slice[a..].find('"')? + a;
    Some(slice[a..b].to_string())
}

/// Read a zip entry into a buffer.
fn zip_read(za: &mut zip::ZipArchive<fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    za.by_name(name).ok()?.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// EPUB spine count (reading-order length, the closest thing EPUB has to a
/// page count). Follows META-INF/container.xml → OPF `rootfile` → counts
/// `<itemref>` in the OPF `<spine>`. Extension-based (.epub).
fn epub_spine_count(path: &Path) -> Option<usize> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    if ext != "epub" {
        return None;
    }
    let mut za = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let container =
        String::from_utf8_lossy(&zip_read(&mut za, "META-INF/container.xml")?).into_owned();
    let opf_path = xml_attr(&container, "rootfile", "full-path")?;
    let opf = String::from_utf8_lossy(&zip_read(&mut za, &opf_path)?).into_owned();
    Some(opf.matches("<itemref").count())
}

/// Number of contained entries for an archive: zip (`za.len()`), tar, or
/// tar.gz (gunzipped then counted). Returns None for other/unparseable.
fn archive_entries(path: &Path, mime: Option<&str>) -> Option<usize> {
    match mime {
        Some("application/zip") => {
            Some(zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?.len())
        }
        Some("application/x-tar") => tar_entries(path, false),
        Some("application/gzip") if is_tar_gz(path) => tar_entries(path, true),
        _ => None,
    }
}

fn is_tar_gz(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_lowercase())
        .unwrap_or_default();
    name.ends_with(".tar.gz") || name.ends_with(".tgz")
}

/// Count entries in a (optionally gzipped) tar archive.
fn tar_entries(path: &Path, gz: bool) -> Option<usize> {
    let file = fs::File::open(path).ok()?;
    let reader: Box<dyn std::io::Read> = if gz {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries().ok()?;
    Some(entries.filter_map(Result::ok).count())
}

/// SQLite databases start with the 16-byte magic `SQLite format 3\0`.
fn is_sqlite(buf: &[u8]) -> bool {
    buf.starts_with(b"SQLite format 3\0")
}

/// Count user tables in a SQLite DB via `rusqlite`, opened read-only. Returns
/// None for encrypted/corrupt DBs. Internal `sqlite_%` tables are excluded.
fn sqlite_table_count(path: &Path) -> Option<usize> {
    use rusqlite::OpenFlags;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = rusqlite::Connection::open_with_flags(path, flags).ok()?;
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .ok()?;
    Some(n as usize)
}

/// Font family + full name for ttf/otf/ttc via `ttf-parser`. Extension-based.
fn font_meta(path: &Path) -> Option<BTreeMap<String, String>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())?;
    if !matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
        return None;
    }
    let data = fs::read(path).ok()?;
    let face = ttf_parser::Face::parse(&data, 0).ok()?;
    let mut tags = BTreeMap::new();
    for name in face.names() {
        if let Some(s) = name.to_string() {
            match name.name_id {
                1 => {
                    tags.entry("family".to_string()).or_insert(s);
                }
                4 => {
                    tags.entry("full_name".to_string()).or_insert(s);
                }
                _ => {}
            }
        }
    }
    if tags.is_empty() {
        None
    } else {
        Some(tags)
    }
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

/// Convert a `SystemTime` from `fs::metadata` to Unix epoch seconds. None for
/// times before the epoch or when the filesystem reports none.
fn epoch(t: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    let t = t.ok()?;
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => Some(d.as_secs() as i64),
        Err(e) => Some(-(e.duration().as_secs() as i64)),
    }
}

/// Column count for `.csv`/`.tsv` files: count delimiter-separated fields in
/// the first line of the sniff buffer. Naive — does not handle quoted
/// delimiters. Returns None for other extensions or empty files.
fn csv_columns(path: &Path, buf: &[u8]) -> Option<usize> {
    let sep: u8 = match path
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase()
        .as_str()
    {
        "csv" => b',',
        "tsv" => b'\t',
        _ => return None,
    };
    let first_line = buf
        .split(|&b| b == b'\n')
        .next()
        .filter(|l| !l.is_empty())?;
    Some(first_line.iter().filter(|&&b| b == sep).count() + 1)
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
