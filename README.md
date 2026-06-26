# fmeta

[![CI](https://github.com/ljh-sh/fmeta/actions/workflows/ci.yml/badge.svg)](https://github.com/ljh-sh/fmeta/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/ljh-sh/fmeta/badge)](https://scorecard.dev/)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

> A `find` alternative that emits files with rich metadata — size, kind, encoding, mime, a coarse type hint — gitignore-aware, and designed to be easy for humans and AI agents to consume.

**fmeta** walks one or more directories and prints each visited file alongside detected metadata columns. Think of it as `find` crossed with a lightweight `file`: the same deterministic walk, but every line already carries the metadata you would otherwise have to spawn a second tool for.

## Why

`find` lists paths. To know whether a file is text, what encoding it uses, or what mime type it has, you pipe into `file`, `xdetect`, or a script — once per entry, spawning a process for every file. `fmeta` does the walk and the detection in a single pass, so the output is immediately useful for:

- piping into `awk` / `jq` for filtering
- feeding an LLM agent a compact inventory of a tree
- auditing a directory for binary files or non-UTF-8 encodings

## Install

### Cargo

```bash
cargo install fmeta
```

### Build from source

Requires Rust 1.74+.

```bash
git clone https://github.com/ljh-sh/fmeta
cd fmeta
cargo build --release
# binary: target/release/fmeta
```

### Direct binary

Download a prebuilt binary from the [releases page](https://github.com/ljh-sh/fmeta/releases).

## Usage

```bash
# Walk the current directory, emit one TSV row per file (default).
fmeta

# Limit depth, show hidden files, emit JSON Lines.
fmeta --max-depth 2 --all --format json path/to/dir

# Human-readable aligned table.
fmeta --format table path/to/dir

# Disable .gitignore filtering; show everything.
fmeta --no-ignore path/to/dir

# Skip detection entirely (paths only, fastest).
fmeta --paths-only path/to/dir

# Read more bytes per file for better detection accuracy.
fmeta --sniff 16384 path/to/dir
```

### Output formats

**TSV (default)** — `size  kind  encoding  mime  mime_hint  path`, tab-separated, no header. `path` is last so paths with spaces don't break `awk`/`cut`. The agent/pipeline format.

```
-	-	-	-	-	./src
12	file	UTF-8	-	text	./Cargo.toml
17	file	-	image/png	image	./logo.png
```

**Table (`--format table`)** — aligned columns (adds `depth` and `dims`) for human reading:

```
depth  size  kind  mime       encoding  mime_hint  dims     path
-----  ----  ----  ---------  --------  ---------  -------  ----
1      12    file  -          UTF-8     text       -        ./Cargo.toml
1      17    file  image/png  -         image      256x256  ./logo.png
1      -     dir   -          -         -          -        ./src
```

**JSON Lines (`--format json`)** — one object per line, stable schema, safe for `jq`, `grep`, or direct LLM consumption. Images additionally carry `width`, `height`, and an `exif` map:

```json
{"path":"src/cli.rs","depth":1,"kind":"file","size":1684,"encoding":"UTF-8","binary":false,"category":"text"}
{"path":"logo.png","depth":1,"kind":"file","size":12345,"mime":"image/png","binary":true,"category":"image","width":256,"height":256}
```

Fields:

| field        | always present | notes                                                          |
| ------------ | -------------- | -------------------------------------------------------------- |
| `path`       | yes            | as given on the command line / walk                            |
| `depth`      | yes            | 0 = root                                                       |
| `kind`       | yes            | `file` \| `dir` \| `symlink` \| `other`                        |
| `size`       | files only     | bytes; `None` if unreadable                                    |
| `mime`       | files only     | from `infer`; `None` for plain text without a signature        |
| `encoding`   | text files     | from `chardetng`; absent for binary or empty files             |
| `binary`     | files only     | `true` if a NUL byte is found in the sniff window              |
| `category`   | files only     | `mime_hint`: `text` \| `image` \| `audio` \| `video` \| `archive` \| `binary` \| `data` |
| `width`      | images, video  | pixel width (`imagesize` / `mp4parse`)                         |
| `height`     | images, video  | pixel height (`imagesize` / `mp4parse`)                        |
| `exif`       | images         | EXIF tag → value map (`kamadak-exif`); absent when none        |
| `pages`      | PDFs           | page count (`lopdf`); absent for encrypted/malformed PDFs      |
| `duration_secs` | audio, video | duration in seconds (`lofty` / `mp4parse`)                  |
| `tags`       | audio, Office, fonts | audio: artist/album/title/… (`lofty`); Office: title/author/created/modified; fonts: family/full_name (`ttf-parser`) |
| `columns`    | CSV/TSV        | field count of the first row (naive)                           |
| `entries`    | archives, EPUB | zip/tar/tar.gz entry count; EPUB spine count; Office internal file count |
| `tables`     | SQLite         | user-table count (`rusqlite`, read-only)                        |
| `mtime`      | yes            | last-modified, Unix epoch seconds (also the index-DB cache key) |
| `ctime`      | yes            | creation/birth time, Unix epoch seconds (absent if unsupported) |
| `is_symlink` | symlinks       | omitted when `false`                                           |

### Options

| flag                | description                                              |
| ------------------- | -------------------------------------------------------- |
| `-d, --max-depth N` | maximum recursion depth (default: unlimited)             |
| `-a, --all`         | include hidden files and directories                     |
| `--no-ignore`       | disable `.gitignore` / `.ignore` filtering               |
| `-L, --follow`      | follow symbolic links                                    |
| `-o, --format F`    | `tsv` (default), `table`, or `json`                      |
| `--sniff BYTES`     | bytes read per file for detection (default: 8192)        |
| `--paths-only`      | skip detection, emit paths only                          |
| `--fast`            | bounded walk: skip whole-file extractors (PDF pages, audio) — the default is "deep" |
| `--index [PATH...]` | **index mode**: walk + upsert metadata into the index DB (incremental via mtime) |
| `--sql SQL`         | **query mode**: run raw SQL over the index DB, print rows as TSV |
| `--db PATH`         | override the index DB path (default `~/.local/data/ljh-sh/fmeta/sqlite.db`) |

## Database mode

fmeta is also a **file metadata database**. Build an index of a tree, then query it with raw SQL — no re-walk, no `jq`.

```sh
# Index a tree into the default DB (~/.local/data/ljh-sh/fmeta/sqlite.db).
fmeta --index path/to/dir

# Re-indexing is incremental: files whose mtime is unchanged are skipped.
fmeta --index path/to/dir      # -> "indexed 3 (cached 2594)"

# Query with raw SQL (TSV output).
fmeta --sql "SELECT path,size FROM files WHERE category='image' ORDER BY size DESC LIMIT 20"
fmeta --sql "SELECT mime,count(*) FROM files GROUP BY mime ORDER BY 2 DESC"
fmeta --sql "SELECT path,pages FROM files WHERE pages > 100"
```

The schema is the `files` table — one row per indexed file (absolute path as primary key), columns mirror the fields above (`size`, `mime`, `category`, `encoding`, `width`, `height`, `duration_secs`, `pages`, `tables`, `entries`, `mtime`, `ctime`, …; `exif`/`tags` as JSON text). WAL mode enables concurrent readers. Use `--db PATH` to target a different DB.

## Scope

In scope: directory traversal (gitignore-aware), rich per-file metadata (size/mime/encoding/image dims+EXIF/PDF pages/audio tags+duration/video dims+duration/Office props/archives/EPUB/SQLite tables/font/CSV cols/mtime/ctime), TSV + table + JSON output, **and a global SQLite index (`--index`/`--sql`)**.

Out of scope (future): `--prune` (drop deleted files from index), parallel traversal, mkv/webm, network/remote files, parquet (→ duckdb). See [docs/design.md](docs/design.md) for the roadmap.

## License

Apache-2.0. See [LICENSE](LICENSE).
