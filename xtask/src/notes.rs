//! Release notes for a tag: a pre-release warning, then the rest of the tag's message.

use std::path::Path;
use std::process::Command;

/// Everything the notes are written from.
#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    /// `owner/name` on GitHub.
    pub repo: String,
    /// The annotated tag's message, if the tag has one.
    pub message: Option<String>,
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

fn collect(dir: &Path, tag: &str, repo: &str) -> Result<Release, String> {
    let tag_ref = format!("refs/tags/{tag}");
    git(dir, &["rev-parse", "--verify", "--quiet", &tag_ref])
        .map_err(|_| format!("there's no tag {tag}"))?;

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
    Ok(parts.join("\n\n") + "\n")
}

fn message_after_first_line(message: &str) -> Option<String> {
    let rest: Vec<&str> = message.lines().skip(1).collect();
    let text = rest.join("\n");
    let text = text.trim_matches('\n').trim_end();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests;
