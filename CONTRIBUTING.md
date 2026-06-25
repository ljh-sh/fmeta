# Contributing to fmeta

Thanks for your interest! fmeta is a small, focused tool. Please read this short guide before opening an issue or PR.

## Reporting issues

Open a [GitHub issue](../../issues) and include:

- Operating system and version
- fmeta version (`fmeta --version`)
- Installation method (cargo / binary / source)
- The exact command you ran and a minimal sample tree
- Expected vs actual output

## Feature requests

fmeta deliberately stays small. We add metadata dimensions that are hard or slow to get with existing tools, and that make file trees more useful as structured data for humans and AI agents. If your idea fits, open an issue and explain the use case.

## Building from source

Requires Rust 1.74+.

```sh
git clone https://github.com/ljh-sh/fmeta
cd fmeta
cargo build --release
```

The binary will be at `target/release/fmeta`.

## Running tests

```sh
cargo test
```

For a quick smoke test on the repo itself:

```sh
cargo run --release -- --max-depth 2 .
```

## Pull requests

- Keep the change minimal and focused.
- Follow the existing Rust style (`cargo fmt`, `cargo clippy` clean).
- Update README examples and `docs/design.md` if your change affects CLI behavior or the output schema.
- Do not add heavy dependencies — keeping the dependency surface small is an explicit goal.

## License

By contributing, you agree that your contributions will be licensed under the Apache 2.0 License.
