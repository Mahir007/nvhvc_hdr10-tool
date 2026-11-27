use super::*;

const REPO: &str = "owner/project";
const BASE: &str = "https://github.com/owner/project";

fn commit(sha: char, subject: &str, body: &str) -> Commit {
    Commit {
        sha: sha.to_string().repeat(40),
        subject: subject.to_owned(),
        body: body.to_owned(),
    }
}

fn release(tag: &str) -> Release {
    Release {
        tag: tag.to_owned(),
        repo: REPO.to_owned(),
        message: None,
        readme: None,
        files: Vec::new(),
        previous: None,
        commits: Vec::new(),
    }
}

fn link(sha: char) -> String {
    let sha = sha.to_string().repeat(40);
    format!("([`{}`]({BASE}/commit/{sha}))", &sha[..7])
}

#[test]
fn globs_match_whole_names() {
    assert!(glob_matches(
        "nvhvc-*-x86_64.flatpak",
        "nvhvc-1.0.0-x86_64.flatpak"
    ));
    assert!(glob_matches(
        "nvhvc-*-x86_64.flatpak",
        "nvhvc-1.0.0-beta.1+abc1234-x86_64.flatpak"
    ));
    assert!(!glob_matches(
        "nvhvc-*-x86_64.flatpak",
        "nvhvc-1.0.0-x86_64.flatpak.sha256"
    ));
    assert!(!glob_matches(
        "nvhvc-*-windows-x64.zip",
        "other-1.0.0-windows-x64.zip"
    ));
    assert!(glob_matches("a*b*c", "aXbYc"));
}

#[test]
fn subjects_parse_as_conventional_commits() {
    let s = parse_subject("feat(gtk)!: drop the old dialog").unwrap();
    assert_eq!((s.kind, s.scope, s.breaking), ("feat", Some("gtk"), true));
    assert_eq!(s.description, "drop the old dialog");
    let s = parse_subject("fix: keep the log").unwrap();
    assert_eq!((s.kind, s.scope, s.breaking), ("fix", None, false));
    assert!(parse_subject("Merge branch 'x'").is_none());
    assert!(parse_subject("feature: unknown type").is_none());
    assert!(parse_subject("fix(): empty scope").is_none());
    assert!(parse_subject("fix:no space").is_none());
}

#[test]
fn breaking_change_footer_runs_to_the_next_trailer() {
    let body = "Why it changed.\n\nBREAKING CHANGE: pass --output\nfrom now on.\n\
                Co-Authored-By: Someone <a@b.c>\n";
    assert_eq!(
        breaking_change(body).as_deref(),
        Some("pass --output from now on.")
    );
    assert_eq!(
        breaking_change("BREAKING-CHANGE: use the new flag").as_deref(),
        Some("use the new flag")
    );
    assert_eq!(breaking_change("No footer here."), None);
}

#[test]
fn sha256_files_give_their_file_names() {
    let text = "ab12  nvhvc-1.0.0-x86_64.flatpak\ncd34 *nvhvc-1.0.0-windows-x64.zip\n\n";
    assert_eq!(
        sha256_names(text),
        ["nvhvc-1.0.0-x86_64.flatpak", "nvhvc-1.0.0-windows-x64.zip"]
    );
}

#[test]
fn pre_release_starts_with_a_warning() {
    let notes = render(&release("v1.2.0-beta.1")).unwrap();
    assert!(notes.starts_with(
        "> [!WARNING]\n> This is a pre-release, for trying things out before 1.2.0 is final. \
         Please report problems in [Issues](https://github.com/owner/project/issues).\n\n"
    ));
    let notes = render(&release("v1.2.0")).unwrap();
    assert!(!notes.contains("[!WARNING]"));
    assert!(notes.starts_with("## What's changed"));
}

#[test]
fn tag_message_appears_without_its_first_line() {
    let mut r = release("v1.0.0");
    r.message = Some("v1.0.0\n\nThe first release.\nIt does things.\n".to_owned());
    let notes = render(&r).unwrap();
    assert!(notes.starts_with("The first release.\nIt does things.\n\n## What's changed"));

    r.message = Some("v1.0.0\n".to_owned());
    assert!(render(&r).unwrap().starts_with("## What's changed"));
}

