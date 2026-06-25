# fmeta

[![CI](https://github.com/ljh-sh/fmeta/actions/workflows/ci.yml/badge.svg)](https://github.com/ljh-sh/fmeta/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/ljh-sh/fmeta/badge)](https://scorecard.dev/)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

> `find` 的替代品 —— 输出带丰富元数据（大小、类型、编码、mime、粗粒度类型提示）的文件列表，感知 .gitignore，专为人和 AI agent 友好而设计。

**fmeta** 遍历一个或多个目录，对每个访问到的文件输出检测到的元数据列。可以理解为 `find` 和轻量级 `file` 的结合：同样的确定性遍历，但每一行已经携带了你原本需要再起一个工具才能拿到的元数据。

## 为什么需要

`find` 只列出路径。想知道一个文件是不是文本、用什么编码、mime 类型是什么，你得 pipe 到 `file`、`xdetect` 或自己写脚本——每个条目都要 fork 一个进程。`fmeta` 在一次遍历里同时完成 walk 和 detection，因此输出可以直接用于：

- pipe 到 `awk` / `jq` 做过滤
- 给 LLM agent 一个目录树的紧凑清单
- 审计目录里的二进制文件或非 UTF-8 编码

## 安装

### Cargo

```bash
cargo install fmeta
```

### 从源码构建

需要 Rust 1.74+。

```bash
git clone https://github.com/ljh-sh/fmeta
cd fmeta
cargo build --release
# 二进制：target/release/fmeta
```

### 直接下载

从 [releases 页面](https://github.com/ljh-sh/fmeta/releases) 下载预编译二进制。

## 用法

```bash
# 遍历当前目录，每个文件输出一行 TSV（默认）。
fmeta

# 限制深度、显示隐藏文件、输出 JSON Lines。
fmeta --max-depth 2 --all --format json path/to/dir

# 人类可读的对齐表格。
fmeta --format table path/to/dir

# 关闭 .gitignore 过滤，显示全部。
fmeta --no-ignore path/to/dir

# 完全跳过检测（只输出路径，最快）。
fmeta --paths-only path/to/dir

# 读取更多字节以提高检测精度。
fmeta --sniff 16384 path/to/dir
```

### 输出格式

**TSV（默认）**——`size  kind  encoding  mime  mime_hint  path`，Tab 分隔，无表头。`path` 在最后，路径含空格也不会破坏 `awk`/`cut`。这是 agent / 管道友好的格式。

```
-	-	-	-	-	./src
12	file	UTF-8	-	text	./Cargo.toml
17	file	-	image/png	image	./logo.png
```

**表格（`--format table`）**——对齐列（多了 `depth`），便于人读：

```
depth  size  kind  mime       encoding  mime_hint  path
-----  ----  ----  ---------  --------  ---------  ----
1      12    file  -          UTF-8     text       ./Cargo.toml
1      17    file  image/png  -         image      ./logo.png
1      -     dir   -          -         -          ./src
```

**JSON Lines（`--format json`）**——每行一个对象，schema 稳定，可安全喂给 `jq`、`grep` 或直接给 LLM：

```json
{"path":"src/cli.rs","depth":1,"kind":"file","size":1684,"encoding":"UTF-8","binary":false,"category":"text"}
```

字段：

| 字段        | 是否一定有 | 说明                                              |
| ----------- | ---------- | ------------------------------------------------- |
| `path`      | 是         | 命令行传入或遍历得到的路径                        |
| `depth`     | 是         | 0 = 根                                            |
| `kind`      | 是         | `file` \| `dir` \| `symlink` \| `other`           |
| `size`      | 仅文件     | 字节数；不可读时为 `None`                         |
| `mime`      | 仅文件     | 来自 `infer`；纯文本无签名时为 `None`             |
| `encoding`  | 仅文本文件 | 来自 `chardetng`；二进制或空文件时缺省            |
| `binary`    | 仅文件     | sniff 窗口内出现 NUL 字节则为 `true`              |
| `category`  | 仅文件     | `mime_hint`：`text`\|`image`\|`audio`\|`video`\|`archive`\|`binary`\|`data` |
| `is_symlink`| 符号链接   | `false` 时省略                                     |

### 选项

| 选项                | 说明                                          |
| ------------------- | --------------------------------------------- |
| `-d, --max-depth N` | 最大递归深度（默认：无限）                    |
| `-a, --all`         | 包含隐藏文件和目录                            |
| `--no-ignore`       | 关闭 `.gitignore` / `.ignore` 过滤            |
| `-L, --follow`      | 跟随符号链接                                  |
| `-o, --format F`    | `tsv`（默认）、`table` 或 `json`              |
| `--sniff BYTES`     | 每个文件读取的字节数（默认：8192）            |
| `--paths-only`      | 跳过检测，只输出路径                          |

## 范围

范围内：目录遍历（感知 .gitignore）、大小、mime 检测、文本编码检测、粗粒度类型提示、TSV + 表格 + JSON 输出。

范围外（未来）：图片 / 音频 / 视频尺寸，EXIF / ID3 / PDF 元数据（已单开 issue 跟踪），网络和远程文件。路线图见 [docs/design.md](docs/design.md)。

## 许可证

Apache-2.0。详见 [LICENSE](LICENSE)。
