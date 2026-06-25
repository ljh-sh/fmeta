# fmeta — design

> Status: v0 working. This document records the architectural decisions behind v0 and the roadmap for v1+.

## Goals

A `find` alternative whose output is immediately useful for AI agents and shell pipelines. `find` lists paths; the consumer then has to spawn `file`, `xdetect`, or a script per entry to learn anything about each file. `fmeta` walks and detects in a single process, emitting stable metadata columns alongside each path.

## Non-goals (v0)

- Image / audio / video dimension extraction.
- EXIF / ID3 / PDF / Office metadata.
- Network and remote files.
- `gitignore` / `.ignore` semantics (a future `--no-ignore` mode is possible but not in v0).

## Architecture

```
src/
├── main.rs        # thin entry: parse args, dispatch, exit code
├── lib.rs         # orchestrates walk -> detect -> render
├── cli.rs         # clap argument model
├── traverse.rs    # own recursive walker (no jwalk / ignore dependency)
├── detect.rs      # size + mime (infer) + encoding (chardetng) per file
└── output.rs      # table and JSON Lines renderers
```

### Why own traversal?

`jwalk` and `ignore` are excellent, but for v0 we only need pre-order depth-limited traversal with hidden-file filtering and optional symlink following. A ~70-line walker keeps the dependency surface minimal (important for the "single-binary, low cold-start" ethos of ljh-sh tools) and gives full control over ordering and error handling. If parallelism becomes a bottleneck, swapping in `jwalk` later is a localized change.

### Why `infer` for mime?

`infer` is zero-dependency, pure-Rust, and signature-based. It correctly identifies common binary formats (PNG, JPEG, ELF, Mach-O, ZIP, gzip, PDF). Its limitation is that plain-text formats (`.txt`, `.md`, `.rs`, `.py`) have no magic signature, so `infer` returns `None` for them. This is why we pair it with `chardetng`: when mime is `None` or known-text, we attempt encoding detection instead. The combination covers the two questions agents actually ask: "is this binary?" and "if text, what encoding?".

### Why `chardetng` for encoding?

`chardetng` is the same engine Firefox uses, Apache-2.0, and already proven in the ljh-sh `chardet` repo. It gives a best-effort encoding label (`UTF-8`, `Shift_JIS`, `windows-1252`, …). The `binary` flag uses the NUL-byte heuristic (`git` and `grep -I` use the same rule), which is robust and cheap.

### Output schema

Two formats, one schema:

- **Table** — aligned columns for human reading.
- **JSON Lines** — one object per line, stable field order, optional fields omitted rather than null. Chosen over a JSON array so output streams without buffering the whole tree in memory.

Field selection favours "every field answers a real question". We deliberately do not emit `mtime`/`ctime`/`perms` in v0: those are `stat` territory and trivially obtainable; v0 focuses on content-derived metadata that is expensive to recompute.

## Design decisions

| # | decision | rationale |
|---|----------|-----------|
| 1 | Own walker, not `jwalk`/`ignore` | minimal deps, full control over ordering and error handling for v0 |
| 2 | `infer` + `chardetng`, not `file` lib | pure Rust, single binary, no libmagic C dependency |
| 3 | NUL-byte heuristic for `binary` | matches `git`/`grep -I`, robust and cheap |
| 4 | JSON Lines, not JSON array | streams without buffering; safe for `jq`/`grep` |
| 5 | `--sniff` default 8192 | enough for reliable detection on most files, cheap IO |
| 6 | Pre-order, sorted traversal | deterministic output, reproducible across runs |
| 7 | Emit directories as rows too | mirrors `find`, lets consumers see tree structure |
| 8 | No `mtime`/`perms` in v0 | `stat` territory; v0 stays content-focused |

## Roadmap

- **v0.1** (this release): traversal, size, mime, encoding, table + JSON.
- **v0.2**: image dimensions (width/height) via a pluggable extractor trait; optional `--extractors` selection.
- **v0.3**: audio duration, video dimensions.
- **v1.0**: document metadata (PDF page count, Office properties), `gitignore` awareness, parallel traversal.

## Security notes

`fmeta` only reads files (open + read up to `--sniff` bytes), never writes or executes. Following symlinks is opt-in (`-L`). Detection reads a bounded prefix of each file, so a hostile tree cannot cause unbounded IO. See [SECURITY.md](../SECURITY.md) for vulnerability reporting.