#[test]
fn downloads_follow_the_kinds_list() {
    let mut r = release("v1.0.0-beta.1");
    r.files = vec![
        "nvhvc-1.0.0-beta.1+abc1234-windows-x64.msix".to_owned(),
        "nvhvc-msix.cer".to_owned(),
        "nvhvc-1.0.0-beta.1+abc1234-x86_64.flatpak".to_owned(),
        "nvhvc-1.0.0-beta.1+abc1234-windows-x64.zip".to_owned(),
    ];
    r.readme =
        Some("# nvhvc\n\n```\n## Install in a code block\n```\n\n## Installing\n".to_owned());
    let notes = render(&r).unwrap();
    let release_dl = format!("{BASE}/releases/download/v1.0.0-beta.1");
    let dl = format!("{release_dl}/nvhvc-1.0.0-beta.1%2Babc1234");
    let table = format!(
        "## Download\n\n| System | Download |\n| --- | --- |\n\
         | Windows | [Portable zip]({dl}-windows-x64.zip) · [MSIX]({dl}-windows-x64.msix) · \
         [MSIX certificate]({release_dl}/nvhvc-msix.cer) |\n\
         | Linux | [Flatpak]({dl}-x86_64.flatpak) |\n\n\
         For how to install it, see [Installing]({BASE}/blob/v1.0.0-beta.1/README.md#installing) \
         in the README.\n\n## What's changed"
    );
    assert!(notes.contains(&table), "{notes}");
}

#[test]
fn a_row_with_one_file_keeps_its_link_text() {
    let mut r = release("v1.0.0");
    r.files = vec!["nvhvc-1.0.0-x86_64.flatpak".to_owned()];
    let notes = render(&r).unwrap();
    assert!(notes.contains(&format!(
        "| Linux | [Flatpak]({BASE}/releases/download/v1.0.0/nvhvc-1.0.0-x86_64.flatpak) |"
    )));
    // No README heading about installing.
    assert!(notes.contains(&format!(
        "For how to install it, see the [README]({BASE}/blob/v1.0.0/README.md)."
    )));
}

#[test]
fn a_kind_without_link_text_shows_the_file_name() {
    let kinds = [
        Kind {
            glob: "app-*.dmg",
            system: System::MacOs,
            text: None,
        },
        Kind {
            glob: "app-*.apk",
            system: System::Android,
            text: Some("APK"),
        },
    ];
    let files = vec!["app-1.apk".to_owned(), "app-1.dmg".to_owned()];
    let rows = rows(&files, &kinds).unwrap();
    let rows: Vec<_> = rows
        .iter()
        .map(|row| (row.system, row.links.clone()))
        .collect();
    assert_eq!(
        rows,
        [
            (System::MacOs, vec![("app-1.dmg", "app-1.dmg")]),
            (System::Android, vec![("APK", "app-1.apk")]),
        ]
    );
}

#[test]
fn an_unknown_file_is_an_error() {
    let mut r = release("v1.0.0");
    r.files = vec!["notes.txt".to_owned()];
    assert!(render(&r).unwrap_err().contains("notes.txt"));
}

#[test]
fn install_heading_is_the_first_one_about_installing() {
    let readme = "# Title\n\n## Layout\n\n### Quick start\n\n## Installing\n";
    assert_eq!(install_heading(readme).as_deref(), Some("Quick start"));
    assert_eq!(
        install_heading("## Downloads and setup ##\n").as_deref(),
        Some("Downloads and setup")
    );
    assert_eq!(install_heading("#### Install\n## Usage\n"), None);
    assert_eq!(
        anchor("Quick start: Windows (MSIX)"),
        "quick-start-windows-msix"
    );
}

