# Agent rules for nvhvc_hdr10-tool

`AGENTS.md` is a symlink to this file.

nvhvc patches HDR10 signalling into NVENC HEVC encodes without re-encoding: it rewrites the SPS
VUI colour fields and inserts mastering-display and content-light-level SEI. Crates:

- `nvhvc-core`: bitstream editing, tool discovery, probe/extract/remux pipeline, settings, and
  the presentation model both GUIs share (`uimodel`).
- `nvhvc-cli`: the `nvhvc` binary.
- `nvhvc-gtk`: GTK4 + libadwaita GUI for Linux. Builds as a stub elsewhere.
- `nvhvc-winui`: WinUI 3 GUI for Windows on windows-reactor (crates.io releases). Builds as a stub
  elsewhere.

## Git and pull requests

- Every commit follows [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/):
  `<type>(<scope>)!: <description>`, lower-case type, imperative description, no trailing period.
- Types: `feat`, `fix`, `perf`, `refactor`, `style` (formatting only), `test`, `docs`, `build`
  (manifests, lockfile, build.rs, dependency bumps), `ci`, `chore` (repo housekeeping such as
  `.gitattributes`), `revert`.
- Scopes: `core`, `cli`, `gtk`, `winui`, or `gui` for a change that lands in both GUIs together.
  No scope for repo-wide changes.
- A change that fits more than one type is split into one commit per type. Keep each commit
  building: land a core API before the GUI or CLI commit that uses it.
- Breaking changes get `!` and a `BREAKING CHANGE:` footer saying what to do. `Co-Authored-By:`
  stays the last trailer.
- Work lands on `main` through pull requests. Until 1.0, PRs are merged with a merge commit, not
  squashed or rebased, so each commit stays on `main` as written. PR titles are milestone names
  (`M10: ...`), matching the earlier PRs.
- From 1.0 on, PRs are squash-merged. The PR title becomes the commit subject, so it follows
  Conventional Commits and the milestone name moves to the PR body.

## Writing commit messages, PR bodies and docs

- A commit body says why: the cause, the constraint, or a behaviour change the diff doesn't make
  obvious. Skip it if the subject says enough; most docs commits need none.
- Don't restate the subject or the diff, list the files or tests touched, or list every component
  a feature adds. Findings go in the doc, not the commit. Test setup details go in a comment in
  the test.
- Docs read like a person documenting their own tool. Don't sell ("exact", "seamlessly",
  "powerful"), don't explain what the reader already knows, say each thing once, use short
  sentences and little bold or italic. No "bold lead-in: sentence" list items.
- The README is for users: what it does, install, quick start near the top, usage,
  troubleshooting. Build, CI, release and packaging internals go in `docs/building.md` or
  `CONTRIBUTING.md`. Leave out implementation details a user wouldn't act on.
- Check every claim in the docs against the code before writing it.

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

## Development environment

- Linux work (core, CLI, GTK, end-to-end runs) happens in the Debian WSL instance. Don't use the
  other WSL distros without asking.
- Keep cargo output and test outputs on ext4 (`CARGO_TARGET_DIR=$HOME/nvhvc-target`, `/tmp`).
  The `/mnt/p` checkout is on 9P, which is slow for builds and MP4 assembly.
- Anything on the Windows side (builds, installs, edits, deletes, and reads not clearly part of
  the request) needs the user's approval first, with a one-line reason. Log every Windows action
  in `windows-changes.ods` at the checkout root (git-excluded).
- Build nvhvc-winui on Windows from `P:\nvhvc_hdr10-tool` with PowerShell 7 (`pwsh.exe`), never
  Windows PowerShell 5.1: `cargo build -p nvhvc-winui`. Type-check it from Linux first with
  `cargo clippy -p nvhvc-winui --target x86_64-pc-windows-msvc`.
- Don't build from the junction path under `C:\Users`; cargo rebuilds whenever the path spelling
  changes.

## Reviews

- Review subagents can edit files even when asked only for findings. Scope them read-only, then
  read the full `git diff` afterwards and check each finding against the actual file.
