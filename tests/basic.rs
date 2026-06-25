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

    let bin = metas
        .iter()
        .find(|m| m.path.ends_with("binary.bin"))
        .unwrap();
    // NUL byte => binary, no encoding reported.
    assert_eq!(bin.binary, Some(true));
    assert!(bin.encoding.is_none());

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
