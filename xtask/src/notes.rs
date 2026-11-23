//! Release notes for a tag, in this order: a pre-release warning, the rest of the tag's message,
//! and the changes since the previous release.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

const TYPES: &[&str] = &[
    "feat", "fix", "perf", "docs", "refactor", "style", "test", "ci", "build", "chore", "revert",
];

const HEADINGS: [&str; 5] = [
    "⚠️ Breaking changes",
    "✨ Features",
    "🐛 Fixes",
    "⚡ Performance",
    "📖 Documentation",
];

#[derive(Clone, Debug)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    pub body: String,
}

/// Everything the notes are written from.
#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    /// `owner/name` on GitHub.
    pub repo: String,
    /// The annotated tag's message, if the tag has one.
    pub message: Option<String>,
    /// The tag the changes are counted from.
    pub previous: Option<String>,
    /// The commits since `previous`, oldest first, without merges.
    pub commits: Vec<Commit>,
}

/// Reads the release from the git repository in the current folder and writes its notes.
pub fn from_git(tag: &str, repo: &str) -> Result<String, String> {
    render(&collect(Path::new("."), tag, repo)?)
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("can't run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A pre-release counts from the previous tag of any kind, a release from the previous release.
fn collect(dir: &Path, tag: &str, repo: &str) -> Result<Release, String> {
    let tag_ref = format!("refs/tags/{tag}");
    git(dir, &["rev-parse", "--verify", "--quiet", &tag_ref])
        .map_err(|_| format!("there's no tag {tag}"))?;

    let parent = format!("{tag}^");
    let mut describe = vec!["describe", "--tags", "--abbrev=0", "--match", "v*"];
    if !tag.contains('-') {
        describe.extend(["--exclude", "v*-*"]);
    }
    describe.push(&parent);
    let previous = git(dir, &describe)
        .ok()
        .map(|out| out.trim().to_owned())
        .filter(|name| !name.is_empty());

    let range = match &previous {
        Some(previous) => format!("{previous}..{tag}"),
        None => tag.to_owned(),
    };
    let log = git(
        dir,
        &[
            "log",
            "--reverse",
            "--no-merges",
            "--format=%H%x1f%s%x1f%b%x1e",
            &range,
        ],
    )?;
    let commits = log
        .split('\x1e')
        .filter_map(|record| {
            let mut fields = record.trim_start_matches('\n').splitn(3, '\x1f');
            let sha = fields.next().filter(|sha| !sha.is_empty())?;
            Some(Commit {
                sha: sha.to_owned(),
                subject: fields.next().unwrap_or_default().to_owned(),
                body: fields.next().unwrap_or_default().to_owned(),
            })
        })
        .collect();

    // A lightweight tag's contents would be the commit's message.
    let annotated = git(dir, &["cat-file", "-t", &tag_ref]).is_ok_and(|t| t.trim() == "tag");
    let message = if annotated {
        let contents = git(dir, &["tag", "-l", "--format=%(contents)", tag])?;
        let signature = git(dir, &["tag", "-l", "--format=%(contents:signature)", tag])?;
        Some(
            contents
                .strip_suffix(&signature)
                .unwrap_or(&contents)
                .to_owned(),
        )
    } else {
        None
    };

    Ok(Release {
        tag: tag.to_owned(),
        repo: repo.to_owned(),
        message,
        previous,
        commits,
    })
}

pub fn render(release: &Release) -> Result<String, String> {
    let base = format!("https://github.com/{}", release.repo);
    let version = release.tag.strip_prefix('v').unwrap_or(&release.tag);
    let mut parts = Vec::new();

    if let Some((final_version, _)) = version.split_once('-') {
        parts.push(format!(
            "> [!WARNING]\n> This is a pre-release, for trying things out before {final_version} \
             is final. Please report problems in [Issues]({base}/issues)."
        ));
    }
    if let Some(summary) = release
        .message
        .as_deref()
        .and_then(message_after_first_line)
    {
        parts.push(summary);
    }
    parts.push(changes(release, &base));
    Ok(parts.join("\n\n") + "\n")
}

fn message_after_first_line(message: &str) -> Option<String> {
    let rest: Vec<&str> = message.lines().skip(1).collect();
    let text = rest.join("\n");
    let text = text.trim_matches('\n').trim_end();
    (!text.is_empty()).then(|| text.to_owned())
}

struct Subject<'a> {
    kind: &'a str,
    scope: Option<&'a str>,
    breaking: bool,
    description: &'a str,
}

/// A Conventional Commits subject with one of the known types.
fn parse_subject(subject: &str) -> Option<Subject<'_>> {
    let (head, description) = subject.split_once(": ")?;
    let (head, breaking) = match head.strip_suffix('!') {
        Some(head) => (head, true),
        None => (head, false),
    };
    let (kind, scope) = match head.split_once('(') {
        Some((kind, rest)) => (kind, Some(rest.strip_suffix(')')?)),
        None => (head, None),
    };
    let scope_ok = scope.is_none_or(|s| !s.is_empty() && !s.contains(['(', ')']));
    (TYPES.contains(&kind) && scope_ok && !description.trim().is_empty()).then_some(Subject {
        kind,
        scope,
        breaking,
        description: description.trim(),
    })
}

