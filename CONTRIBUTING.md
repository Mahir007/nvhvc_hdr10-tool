# Contributing

The [README](README.md) covers setup, building and the checks to run. [CLAUDE.md](CLAUDE.md) has
the commit and pull request rules.

## CI

`.github/workflows/ci.yml` runs on every push to a branch:

| Job | Does |
| --- | --- |
| lint | actionlint, shellcheck, rustfmt, and clippy for Linux and for the Windows target |
| test-linux, test-windows | `cargo test --workspace` on each system |
| build-linux | The command-line tool for Linux, statically linked against musl, in a `.tar.gz`. Extracts it and checks `--version`. |

Every run uploads its archives with their `.sha256` files as `release-*` artifacts, kept for a
day. Rust's version is pinned as `RUST_VERSION`.

## Versions

The source's version is `0.0.0-dev`. A build gets its version from `NVHVC_VERSION` at compile
time, and every front-end reports `nvhvc_core::VERSION`. CI sets it to the tag's version for a
release, adds `+<short commit>` for a pre-release, and uses `0.0.0-dev+<short commit>` otherwise.