#[test]
fn changes_are_grouped_by_type() {
    let mut r = release("v1.1.0");
    r.previous = Some("v1.0.0".to_owned());
    r.commits = vec![
        commit('1', "feat(gtk): add a thing", ""),
        commit('2', "fix: stop a crash", ""),
        commit('3', "perf(core): read less", ""),
        commit('4', "docs: explain the thing", ""),
        commit('5', "docs(contributing): describe CI", ""),
        commit('6', "ci: cache the toolchain", ""),
        commit('7', "feat(cli)!: rename run to patch", ""),
        commit(
            '8',
            "fix: change the default",
            "BREAKING CHANGE: pass --mkv to keep MKV output.",
        ),
        commit('9', "Update the README", ""),
    ];
    let notes = render(&r).unwrap();
    let expected = format!(
        "## What's changed\n\n\
         ### ⚠️ Breaking changes\n\n\
         - **cli:** Rename run to patch {}\n\
         - Pass --mkv to keep MKV output. {}\n\n\
         ### ✨ Features\n\n- **gtk:** Add a thing {}\n\n\
         ### 🐛 Fixes\n\n- Stop a crash {}\n\n\
         ### ⚡ Performance\n\n- **core:** Read less {}\n\n\
         ### 📖 Documentation\n\n- Explain the thing {}\n\n\
         <details>\n<summary>🧰 Other changes (3)</summary>\n\n\
         - docs(contributing): describe CI {}\n\
         - ci: cache the toolchain {}\n\
         - Update the README {}\n\n\
         </details>\n\n\
         **Full changelog:** [v1.0.0...v1.1.0]({BASE}/compare/v1.0.0...v1.1.0)\n",
        link('7'),
        link('8'),
        link('1'),
        link('2'),
        link('3'),
        link('4'),
        link('5'),
        link('6'),
        link('9'),
    );
    assert_eq!(notes, expected);
}

#[test]
fn only_typed_other_changes_mean_no_user_facing_changes() {
    let mut r = release("v1.0.1");
    r.commits = vec![commit('1', "ci: run on tags", "")];
    let notes = render(&r).unwrap();
    assert!(notes.contains("## What's changed\n\nNo user-facing changes.\n\n<details>"));
    assert!(notes.contains(&format!(
        "**Full changelog:** [all commits]({BASE}/commits/v1.0.1)"
    )));

    // A commit without a type might be anything, so the sentence is left out.
    r.commits.push(commit('2', "Tweak things", ""));
    assert!(!render(&r).unwrap().contains("No user-facing changes."));
}

#[test]
fn a_revert_and_its_commit_are_left_out() {
    let reverted = commit('a', "feat: try a thing", "");
    let revert = commit(
        'b',
        "revert: feat: try a thing",
        &format!("This reverts commit {}.\n", reverted.sha),
    );
    let mut r = release("v1.0.0");
    r.commits = vec![reverted, commit('c', "fix: keep this", ""), revert];
    let notes = render(&r).unwrap();
    assert!(!notes.contains("try a thing"));
    assert!(notes.contains("Keep this"));

    // A revert of something already released stays.
    r.commits = vec![commit(
        'd',
        "revert: feat: an old thing",
        &format!("This reverts commit {}.\n", "e".repeat(40)),
    )];
    assert!(render(&r).unwrap().contains("- revert: feat: an old thing"));
}

#[test]
fn checksums_follow_the_table() {
    let mut r = release("v1.0.0");
    r.files = vec![
        "nvhvc-1.0.0-x86_64.flatpak".to_owned(),
        "nvhvc-1.0.0-windows-x64.zip".to_owned(),
        "nvhvc-1.0.0-windows-x64.msix".to_owned(),
    ];
    let notes = render(&r).unwrap();
    let expected = "<details>\n<summary>🔒 SHA-256</summary>\n\n\
        Download the file's `.sha256` too, and run its line in the folder with both. \
        It prints `OK` or `True` if the file is intact.\n\n\
        Windows, in PowerShell:\n\n```powershell\n\
        $hash, $file = (Get-Content 'nvhvc-1.0.0-windows-x64.zip.sha256') -split '\\s+'; (Get-FileHash $file).Hash -eq $hash\n\
        $hash, $file = (Get-Content 'nvhvc-1.0.0-windows-x64.msix.sha256') -split '\\s+'; (Get-FileHash $file).Hash -eq $hash\n\
        ```\n\n\
        Linux:\n\n```sh\nsha256sum -c 'nvhvc-1.0.0-x86_64.flatpak.sha256'\n```\n\n\
        </details>\n";
    assert!(notes.ends_with(expected), "{notes}");
    assert!(!notes.contains("\n\n\n"));
}

