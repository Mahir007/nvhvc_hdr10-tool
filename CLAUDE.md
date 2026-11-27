# Agent rules for nvhvc_hdr10-tool

`AGENTS.md` has these rules plus the owner's global `CLAUDE.md`, for agents that can't read that
file. Change it whenever either one changes.

nvhvc patches HDR10 signalling into NVENC HEVC encodes without re-encoding: it rewrites the SPS
VUI colour fields and inserts mastering-display and content-light-level SEI. Crates:

- `nvhvc-core`: bitstream editing, tool discovery, probe/extract/remux pipeline, settings, and
  the presentation model both GUIs share (`uimodel`).
- `nvhvc`: the `nvhvc` binary: the command-line tool and the app, which it opens from `nvhvc-gtk`
  on Linux and `nvhvc-winui` on Windows.
- `nvhvc-gtk`: GTK4 + libadwaita GUI for Linux, a library. Empty elsewhere.
- `nvhvc-winui`: WinUI 3 GUI for Windows on windows-reactor (crates.io releases), a library. Empty
  elsewhere.

## Git and pull requests

The owner's global commit rules apply. `AGENTS.md` includes them under Commit messages. This
project adds:

- Descriptions have no trailing period.
- `style` is for formatting only, `build` for manifests, the lockfile, `build.rs` and dependency
  bumps, and `chore` for repo housekeeping such as `.gitattributes`.
- Scopes: `core`, `cli`, `gtk`, `winui`, `gui` for a change that lands in both GUIs together,
  `packaging` for the installer and Flatpak files, and `contributing` for contributor docs. No scope
  for repo-wide changes.
- A change that fits more than one type is split into one commit per type. Keep each commit
  building: land a core API before the GUI or CLI commit that uses it.
- `Co-Authored-By:` stays the last trailer.
- Work lands on `main` through pull requests. Until 1.0, PRs are merged with a merge commit, not
  squashed or rebased, so each commit stays on `main` as written. PR titles are milestone names
  (`M10: ...`), matching the earlier PRs. This overrides the global rule that a PR title follows
  Conventional Commits.
- From 1.0 on, PRs are squash-merged. The PR title becomes the commit subject, so it follows
  Conventional Commits and the milestone name moves to the PR body.

## Pipeline

- Preserve the encode's timing. The patch only touches non-VCL NAL units, so the original
  per-frame timestamps stay valid. Capture them (`mkvextract timestamps_v2`, or a v2 timecode file
  built from ffprobe packet PTS for MP4 inputs) and re-apply them at mux with mkvmerge. Never
  force a constant frame rate.
- MP4Box does all MP4 muxing. ffmpeg only extracts or converts pieces (raw HEVC, SRT, DTS core,
  raw audio, attachments). mkvmerge produces the retimed video-only intermediate MP4Box imports.
- A remux is a carbon copy of the encode with the video swapped: keep track names, languages,
  flags, display dimensions, chapters and cover art, convert what has an MP4 equivalent, and
  report every track that gets dropped.
- MP4Box rebases imported tracks to zero and its `:delay=` option is lossy. Write edit lists with
  `-edits` and `-timescale 1000`.

## GUIs

- GTK and WinUI stay at parity. A UI change lands in both in the same round unless it is
  platform-only (for example GTK's exported global menu). The implementation can differ; the
  behaviour should not.
- No em or en dashes in user-visible text (GUIs, CLI output, About dialogs). The GTK
  custom-executable editor dialogs keep theirs.
- Tool-status dots: green = on PATH, blue = custom path, purple = install-dir fallback,
  red = missing or custom path not found. Blue is a fixed colour, never the accent.
- Banners have a bold title with the body inline; the body drops below when it contains `\n` or
  the line gets long, as WinUI's InfoBar does. WinUI is the reference when the two differ.
- GTK preferences use read-only rows with a pencil that opens an `AdwAlertDialog` editor, not
  inline editing.
- Stage 2 has no container row. The container comes from the preference or the Save as
  extension.
- The store screenshots in `packaging/screenshots` show the GTK app. Retake them when a
  change shows in them; `CONTRIBUTING.md` says how they're made.

## Builds and releases

- `CONTRIBUTING.md` describes CI, how the builds are made, and how to release.
- The source's version is `0.0.0-dev`. Builds take theirs from `NVHVC_VERSION`, and code that
  acts on the version reads `nvhvc_core::VERSION`, which can be `0.0.0-dev+<sha7>` or
  `X.Y.Z-beta.N+<sha7>`.
- `nvhvc` prints the version with `--version`; CI checks every archive with it.
- CI builds dev, alpha and beta versions and the tests with the fast profile, which is Cargo's
  `dev`, and release candidates and releases with `release`. UPX packs the release build of
  `nvhvc.exe`, not the Flatpak.
- The Flatpak carries its own ffmpeg, MKVToolNix and GPAC. The Windows builds use the ones on the
  system.
- The Flatpak build cache is saved on any branch whose manifest has no cache yet, not only on
  `main`. This is an exception to the global rule, because rebuilding the bundled tools takes
  about 20 minutes.
- Keep home-folder paths out of commits, PRs and release notes.

## Development environment

- Linux work (core, CLI, GTK, end-to-end runs) happens in the Debian WSL instance. Don't use the
  other WSL distros without asking.
- Keep cargo output and test outputs on ext4 (`CARGO_TARGET_DIR=$HOME/nvhvc-target`, `/tmp`).
  The `/mnt/p` checkout is on 9P, which is slow for builds and MP4 assembly.
- Build and run the Windows app through the devenv:
  `/mnt/p/nvhvc_hdr10-tool-devenv/dev.sh cargo build -p nvhvc`. It runs from
  `P:\nvhvc_hdr10-tool` in PowerShell 7. Plain `cargo` uses scoop's Rust and writes outside the
  project and `-devenv` folders. Never use Windows PowerShell 5.1. Type-check from Linux first with
  `cargo clippy -p nvhvc -p nvhvc-winui --target x86_64-pc-windows-msvc`.
- Don't build from the junction path under `C:\Users`; cargo rebuilds whenever the path spelling
  changes.

## Reviews

- Review subagents can edit files even when asked only for findings. Scope them read-only, then
  read the full `git diff` afterwards and check each finding against the actual file.
