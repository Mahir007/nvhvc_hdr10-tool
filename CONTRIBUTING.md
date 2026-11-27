# Contributing

The [README](README.md) covers setup, building and the checks to run. [CLAUDE.md](CLAUDE.md) has
the commit and pull request rules.

## CI

`.github/workflows/ci.yml` runs on every push to a branch:

| Job | Does |
| --- | --- |
| lint | actionlint, shellcheck, ruff, rustfmt, and clippy for Linux and for the Windows target, and validates the desktop file and metainfo |
| test-linux, test-windows | `cargo test --workspace` on each system |
| build-windows | The installer, the portable zip and the MSIX. Checks that `nvhvc.exe` doesn't need the Visual C++ runtime, installs each, and checks `--version` and opens the app's window from each. Uninstalls the installer's copy. |
| build-flatpak | The Flatpak bundle, built in the GNOME SDK |
| check-flatpak | Installs the bundle on a stock runner, checks `--version` and that the five media tools are in it |

Every run uploads its archives with their `.sha256` files as `release-*` artifacts, kept for a
day. Rust's version is pinned as `RUST_VERSION`.

The Flatpak build keeps `.flatpak-builder` in the Actions cache, once for each version of the
manifest. A branch that changes the manifest rebuilds the bundled tools from the first one that
changed, about 20 minutes for all of them, and saves its own cache for its later pushes. Other
branches use `main`'s.

## Versions

The source's version is `0.0.0-dev`. A build gets its version from `NVHVC_VERSION` at compile
time, and every front-end reports `nvhvc_core::VERSION`. CI sets it to the tag's version for a
release, adds `+<short commit>` for a pre-release, and uses `0.0.0-dev+<short commit>` otherwise.

MSIX versions are four numbers. The fourth orders the builds of one version, so an upgrade from a
pre-release to the release works: `alpha.N` is N, `beta.N` is 100 + N, `rc.N` is 200 + N, and the
release is 300. `1.2.0-beta.3` becomes `1.2.0.103`, and a dev build `0.0.0.0`.

## Build profiles

`Cargo.toml` sets up two profiles, and CI picks one from the version:

| Profile | Used for | What it does |
| --- | --- | --- |
| fast, which is Cargo's `dev` | Dev, alpha and beta builds, and the tests | No optimisation and full debug info. The Windows zip and MSIX carry `nvhvc.pdb`, and the Flatpak keeps the debug info in its binaries. |
| `release` | Release candidates and releases | `opt-level = 3`, LTO, one codegen unit, stripped, `panic = "abort"`. UPX 5.3.0 packs `nvhvc.exe`, and CI runs the packed one. |

The fast profile is Cargo's own `dev` rather than a custom one. windows-reactor-setup finds the
folder to copy the Windows App SDK runtime into by the profile's name, and only knows `debug` and
`release`.

UPX doesn't pack the Flatpak: its bundle is compressed already, and packed binaries would make
OSTree's deltas between updates much larger. On Linux with glibc, which covers the tests and the
Flatpak, Rust links with rust-lld by default. The Windows builds keep their default linker.

On Windows, both profiles link the C runtime into `nvhvc.exe` (`+crt-static` in
`.cargo/config.toml`), so they run without the Visual C++ Redistributable. A `RUSTFLAGS`
variable replaces that setting, so CI checks the exe's imports with `check-crt.ps1`.

Locally, `cargo build` builds the fast one and `cargo build --release` the other.

## Packaging

### One binary

`nvhvc` is both the command-line tool and the app, on Windows and Linux. `nvhvc gui` opens the app,
and the Linux desktop file runs that. `nvhvc-winui` and `nvhvc-gtk` are libraries it calls.

On Windows, `nvhvc.exe` is a console program, so a terminal waits for it and shows its output.
Starting it with no arguments outside a terminal, from Explorer, the Start menu or a shortcut,
opens the app too. `crates/nvhvc/build.rs`
embeds `nvhvc.manifest`, whose `consoleAllocationPolicy` stops Windows 11 24H2 and later from
opening a console window for such a start. Older Windows opens one, and the exe closes it at once.

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

`packaging/flatpak/metainfo.py` fills in the metainfo template: the releases from the tags, and
the commit the screenshot links point at. CI runs it before flatpak-builder, which leaves out
`.git`. A local build without it gets no releases, and screenshots from `main`.

flatpak-builder's `appstreamcli compose` drops a screenshot it can't download, so a build of a
commit that isn't on GitHub has none. CI fails the build if one is missing.

The screenshots in `packaging/screenshots` show the GTK app at its default size, in GNOME's
default light style, at 2x, with the window's shadow on a transparent background. Each one is
captured on a headless sway over black and over white, and the alpha comes from the difference.
Retake them when a change shows in them.

### Installer

`packaging/inno/nvhvc.iss` builds the installer with Inno Setup 7, whose version CI pins as
`INNO_VERSION`. It installs for the current user, in `%LOCALAPPDATA%\Programs\nvhvc`, and never asks
for admin rights. It carries the `nvhvc.exe` the MSIX does, built with
`NVHVC_WINUI_RUNTIME=framework`:

