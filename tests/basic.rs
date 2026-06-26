use std::fs;
use std::path::Path;

/// Build a throwaway tree and verify fmeta walks it deterministically.
#[test]
fn walks_and_reports_metadata() {
    let tmp = tempfile_dir();
    let root = tmp.join("root");
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("hello.txt"), b"hello world\n").unwrap();
    fs::write(root.join("sub/binary.bin"), [0u8, 1, 2, 0, 255]).unwrap();
    fs::write(root.join("sub/empty"), b"").unwrap();

    let metas = collect(&root);

    // Every visited path should appear, sorted pre-order.
    let paths: Vec<&str> = metas.iter().map(|m| m.path.as_str()).collect();
    assert!(
        paths.iter().any(|p| p.ends_with("hello.txt")),
        "hello.txt missing: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.ends_with("binary.bin")),
        "binary.bin missing: {paths:?}"
    );

    let hello = metas
        .iter()
        .find(|m| m.path.ends_with("hello.txt"))
        .unwrap();
    assert_eq!(hello.kind, fmeta::detect::EntryKind::File);
    assert_eq!(hello.size, Some(b"hello world\n".len() as u64));
    assert_eq!(hello.encoding.as_deref(), Some("UTF-8"));
    assert_eq!(hello.binary, Some(false));
    assert_eq!(hello.category.as_deref(), Some("text"));

    let bin = metas
        .iter()
        .find(|m| m.path.ends_with("binary.bin"))
        .unwrap();
    // NUL byte => binary, no encoding reported.
    assert_eq!(bin.binary, Some(true));
    assert!(bin.encoding.is_none());
    assert_eq!(bin.category.as_deref(), Some("binary"));

    let empty = metas.iter().find(|m| m.path.ends_with("empty")).unwrap();
    assert!(
        empty.encoding.is_none(),
        "empty file should have no encoding"
    );
    assert!(empty.mime.is_none());
}

/// `--max-depth 0` should only list the root itself.
#[test]
fn respects_max_depth_zero() {
    let tmp = tempfile_dir();
    let root = tmp.join("root");
    fs::create_dir_all(root.join("a")).unwrap();
    fs::write(root.join("a/f.txt"), b"x").unwrap();

    let opts = fmeta::cli::Cli {
        paths: vec![root.clone()],
        max_depth: 0,
        all: false,
        no_ignore: false,
        follow_links: false,
        format: fmeta::cli::OutputFormat::Table,
        sniff: 8192,
        paths_only: false,
    };
    let entries = fmeta::traverse::walk(std::slice::from_ref(&root), &opts);
    assert_eq!(entries.len(), 1, "depth 0 must visit only the root");
}

fn collect(root: &Path) -> Vec<fmeta::detect::FileMeta> {
    let opts = fmeta::cli::Cli {
        paths: vec![root.to_path_buf()],
        max_depth: usize::MAX,
        all: true,
        no_ignore: false,
        follow_links: false,
        format: fmeta::cli::OutputFormat::Table,
        sniff: 8192,
        paths_only: false,
    };
    let entries = fmeta::traverse::walk(&[root.to_path_buf()], &opts);
    entries
        .iter()
        .map(|e| {
            fmeta::detect::FileMeta::for_entry(
                &e.path,
                e.depth,
                e.is_symlink,
                e.file_type,
                opts.sniff,
                opts.paths_only,
            )
        })
        .collect()
}

/// `.gitignore` patterns filter the default walk; `--no-ignore` restores them.
#[test]
fn gitignore_filters_by_default() {
    let tmp = tempfile_dir();
    let root = tmp.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(".gitignore"), "secret.txt\n").unwrap();
    fs::write(root.join("secret.txt"), b"top secret\n").unwrap();
    fs::write(root.join("keep.txt"), b"keep me\n").unwrap();

    let opts = fmeta::cli::Cli {
        paths: vec![root.clone()],
        max_depth: usize::MAX,
        all: false,
        no_ignore: false,
        follow_links: false,
        format: fmeta::cli::OutputFormat::Table,
        sniff: 8192,
        paths_only: false,
    };
    let paths: Vec<String> = fmeta::traverse::walk(std::slice::from_ref(&root), &opts)
        .into_iter()
        .map(|e| e.path.to_string_lossy().into_owned())
        .collect();
    assert!(
        !paths.iter().any(|p| p.ends_with("secret.txt")),
        "secret.txt should be gitignored away: {paths:?}"
    );
    assert!(paths.iter().any(|p| p.ends_with("keep.txt")));

    let mut no_ignore = opts.clone();
    no_ignore.no_ignore = true;
    let paths2: Vec<String> = fmeta::traverse::walk(std::slice::from_ref(&root), &no_ignore)
        .into_iter()
        .map(|e| e.path.to_string_lossy().into_owned())
        .collect();
    assert!(
        paths2.iter().any(|p| p.ends_with("secret.txt")),
        "secret.txt should appear with --no-ignore: {paths2:?}"
    );
}

/// TSV output: exactly six tab-separated columns, path last.
#[test]
fn tsv_has_six_columns_path_last() {
    let tmp = tempfile_dir();
    let root = tmp.join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("hello.txt"), b"hello world\n").unwrap();

    let metas = collect(&root);
    let mut buf = Vec::new();
    fmeta::output::render_to(&mut buf, &metas, fmeta::cli::OutputFormat::Tsv).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(!out.is_empty(), "tsv should emit rows");

    for line in out.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 6, "tsv row must have 6 columns: {line:?}");
        // `path` is always the last column.
        assert!(
            cols.last().unwrap().ends_with("hello.txt") || cols.last().unwrap().ends_with("root"),
            "last column must be a path: {:?}",
            cols.last()
        );
    }
}

/// Image files get pixel dimensions; EXIF is absent when not present.
#[test]
fn image_dimensions_and_exif() {
    let tmp = tempfile_dir();
    let root = tmp.join("root");
    fs::create_dir_all(&root).unwrap();

    // A minimal PNG (signature + IHDR) with width=3, height=2. `imagesize`
    // reads dimensions from the header; a real PNG body/CRC is not required.
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend_from_slice(&[0, 0, 0, 13]); // IHDR length
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&3u32.to_be_bytes()); // width
    png.extend_from_slice(&2u32.to_be_bytes()); // height
    png.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth, color type, comp, filter, interlace
    png.extend_from_slice(&[0, 0, 0, 0]); // dummy CRC
    fs::write(root.join("pic.png"), &png).unwrap();

    let metas = collect(&root);
    let pic = metas
        .iter()
        .find(|m| m.path.ends_with("pic.png"))
        .expect("pic.png missing");
    assert_eq!(pic.kind, fmeta::detect::EntryKind::File);
    assert_eq!(pic.mime.as_deref(), Some("image/png"));
    assert_eq!(pic.category.as_deref(), Some("image"));
    assert_eq!(pic.width, Some(3));
    assert_eq!(pic.height, Some(2));
    assert!(pic.exif.is_none(), "crafted PNG has no EXIF");
}

fn tempfile_dir() -> std::path::PathBuf {
    // Avoid pulling in the `tempfile` crate; use a process-unique dir.
    let dir = std::env::temp_dir().join(format!(
        "fmeta-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}
