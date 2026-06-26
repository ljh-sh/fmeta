# fmeta — design

> Status: v0.7. This document records the architectural decisions and the roadmap for v1+.

## Goals

A `find` alternative whose output is immediately useful for AI agents and shell pipelines. `find` lists paths; the consumer then has to spawn `file`, `xdetect`, or a script per entry to learn anything about each file. `fmeta` walks and detects in a single process, emitting stable metadata columns alongside each path.

## Non-goals (v0)

- Image / audio / video dimension extraction.
- EXIF / ID3 / PDF / Office metadata (tracked in a follow-up issue — deferred from the v0.2 batch).
- Network and remote files.

## Architecture

```
src/
├── main.rs        # thin entry: parse args, dispatch, exit code
├── lib.rs         # orchestrates walk -> detect -> render
├── cli.rs         # clap argument model
├── traverse.rs    # `ignore`-based walker (gitignore + hidden handling)
├── detect.rs      # size + mime (infer) + encoding (chardetng) + category per file
└── output.rs      # TSV (default), table, and JSON Lines renderers
```

### Why the `ignore` crate for traversal?

v0.1 shipped a small hand-rolled walker to keep the dependency surface minimal. v0.2 adds `.gitignore` / `.ignore` / global-gitignore / hidden-file honoring (issue #7), and correctly implementing gitignore semantics (negation patterns, parent/global files, `.git` special-casing) by hand is error-prone. `ignore` — the engine ripgrep uses, `MIT OR Unlicense` — does all of this correctly and is well-vetted. The cost is a larger dependency tree (`regex`, `globset`, `walkdir`, …) and a bigger binary, which is the deliberate v0.2 trade for correctness. If parallelism becomes a bottleneck later, `ignore` already offers `build_parallel()`.

### Why `infer` for mime?

`infer` is zero-dependency, pure-Rust, and signature-based. It correctly identifies common binary formats (PNG, JPEG, ELF, Mach-O, ZIP, gzip, PDF). Its limitation is that plain-text formats (`.txt`, `.md`, `.rs`, `.py`) have no magic signature, so `infer` returns `None` for them. This is why we pair it with `chardetng`: when mime is `None` or known-text, we attempt encoding detection instead. The combination covers the two questions agents actually ask: "is this binary?" and "if text, what encoding?".

### Why `chardetng` for encoding?

`chardetng` is the same engine Firefox uses, Apache-2.0, and already proven in the ljh-sh `chardet` repo. It gives a best-effort encoding label (`UTF-8`, `Shift_JIS`, `windows-1252`, …). The `binary` flag uses the NUL-byte heuristic (`git` and `grep -I` use the same rule), which is robust and cheap.

### Output schema

Three formats, one underlying schema:

- **TSV** (default) — `size  kind  encoding  mime  mime_hint  path`, tab-separated, no header. `path` is last so paths with spaces don't break `awk`/`cut`. The agent/pipeline format.
- **Table** — aligned columns (adds `depth`) for human reading.
- **JSON Lines** — one object per line, stable field order, optional fields omitted rather than null. Chosen over a JSON array so output streams without buffering the whole tree in memory.

`mime_hint` is a coarse category (`text` / `image` / `audio` / `video` / `archive` / `binary` / `data`) derived from the mime + the NUL-binary flag, so agents can filter ("just the images") without parsing mime types. For `image/*` files, v0.3 additionally emits pixel `width`/`height` (via `imagesize`, header-only) and an `exif` map (via `kamadak-exif`) in JSON, and a `dims` column (`WxH`) in the table; the TSV schema is unchanged. Field selection favours "every field answers a real question". We deliberately do not emit `mtime`/`ctime`/`perms`: those are `stat` territory and trivially obtainable; fmeta focuses on content-derived metadata that is expensive to recompute.

## Design decisions

| # | decision | rationale |
|---|----------|-----------|
| 1 | `ignore` crate for traversal (v0.2; was own walker in v0.1) | correct gitignore/hidden handling outweighs the larger dep tree; hand-rolling gitignore is error-prone |
| 2 | `infer` + `chardetng`, not `file` lib | pure Rust, single binary, no libmagic C dependency |
| 3 | NUL-byte heuristic for `binary` | matches `git`/`grep -I`, robust and cheap |
| 4 | JSON Lines, not JSON array | streams without buffering; safe for `jq`/`grep` |
| 5 | `--sniff` default 8192 | enough for reliable detection on most files, cheap IO |
| 6 | Pre-order, sorted traversal | deterministic output, reproducible across runs |
| 7 | Emit directories as rows too | mirrors `find`, lets consumers see tree structure |
| 8 | No `mtime`/`perms` | `stat` territory; stay content-focused |
| 9 | TSV default output (v0.2) | agent/pipeline-first; `path` last so spaces don't break parsing |
| 10 | `mime_hint` category column (v0.2) | coarse filterable type without mime parsing; honors gitignore via `--no-ignore` toggle |
| 11 | `require_git(false)` (v0.2) | honour `.gitignore` files even outside git worktrees |
| 12 | Image dims + EXIF from the sniff buffer (v0.3) | `imagesize` (header-only, no decode) + `kamadak-exif` (MIT); gated to `image/*` so non-images pay nothing. TSV stays 6-col; dims/EXIF surface in JSON + the table `dims` column |
| 13 | Media metadata not in TSV (v0.3) | keep the default TSV schema stable post-v0.2; agents wanting dims/EXIF use `--format json` |
| 14 | PDF page count via `lopdf` (v0.4) | documents are common opaque files; `lopdf` reads the xref + page tree (not content streams), cheap relative to size. `pages` field is JSON-only; encrypted/malformed PDFs yield none |
| 15 | **Default is DEEP; `--fast` opts out** (v0.5) | the default walk reads whole files for rich metadata (PDF pages, audio duration/tags); `--fast` keeps it bounded to `--sniff` bytes. Inverts the usual `--deep` opt-in — rich metadata out of the box is the point of the tool. Binary size is not a constraint (#13) |
| 16 | Audio duration + tags via `lofty` (v0.5) | `lofty` (MIT/Apache) reads the whole file → `duration_secs` + `tags` (artist/album/title/genre/year) for `audio/*`; JSON-only, gated behind `!opts.fast` |
| 17 | Video dims + duration via `mp4parse` (v0.6) | `mp4parse` (MPL-2.0, Mozilla) parses ISO BMFF (mp4/m4v/mov); `tkhd.width/height` are 16.16 fixed-point (decode with `>> 16`). MPL-2.0 = file-level weak copyleft, fine as an unmodified dep (deny.toml now allows MPL-2.0). Reuses `width`/`height`/`duration_secs`; gated behind `!opts.fast` |
| 18 | mtime/ctime, CSV columns, Office core props, archive/EPUB counts (v0.7) | mtime (`fs::metadata`, also the index-DB cache key) + ctime = **no dep, Δ0 KB**. CSV/TSV column count = ext-based, no dep. Office `docx`/`xlsx`/`pptx` core props (title/author/created/modified) + zip/tar/EPUB spine entry counts via `zip` (deflate-only) + `tar` + `flate2` — ~+34 KB stripped combined. ext-based detection for magic-less formats (CSV, Office, EPUB) since infer reports Office/EPUB as `application/zip` |

## Roadmap

- **v0.1**: traversal, size, mime, encoding, table + JSON.
- **v0.2**: TSV default output + `mime_hint` category, `.gitignore`/`.ignore` honoring via the `ignore` crate (`--no-ignore` to disable), `-a/--all` for hidden files.
- **v0.3**: image pixel dimensions (`imagesize`) + EXIF tags (`kamadak-exif`) for `image/*` files — surfaced in JSON and the table `dims` column; TSV unchanged.
- **v0.4**: PDF page count via `lopdf` — `pages` field (JSON) for `application/pdf`.
- **v0.5**: audio duration + tags via `lofty`; **default is deep, `--fast` opts out** (#13).
- **v0.6**: video dimensions + duration via `mp4parse` (MPL-2.0 allowed in deny.toml).
- **v0.7** (this release): mtime/ctime, CSV/TSV column count, Office core props (docx/xlsx/pptx), archive entry count (zip/tar/tar.gz), EPUB spine count.
- **v0.8**: SQLite introspection + fmeta's own index DB (bundled via `rusqlite`); see #9.
- **v1.0**: font/parquet metadata, parallel traversal. Remaining metadata tracked in #9.

## Security notes

`fmeta` only reads files (open + read up to `--sniff` bytes), never writes or executes. Following symlinks is opt-in (`-L`). Detection reads a bounded prefix of each file, so a hostile tree cannot cause unbounded IO. See [SECURITY.md](../SECURITY.md) for vulnerability reporting.
