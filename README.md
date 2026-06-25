# fmeta

[![CI](https://github.com/ljh-sh/fmeta/actions/workflows/ci.yml/badge.svg)](https://github.com/ljh-sh/fmeta/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/ljh-sh/fmeta/badge)](https://scorecard.dev/)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

> A `find` alternative that emits files with rich metadata — mime type, text encoding, size — designed to be easy for humans and AI agents to consume.

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
# Walk the current directory, print a table.
fmeta

# Limit depth, show hidden files, emit JSON Lines.
fmeta --max-depth 2 --all --format json path/to/dir

# Skip detection entirely (paths only, fastest).
fmeta --paths-only path/to/dir

# Read more bytes per file for better detection accuracy.
fmeta --sniff 16384 path/to/dir
```

### Output formats

**Table (default)**

```
depth  size   kind  mime  encoding  path
-----  -----  ----  ----  --------  ----
1      12104  file  -     UTF-8     ./Cargo.lock
1      864    file  -     UTF-8     ./Cargo.toml
1      -      dir   -     -         ./src
```

**JSON Lines (`--format json`)** — one object per line, stable schema, safe for `jq`, `grep`, or direct LLM consumption:

```json
{"path":"src/cli.rs","depth":1,"kind":"file","size":1684,"encoding":"UTF-8","binary":false}
```

Fields:

| field     | always present | notes                                                   |
| --------- | -------------- | ------------------------------------------------------- |
| `path`    | yes            | as given on the command line / walk                     |
| `depth`   | yes            | 0 = root                                                |
| `kind`    | yes            | `file` \| `dir` \| `symlink` \| `other`                 |
| `size`    | files only     | bytes; `None` if unreadable                             |
| `mime`    | files only     | from `infer`; `None` for plain text without a signature |
| `encoding`| text files     | from `chardetng`; absent for binary or empty files      |
| `binary`  | files only     | `true` if a NUL byte is found in the sniff window       |
| `is_symlink` | symlinks   | omitted when `false`                                    |

### Options

| flag                | description                                              |
| ------------------- | -------------------------------------------------------- |
| `-d, --max-depth N` | maximum recursion depth (default: unlimited)             |
| `-a, --all`         | include hidden files and directories                     |
| `-L, --follow`      | follow symbolic links                                    |
| `-o, --format F`    | `table` (default) or `json`                              |
| `--sniff BYTES`     | bytes read per file for detection (default: 8192)        |
| `--paths-only`      | skip detection, emit paths only                          |

## Scope (v0)

In scope: directory traversal, size, mime detection, text encoding detection, table + JSON output.

Out of scope (future): image / audio / video dimensions, EXIF / ID3 / PDF metadata, network and remote files. See [docs/design.md](docs/design.md) for the roadmap.

## License

Apache-2.0. See [LICENSE](LICENSE).
