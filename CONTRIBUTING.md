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
| build-windows | The portable zip and the MSIX. Checks `--version` from both, installs the MSIX and opens both apps' windows. |

Every run uploads its archives with their `.sha256` files as `release-*` artifacts, kept for a
day. Rust's version is pinned as `RUST_VERSION`.

## Versions

The source's version is `0.0.0-dev`. A build gets its version from `NVHVC_VERSION` at compile
time, and every front-end reports `nvhvc_core::VERSION`. CI sets it to the tag's version for a
release, adds `+<short commit>` for a pre-release, and uses `0.0.0-dev+<short commit>` otherwise.

MSIX versions are four numbers. The fourth orders the builds of one version, so an upgrade from a
pre-release to the release works: `alpha.N` is N, `beta.N` is 100 + N, `rc.N` is 200 + N, and the
release is 300. `1.2.0-beta.3` becomes `1.2.0.103`, and a dev build `0.0.0.0`.

## Packaging

### MSIX

The MSIX holds an `nvhvc-winui.exe` built with `NVHVC_WINUI_RUNTIME=framework`, which uses the
Windows App SDK runtime installed on the system instead of copying one beside the exe. The
package declares that runtime as a dependency. It isn't signed: the publisher in
`packaging/msix/AppxManifest.xml.in` carries the OID that marks an unsigned package.

```powershell
$env:NVHVC_WINUI_RUNTIME = 'framework'
cargo build --release -p nvhvc-cli -p nvhvc-winui
packaging\msix\build-msix.ps1 -Version 0.0.0-dev -BinDir target\release -Out nvhvc.msix
```

`build-msix.ps1` needs `makeappx.exe` from the Windows SDK.

The icon in `packaging/icons` is a placeholder. The MSIX tiles in `packaging/msix/Assets` are
renders of it:

```sh
svg=packaging/icons/io.github.Mahir007.NvhvcHdr10Tool.svg
rsvg-convert -w 44 -h 44 $svg -o packaging/msix/Assets/Square44x44Logo.png
rsvg-convert -w 150 -h 150 $svg -o packaging/msix/Assets/Square150x150Logo.png
rsvg-convert -w 50 -h 50 $svg -o packaging/msix/Assets/StoreLogo.png
```