/// A throwaway git repository, deleted when dropped.
struct TempRepo {
    dir: std::path::PathBuf,
    config: std::path::PathBuf,
    /// Each git call gets the next second, so commits have a fixed order.
    clock: std::cell::Cell<u32>,
}

impl TempRepo {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // An empty config, so the user's signing or hook settings don't apply.
        let config = dir.with_extension("gitconfig");
        std::fs::write(&config, "").unwrap();
        let repo = Self {
            dir,
            config,
            clock: std::cell::Cell::new(0),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    fn git(&self, args: &[&str]) {
        let tick = self.clock.get() + 1;
        self.clock.set(tick);
        let date = format!("2026-01-01T00:{:02}:{:02}Z", tick / 60, tick % 60);
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", &self.config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn commit(&self, subject: &str) {
        self.git(&["commit", "-q", "--allow-empty", "-m", subject]);
    }

    fn tag(&self, tag: &str, message: &[&str]) {
        let mut args = vec!["tag", "-a", tag];
        for line in message {
            args.extend(["-m", line]);
        }
        self.git(&args);
    }

    fn release(&self, tag: &str) -> Release {
        collect(&self.dir, tag, REPO, Vec::new()).unwrap()
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_file(&self.config);
    }
}

fn subjects(release: &Release) -> Vec<&str> {
    release.commits.iter().map(|c| c.subject.as_str()).collect()
}

#[test]
fn ranges_come_from_the_previous_tag() {
    let repo = TempRepo::new("ranges");
    repo.commit("feat: first");
    repo.tag("v0.1.0", &["v0.1.0"]);
    repo.commit("feat: second");
    repo.git(&["checkout", "-q", "-b", "side"]);
    repo.commit("fix: on a branch");
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "Merge branch 'side'",
        "side",
    ]);
    repo.tag("v0.2.0-beta.1", &["v0.2.0-beta.1"]);
    repo.commit("fix: third");
    repo.tag("v0.2.0-beta.2", &["v0.2.0-beta.2"]);
    repo.commit("docs: fourth");
    repo.tag("v0.2.0", &["v0.2.0"]);

    let first = repo.release("v0.1.0");
    assert_eq!(first.previous, None);
    assert_eq!(subjects(&first), ["feat: first"]);

    // Oldest first, without the merge.
    let beta1 = repo.release("v0.2.0-beta.1");
    assert_eq!(beta1.previous.as_deref(), Some("v0.1.0"));
    assert_eq!(subjects(&beta1), ["feat: second", "fix: on a branch"]);

    let beta2 = repo.release("v0.2.0-beta.2");
    assert_eq!(beta2.previous.as_deref(), Some("v0.2.0-beta.1"));
    assert_eq!(subjects(&beta2), ["fix: third"]);

    // A release counts from the previous release, past its pre-releases.
    let release = repo.release("v0.2.0");
    assert_eq!(release.previous.as_deref(), Some("v0.1.0"));
    assert_eq!(
        subjects(&release),
        [
            "feat: second",
            "fix: on a branch",
            "fix: third",
            "docs: fourth"
        ]
    );
}

#[test]
fn the_tag_message_and_readme_come_from_the_tag() {
    let repo = TempRepo::new("message");
    std::fs::write(repo.dir.join("README.md"), "# P\n\n## Install\n").unwrap();
    repo.git(&["add", "README.md"]);
    repo.commit("docs: add the README");
    repo.tag("v1.0.0", &["v1.0.0", "Summary line.\n\nMore detail."]);
    repo.commit("fix: later");
    repo.git(&["tag", "v1.0.1"]);

    let r = repo.release("v1.0.0");
    let notes = render(&r).unwrap();
    assert!(
        notes.starts_with("Summary line.\n\nMore detail.\n\n## What's changed"),
        "{notes}"
    );
    assert_eq!(r.readme.as_deref(), Some("# P\n\n## Install\n"));

    // A lightweight tag has no message of its own.
    assert_eq!(repo.release("v1.0.1").message, None);
    assert!(collect(&repo.dir, "v9.9.9", REPO, Vec::new()).is_err());
}
