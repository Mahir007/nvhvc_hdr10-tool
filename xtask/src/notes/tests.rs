use super::*;

const REPO: &str = "owner/project";

fn release(tag: &str) -> Release {
    Release {
        tag: tag.to_owned(),
        repo: REPO.to_owned(),
        message: None,
    }
}

#[test]
fn pre_release_starts_with_a_warning() {
    let notes = render(&release("v1.2.0-beta.1")).unwrap();
    assert!(notes.starts_with(
        "> [!WARNING]\n> This is a pre-release, for trying things out before 1.2.0 is final. \
         Please report problems in [Issues](https://github.com/owner/project/issues).\n"
    ));
    let notes = render(&release("v1.2.0")).unwrap();
    assert!(!notes.contains("[!WARNING]"));
}

#[test]
fn tag_message_appears_without_its_first_line() {
    let mut r = release("v1.0.0");
    r.message = Some("v1.0.0\n\nThe first release.\nIt does things.\n".to_owned());
    let notes = render(&r).unwrap();
    assert_eq!(notes, "The first release.\nIt does things.\n");

    r.message = Some("v1.0.0\n".to_owned());
    assert_eq!(render(&r).unwrap().trim(), "");
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
        notes.starts_with("Summary line.\n\nMore detail.\n"),
        "{notes}"
    );

    // A lightweight tag has no message of its own.
    assert_eq!(repo.release("v1.0.1").message, None);
    assert!(collect(&repo.dir, "v9.9.9", REPO).is_err());
}
