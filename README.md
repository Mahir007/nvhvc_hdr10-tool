# nvhvc_hdr10-tool

nvhvc adds HDR10 signalling to an NVENC HEVC encode without re-encoding it. It reads the HDR10
metadata from the source, rewrites the colour fields in the encode's SPS, inserts mastering-display
and content-light-level SEI, and remuxes to MKV or MP4. The other tracks, chapters and per-frame
timestamps are carried over from the encode.

## Layout

| Crate | What it is |
| --- | --- |
| `crates/nvhvc-core` | Bitstream editing, tool discovery, the probe/extract/patch/remux pipeline, settings, and the presentation model both GUIs use (`uimodel`). |
| `crates/nvhvc-cli` | The `nvhvc` binary. |
| `crates/nvhvc-gtk` | GTK 4 + libadwaita GUI. Builds as a stub on anything but Linux. |
| `crates/nvhvc-winui` | WinUI 3 GUI on [windows-reactor](https://crates.io/crates/windows-reactor). Builds as a stub on anything but Windows. |

The stubs keep `cargo build` and `cargo test` working for the whole workspace on either platform.

## Requirements

- Rust 1.92 or later, 1.95 for `nvhvc-winui`. Install it with rustup; distribution packages are
  often older.
- Linux: GTK 4.18 or later, libadwaita 1.8 or later, a C toolchain and pkg-config.
- Windows: Visual Studio 2022 Build Tools with the C++ workload (MSVC and the Windows SDK).
- The media tools the pipeline runs. They're needed to run the app and the pipeline examples, not
  to build.

| Tool | Package | Needed for |
| --- | --- | --- |
| ffmpeg, ffprobe | FFmpeg | everything |
| mkvmerge, mkvextract | MKVToolNix | everything |
| MP4Box | GPAC | MP4 output |

Tools are found through a custom path in settings, then `PATH`, then the default install folders
under `Program Files` on Windows.

## Windows setup

Pick one package manager.

winget:

```powershell
winget install Rustlang.Rustup Git.Git Gyan.FFmpeg MoritzBunkus.MKVToolNix GPAC.GPAC
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Chocolatey (elevated shell):

```powershell
choco install rustup.install git ffmpeg mkvtoolnix gpac visualstudio2022-workload-vctools
```

Scoop:

```powershell
scoop bucket add extras
scoop install rustup git ffmpeg mkvtoolnix gpac
```

Scoop has no package for the Build Tools. Install them with the winget command above.

Then, in a new terminal:

```powershell
rustup default stable
git clone https://github.com/Mahir007/nvhvc_hdr10-tool
cd nvhvc_hdr10-tool
```

## Linux setup

apt (Ubuntu 25.10 or later; Debian 13 ships libadwaita 1.7, so use Homebrew there):

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev ffmpeg mkvtoolnix
```

Ubuntu 25.10 and later have no gpac package. Get MP4Box from Homebrew (`brew install gpac`).

dnf (Fedora 43 or later):

```sh
sudo dnf install gcc pkgconf-pkg-config gtk4-devel libadwaita-devel mkvtoolnix gpac
sudo dnf install ffmpeg --allowerasing   # from RPM Fusion
```

Fedora's own `ffmpeg-free` leaves out HEVC, and the probe decodes HEVC frames. Enable
[RPM Fusion](https://rpmfusion.org/Configuration) before the second command.

pacman:

```sh
sudo pacman -S --needed base-devel rustup gtk4 libadwaita ffmpeg mkvtoolnix-cli gpac
```

For apt and dnf, install Rust from <https://rustup.rs>. Then:

```sh
rustup default stable
git clone https://github.com/Mahir007/nvhvc_hdr10-tool
cd nvhvc_hdr10-tool
```

### Homebrew

Use Homebrew when the distribution's GTK or libadwaita is too old. The distribution's C compiler
is still needed.

```sh
brew install rustup gtk4 libadwaita pkgconf ffmpeg mkvtoolnix gpac
rustup default stable
```

Every cargo command then needs pkg-config pointed at Homebrew, and an rpath so the binary finds
Homebrew's GTK at run time:

```sh
B=$(brew --prefix)
export PKG_CONFIG_PATH=$B/lib/pkgconfig:$B/share/pkgconfig
export RUSTFLAGS="-C link-arg=-Wl,-rpath,$B/lib"
```

Changing `RUSTFLAGS` rebuilds the whole target directory, so keep them the same for every crate
you build into it.

## Building and running

```sh
cargo run -p nvhvc-gtk            # Linux GUI
cargo run -p nvhvc-winui          # Windows GUI
cargo run -p nvhvc-cli -- tools   # CLI; arguments go after --
```

`--release` builds use LTO with one codegen unit, so they're slow. Use debug builds while working.

The first `nvhvc-winui` build downloads the Windows App SDK runtime from nuget.org and copies it
into the target directory next to the exe. The app runs from there without a system-wide runtime.
To move the exe, move those files with it.

## Checks

```sh
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

The unit tests are in `nvhvc-core` and don't need the media tools.

`nvhvc-winui` can be type-checked from Linux:

```sh
rustup target add x86_64-pc-windows-msvc
cargo clippy -p nvhvc-winui --target x86_64-pc-windows-msvc
```

Linking and running it needs Windows.

## Debugging

`NVHVC_TRACE=1` logs every external tool command and its duration to stderr. It works with the CLI
and both GUIs.

`nvhvc run --keep-intermediates` keeps the extracted stream, the patched stream and the timecode
file.

`nvhvc-core` has examples for working on one stage at a time:

| Example | Does |
| --- | --- |
| `dump_nals FILE.hevc` | Lists the NAL units in an Annex B stream and prints SPS bytes. |
| `patch_file IN.hevc OUT.hevc` | Patches a raw stream with a fixed HDR10 metadata set. |
| `probe SOURCE` | Prints what ffprobe reports for a source. |
| `run SOURCE ENCODE OUTPUT [CHROMA_LOC]` | Runs the whole pipeline. |

```sh
cargo run -p nvhvc-core --example dump_nals -- stream.hevc
```

## CLI

```
nvhvc tools                       list the external tools and where they were found
nvhvc probe SOURCE                print the source's HDR10 metadata
nvhvc run SOURCE ENCODE OUTPUT    patch ENCODE with SOURCE's metadata into OUTPUT
```

The output container follows the extension of `OUTPUT` (`.mkv` or `.mp4`). `run` also takes
`--chroma-loc 0-5` to write that chroma sample location instead of keeping the encode's. Every
command takes `--ffmpeg`, `--ffprobe`, `--mkvmerge`, `--mkvextract` and `--mp4box` to use a
specific copy of a tool.

## Contributing

Commits follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), with the
crate as the scope (`core`, `cli`, `gtk`, `winui`, or `gui` for both GUIs). A UI change goes into
both GUIs. [CLAUDE.md](CLAUDE.md) has the full rules.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
