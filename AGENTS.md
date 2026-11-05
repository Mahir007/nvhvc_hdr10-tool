# Agent rules for nvhvc_hdr10-tool

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
- Build and run nvhvc-winui on Windows through the devenv:
  `/mnt/p/nvhvc_hdr10-tool-devenv/dev.sh cargo build -p nvhvc-winui`. It runs from
  `P:\nvhvc_hdr10-tool` in PowerShell 7. Plain `cargo` uses scoop's Rust and writes outside the
  project and `-devenv` folders. Never use Windows PowerShell 5.1. Type-check from Linux first with
  `cargo clippy -p nvhvc-winui --target x86_64-pc-windows-msvc`.
- Don't build from the junction path under `C:\Users`; cargo rebuilds whenever the path spelling
  changes.

## Reviews

- Review subagents can edit files even when asked only for findings. Scope them read-only, then
  read the full `git diff` afterwards and check each finding against the actual file.

## Environment (Debian WSL and Windows)

### Debian WSL

- Full access to sudo, KVM, apt and brew. Install and configure whatever
  the work or research needs.

### Windows host

Project-specific rules about Windows override this section.

#### Where to work

- The folder the work starts in is the project folder.
- Create a `<project>-devenv` folder next to it when the work needs one
  (`Project 2` and `Project 2-devenv`). It holds whatever the work needs
  that doesn't belong in the project.
- Work in both folders without asking.
- Everything else on Windows is off-limits by default. Ask before
  touching it and agree the scope. Ask again for each new task that needs
  it.

#### Tools

- Prefer portable versions of tools, because they can stay in the
  `-devenv` folder.
- If there is no portable version, use the scoop version. Scoop installs
  outside both folders, so ask first.

#### Change record

- Record every change on Windows outside the project and `-devenv` folders
  as it is made (installs, files, settings, PATH, registry, scheduled
  tasks).
- In the `-devenv` folder, record only the tools you download.
- Also record broad scans and reads of personal or sensitive locations on
  Windows.
- Keep the record in `system-changes.ods` in the `-devenv` folder.
  `system-changes.csv` next to it is the backing file: edit the CSV, then
  regenerate the .ods from it. Ensure the resulting .ods is Excel compatible.
- One row per change, or per task for reads. Columns: ID, date, type,
  item, version, source, location, size, command, integrity, status,
  revert step, date reversed, why, approved, notes.
- Approved quotes who approved the action and when. Leave it empty for
  tools in the `-devenv` folder. The revert step is a runnable command
  wherever there is one.
- In notes, say what a search or scan did not cover.
- Format the .ods properly: frozen and styled header, sized columns,
  wrapped text, filters, dates as real date cells, and a total of what is
  still on disk that counts nested locations once. Store size in bytes and
  use a number format to display it as B, MB or GB.
- Reverse all changes by the end of the project. Dry-run each reversal
  where the tool supports it. Keep the record until the user has reviewed
  it.

## Writing style for READMEs and other docs

Write like a person documenting their own tool, not like a product page.

- Don't sell. State what the thing does and stop. No "exact", "precisely",
  "seamlessly", "effortlessly", "powerful", "without asking", or a colon
  followed by a list of features presented as if it were an achievement
  ("You choose the exact stream: video, audio, resolution and bitrate").
- Don't explain what the reader already knows (where a Downloads folder is,
  "use your package manager"). Don't describe obvious behaviour ("options can
  be combined").
- Say each thing once. If a table or a section already covers it, don't add a
  paragraph that restates it.
- Prefer short, direct sentences. No long clauses chained with "so that...,
  which...". No filler adjectives ("small", "simple", "robust").
- Little bold or italic emphasis. Don't use "bold lead-in: sentence" for
  list items or troubleshooting entries; use plain headings or plain text.
- README is for users: what it does, install, quick start, usage,
  troubleshooting. Build, CI, release and compression internals go in
  CONTRIBUTING.md, not docs/building.md.
- Put a short quick start near the top, before the full option reference.
- Merge overlapping lists and tables into one.
- Leave out implementation details (exit codes, internal file names, cache
  keys) unless a user would act on them.
- Check any claim in the docs against the code before writing it.

## Commit message bodies