```powershell
$env:NVHVC_WINUI_RUNTIME = 'framework'
cargo build --release -p nvhvc
iscc /DVersion=0.0.0-dev "/O$PWD\dist" packaging\inno\nvhvc.iss
```

At the Ready page, Setup runs its `nvhvc.exe check-runtime`, which asks Windows for the runtime the
way the app does when it starts. If it's missing, Setup downloads Microsoft's runtime installer,
checks its SHA-256, runs it with `--quiet` and checks again. Without admin rights, that installs
the runtime for the current user. The URL and hash are at the top of the script. Any 2.x from 2.4
on runs the app, so they only need updating for a newer runtime.

Adding `nvhvc` to `PATH` is an optional task. It adds or removes only the app's folder in the
user's `Path`, and the uninstaller takes it off again.

CI installs it silently with the `PATH` task, checks `--version`, the runtime, `PATH` and the app's
window, and uninstalls it. That happens before the MSIX check installs a runtime of its own.

`packaging/icons/nvhvc.ico`, for Setup and the shortcuts, is a render of the SVG icon:

```sh
svg=packaging/icons/io.github.Mahir007.NvhvcHdr10Tool.svg
for n in 16 20 24 32 40 48 64 256; do rsvg-convert -w $n -h $n $svg -o /tmp/icon-$n.png; done
magick /tmp/icon-{16,20,24,32,40,48,64,256}.png packaging/icons/nvhvc.ico
```

### Portable zip

The zip holds `nvhvc.exe`, the licence, and the Windows App SDK runtime files listed in
`.github/scripts/windows-runtime-files.txt`. windows-reactor-setup copies the whole runtime beside
the exe, and local builds keep all of it. The zip leaves out the files the app doesn't need:

- The language folders other than `en-us`. WinUI's own text, such as a text box's context menu,
  falls back to English.
- `Microsoft.Web.WebView2.Core.dll`, `DWriteCore.dll`, `Microsoft.Graphics.Imaging.dll`,
  `SessionHandleIPCProxyStub.dll` and the map control's `map.html`.
- `Microsoft.UI.Xaml.Phone.dll`, which holds the date and time pickers, Pivot and the page
  transitions. Add it back if the app starts using them.
- `Microsoft.UI.Xaml.Controls.pri`, `Microsoft.UI.pri` and `Microsoft.WindowsAppRuntime.pri`.
  `resources.pri` has the same resources.

The log of build-windows' "Pack the zip" step lists the files left out. After updating
windows-reactor, check that list for new files, then run the app from the unpacked zip through
every screen and a patch.

### MSIX

The MSIX holds an `nvhvc.exe` built with `NVHVC_WINUI_RUNTIME=framework`, which uses the
Windows App SDK runtime installed on the system instead of copying one beside the exe. The
package declares that runtime as a dependency.

```powershell
$env:NVHVC_WINUI_RUNTIME = 'framework'
cargo build --release -p nvhvc
packaging\msix\build-msix.ps1 -Version 0.0.0-dev -BinDir target\release -Out nvhvc.msix
```

Without `-Pfx`, the package is unsigned, and its publisher carries the OID that lets Windows 11
install it with `Add-AppxPackage -AllowUnsigned`. With `-Pfx` and the password in
`MSIX_PFX_PASSWORD`, it's signed, and its publisher is the certificate's subject.

CI signs it with a self-signed certificate for `CN=Mahir007`, kept in the `MSIX_PFX` (the
`.pfx` in base64) and `MSIX_PFX_PASSWORD` secrets. The release ships the certificate as
`nvhvc-msix.cer`. A fork's CI has no secrets and builds the unsigned package; a release fails
without them.

The certificate expires on 2031-11-23. The signatures are timestamped, so packages signed before
then still install. To replace it, make a new one with the same subject, update both secrets
and the thumbprint in the README. Users then trust the new `.cer`.

```sh
openssl req -x509 -newkey rsa:3072 -sha256 -days 1826 -nodes -subj "/CN=Mahir007" \
  -keyout key.pem -out cert.pem -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=codeSigning" -addext "basicConstraints=critical,CA:FALSE"
openssl rand -base64 32 | tr -d '\n' > pw
openssl pkcs12 -export -inkey key.pem -in cert.pem -out msix.pfx -passout file:pw
base64 -w0 msix.pfx | gh secret set MSIX_PFX
gh secret set MSIX_PFX_PASSWORD < pw
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

A second `-m` adds a summary to the top of the release notes. It's also the release's description
in the Flatpak's metainfo, which software centres show, so keep it to sentences and `- ` lists.
The rest of the notes come from `cargo xtask release-notes`, which groups the commits since the
previous tag (for a pre-release) or the previous release by their Conventional Commits type. To
see them before tagging, tag locally and run it:

```sh
cargo xtask release-notes v1.2.0-beta.1 Mahir007/nvhvc_hdr10-tool
```

Pick the next version from the commits since the last release: a breaking change bumps the minor
version until 1.0 and the major after it, a `feat` the minor, anything else the patch.
