# Contributing

The [README](README.md) covers setup, building and the checks to run. [CLAUDE.md](CLAUDE.md) has
the commit and pull request rules.

## CI

`.github/workflows/ci.yml` runs on every push to a branch:

| Job | Does |
| --- | --- |
| lint | actionlint, shellcheck, ruff, rustfmt, and clippy for Linux and for the Windows target, and validates the desktop file and metainfo |
| test-linux, test-windows | `cargo test --workspace` on each system |
| build-linux | The command-line tool for Linux, statically linked against musl, in a `.tar.gz`. Extracts it and checks `--version`. |
| build-windows | The portable zip and the MSIX. Checks `--version` from both, installs the MSIX and opens both apps' windows. |
| build-flatpak | The Flatpak bundle, built in the GNOME SDK |
| check-flatpak | Installs the bundle on a stock runner, checks `--version` and that the five media tools are in it |

Every run uploads its archives with their `.sha256` files as `release-*` artifacts, kept for a
day. Rust's version is pinned as `RUST_VERSION`.

The Flatpak build keeps `.flatpak-builder` in the Actions cache. Only `main` saves it, and a
branch starts from the newest one, so a branch that changes the manifest rebuilds the bundled
tools on every push until it's merged. That takes about an hour.

## Versions

The source's version is `0.0.0-dev`. A build gets its version from `NVHVC_VERSION` at compile
time, and every front-end reports `nvhvc_core::VERSION`. CI sets it to the tag's version for a
release, adds `+<short commit>` for a pre-release, and uses `0.0.0-dev+<short commit>` otherwise.

MSIX versions are four numbers. The fourth orders the builds of one version, so an upgrade from a
pre-release to the release works: `alpha.N` is N, `beta.N` is 100 + N, `rc.N` is 200 + N, and the
release is 300. `1.2.0-beta.3` becomes `1.2.0.103`, and a dev build `0.0.0.0`.

## Packaging

### Flatpak

`packaging/flatpak/io.github.Mahir007.NvhvcHdr10Tool.yml` builds ffmpeg, MKVToolNix (with the parts
of Qt and Boost it needs) and GPAC, since the sandbox can't run the host's. The app gets the host
file system, because the tools read the encode by its path and write the output beside it. Its
temporary files go to `/var/tmp`, which is on disk, rather than `/tmp`.

```sh
flatpak install --user flathub org.gnome.Sdk//51 org.freedesktop.Sdk.Extension.rust-stable//26.08
flatpak-builder --user --install --force-clean build-flatpak packaging/flatpak/io.github.Mahir007.NvhvcHdr10Tool.yml
```

The first build takes most of an hour. Later ones rebuild only what changed.

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

## Releases

Pushing a tag starts `.github/workflows/release.yml`. It checks the tag is a version
(`v1.2.0`, or `v1.2.0-alpha.1`, `-beta.1` or `-rc.1`), runs CI for that version, and publishes
CI's archives as a GitHub release. A version with a hyphen is a pre-release.

```sh
git tag -a v1.2.0-beta.1 -m "v1.2.0-beta.1"
git push origin v1.2.0-beta.1
```

A second `-m` adds a summary to the top of the release notes. The rest of the notes come from
`cargo xtask release-notes`, which groups the commits since the previous tag (for a pre-release)
or the previous release by their Conventional Commits type. To see them before tagging, tag
locally and run it:

```sh
cargo xtask release-notes v1.2.0-beta.1 Mahir007/nvhvc_hdr10-tool
```

Pick the next version from the commits since the last release: a breaking change bumps the minor
version until 1.0 and the major after it, a `feat` the minor, anything else the patch.