/// The text of a `BREAKING CHANGE:` footer, up to the next blank line or trailer.
fn breaking_change(body: &str) -> Option<String> {
    let mut lines = body.lines();
    let first = lines.by_ref().find_map(|line| {
        line.strip_prefix("BREAKING CHANGE: ")
            .or_else(|| line.strip_prefix("BREAKING-CHANGE: "))
    })?;
    let mut text = vec![first.trim()];
    for line in lines {
        if line.trim().is_empty() || is_trailer(line) {
            break;
        }
        text.push(line.trim());
    }
    Some(text.join(" "))
}

/// `Token: value` or `Token #value`, with no spaces in the token.
fn is_trailer(line: &str) -> bool {
    let token_end = line.find(": ").or_else(|| line.find(" #"));
    token_end.is_some_and(|end| {
        let token = &line[..end];
        !token.is_empty() && token.chars().all(|c| c.is_alphanumeric() || c == '-')
    })
}

/// The commit a revert undoes, from the line `git revert` writes.
fn reverted_sha(commit: &Commit) -> Option<&str> {
    let subject = &commit.subject;
    let is_revert = parse_subject(subject).is_some_and(|s| s.kind == "revert")
        || subject.starts_with("Revert \"");
    if !is_revert {
        return None;
    }
    const MARKER: &str = "This reverts commit ";
    let start = commit.body.find(MARKER)? + MARKER.len();
    let sha = commit.body.get(start..start + 40)?;
    sha.chars().all(|c| c.is_ascii_hexdigit()).then_some(sha)
}

/// Leaves out each revert whose commit is also in the list, and that commit.
fn without_reverts(commits: &[Commit]) -> Vec<&Commit> {
    let shas: HashSet<&str> = commits.iter().map(|c| c.sha.as_str()).collect();
    let mut dropped = HashSet::new();
    for commit in commits {
        if let Some(sha) = reverted_sha(commit).filter(|sha| shas.contains(sha)) {
            dropped.insert(sha);
            dropped.insert(commit.sha.as_str());
        }
    }
    commits
        .iter()
        .filter(|c| !dropped.contains(c.sha.as_str()))
        .collect()
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn changes(release: &Release, base: &str) -> String {
    let mut headed: [Vec<String>; 5] = Default::default();
    let mut other = Vec::new();
    let mut all_typed = true;
    for commit in without_reverts(&release.commits) {
        let short = commit.sha.get(..7).unwrap_or(&commit.sha);
        let link = format!(" ([`{short}`]({base}/commit/{}))", commit.sha);
        let Some(subject) = parse_subject(&commit.subject) else {
            all_typed = false;
            other.push(format!("- {}{link}", commit.subject));
            continue;
        };
        let breaking = breaking_change(&commit.body);
        let section = if subject.breaking || breaking.is_some() {
            Some(0)
        } else {
            match subject.kind {
                "feat" => Some(1),
                "fix" => Some(2),
                "perf" => Some(3),
                "docs" if subject.scope != Some("contributing") => Some(4),
                _ => None,
            }
        };
        match section {
            Some(section) => {
                let scope = subject
                    .scope
                    .map(|scope| format!("**{scope}:** "))
                    .unwrap_or_default();
                let text = breaking.as_deref().unwrap_or(subject.description);
                headed[section].push(format!("- {scope}{}{link}", capitalise(text)));
            }
            None => other.push(format!("- {}{link}", commit.subject)),
        }
    }

    let mut out = String::from("## What's changed");
    for (heading, entries) in HEADINGS.iter().zip(&headed) {
        if !entries.is_empty() {
            write!(out, "\n\n### {heading}\n\n{}", entries.join("\n")).unwrap();
        }
    }
    if headed.iter().all(Vec::is_empty) && all_typed {
        out.push_str("\n\nNo user-facing changes.");
    }
    if !other.is_empty() {
        write!(
            out,
            "\n\n<details>\n<summary>🧰 Other changes ({})</summary>\n\n{}\n\n</details>",
            other.len(),
            other.join("\n")
        )
        .unwrap();
    }
    match &release.previous {
        Some(previous) => write!(
            out,
            "\n\n**Full changelog:** [{previous}...{tag}]({base}/compare/{previous}...{tag})",
            tag = release.tag
        ),
        None => write!(
            out,
            "\n\n**Full changelog:** [all commits]({base}/commits/{})",
            release.tag
        ),
    }
    .unwrap();
    out
}

#[cfg(test)]
mod tests;
