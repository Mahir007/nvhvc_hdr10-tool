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
        previous: None,
        commits: Vec::new(),
    }
}

fn link(sha: char) -> String {
    let sha = sha.to_string().repeat(40);
    format!("([`{}`]({BASE}/commit/{sha}))", &sha[..7])
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
        collect(&self.dir, tag, REPO).unwrap()
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
fn the_tag_message_comes_from_the_tag() {
    let repo = TempRepo::new("message");
    repo.commit("docs: start");
    repo.tag("v1.0.0", &["v1.0.0", "Summary line.\n\nMore detail."]);
    repo.commit("fix: later");
    repo.git(&["tag", "v1.0.1"]);

    let r = repo.release("v1.0.0");
    let notes = render(&r).unwrap();
    assert!(
        notes.starts_with("Summary line.\n\nMore detail.\n\n## What's changed"),
        "{notes}"
    );

    // A lightweight tag has no message of its own.
    assert_eq!(repo.release("v1.0.1").message, None);
    assert!(collect(&repo.dir, "v9.9.9", REPO).is_err());
}