The same rules apply. A body says why the change was made: the cause, the
constraint, or a behaviour change the diff doesn't make obvious.

- Skip the body if the subject says enough. Most docs commits need none.
- Don't restate the subject, the diff, or the content of the doc being
  committed. Findings go in the doc, not the commit.
- Don't list which files were updated ("PLAN.md and X are updated") or
  which tests were added. The diff shows both.
- Don't list every component or check a feature adds. Mention only the
  non-obvious design choice, if there is one.
- Setup details for a test (env vars, default paths) go in a comment in
  the test, not the commit.
- Keep "(e.g. ...)" asides and quoted UI strings to the ones that matter,
  such as the exact error message being fixed.

## CI and release (GitHub Actions)

Use `ci.yml` and `release.yml` as the baseline, laid out as below. Add
workflows or split jobs when the project's needs call for it. The release
workflow should use CI to build the tag's version and publish CI's artifact,
rather than build it again.

### ci.yml

- Triggers: `push` to all branches with `paths-ignore: ['**.md', LICENSE]`,
  `workflow_dispatch`, and `workflow_call` with a required `version` input.
  No `pull_request` trigger: with branch pushes it runs everything twice.
- `concurrency` cancels an older run on the same branch, but not when
  called from release.yml or for a tag:
  `cancel-in-progress: ${{ github.event_name != 'workflow_call' && !startsWith(github.ref, 'refs/tags/') }}`.
- `permissions: contents: read` at the top. `timeout-minutes` on every job.
- Runners are `ubuntu-latest`, `macos-latest`, `windows-latest`. Pin an
  older one only for a concrete reason, and say why in a comment.
- Jobs:
  - lint, on Linux: the language's linters, plus actionlint, pinned:
    `docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:<version> -color`.
  - test: a matrix with Linux and the platform(s) the project ships for.
    Add a job only when it tests something the others can't (an old shell
    version, a script that only runs on Linux in release.yml).
  - build: runs on every push and doesn't wait for lint or test, so every
    run has an artifact. Stamps `inputs.version`, or `0.0.0-dev+<sha7>`
    without one. Writes a step summary table (archive, size, SHA-256).
    Uploads an artifact named `release` with the archives and their
    `.sha256` files, `retention-days: 1`, `if-no-files-found: error`.
  - check: `needs: build`, on the target platform. Extracts the archive with
    that system's own tools, as a user would, runs the binary or script and
    checks `--version` says the built version. On macOS, run both halves of
    a universal binary (x86_64 through Rosetta) and fail, not warn, when
    Rosetta is missing. If build already runs on the target, do this in
    build instead.
- The source says its version is `0.0.0-dev`; only built copies get a real
  one. Anything in the code that acts on its version (download URLs,
  release-only behaviour) must handle `0.0.0-dev+<commit>`.
- Archives are reproducible (fixed order, owners and mtimes from the
  commit), with `.sha256` files in `sha256sum` format.
- Cache anything slow to install: toolchains, modules, downloaded packages.
  Pin tool and module versions and put them in the cache key. For large
  caches, restore with fallback `restore-keys` and save only on `main`.

### release.yml

- `on: push: tags: ['v*']`, three jobs:
  - version: checks the tag against
    `^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$` and outputs the version
    without the `v`.
  - ci: `uses: ./.github/workflows/ci.yml` with that version.
  - release: `needs: [version, ci]`, with `contents: write`,
    `actions: write`, `id-token: write`, `attestations: write`. Checks out
    with `fetch-depth: 0`, downloads the `release` artifact, writes the
    notes (also to the step summary), attests the archives with
    `actions/attest` (`if: ${{ !github.event.repository.private }}`), then
    publishes and finally deletes the run's `release` artifact.
- Publish: the title is the version without the `v`; a version with a
  hyphen gets `--prerelease`; `--verify-tag`. If the release already
  exists, `gh release upload --clobber` and `gh release edit` instead of
  `create`, so a failed job can be re-run.
- Release notes come from a script in the repo, in the project's own
  language. A pre-release lists commits since the previous tag; a final
  release lists everything since the previous final release. Commits are
  grouped by kind; ones that only touch tests, CI or contributor docs go in
  a collapsed section.
