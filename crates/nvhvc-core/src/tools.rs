//! Discovery and invocation of the external command-line tools the pipeline shells out to.
//!
//! Resolution order for each tool: an explicit user override (a full path to the executable, or a
//! directory containing it) → the system `PATH` → a few well-known install locations. This means a
//! user-configured path always wins, even when a system copy exists (as requested).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// An external tool the pipeline can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// `ffmpeg` — stream extraction and the universal remux fallback.
    Ffmpeg,
    /// `ffprobe` — source metadata probing.
    Ffprobe,
    /// `mkvmerge` — Matroska remux.
    Mkvmerge,
    /// `mkvextract` — Matroska timestamp extraction.
    Mkvextract,
    /// `MP4Box` (GPAC) — MP4 remux.
    Mp4box,
}

impl Tool {
    /// A stable, human-readable label (also used in error messages).
    pub fn label(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ffprobe => "ffprobe",
            Tool::Mkvmerge => "mkvmerge",
            Tool::Mkvextract => "mkvextract",
            Tool::Mp4box => "MP4Box",
        }
    }

    /// Candidate executable base names to look for on `PATH` (case variations included).
    fn program_names(self) -> &'static [&'static str] {
        match self {
            Tool::Ffmpeg => &["ffmpeg"],
            Tool::Ffprobe => &["ffprobe"],
            Tool::Mkvmerge => &["mkvmerge"],
            Tool::Mkvextract => &["mkvextract"],
            Tool::Mp4box => &["MP4Box", "mp4box"],
        }
    }

    /// Well-known install locations to check when the tool is not on `PATH` (mainly Windows, where
    /// MKVToolNix and GPAC do not add themselves to `PATH`).
    fn common_locations(self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        match self {
            Tool::Mkvmerge | Tool::Mkvextract => {
                let exe = format!("{}.exe", self.label());
                for pf in &program_files {
                    out.push(pf.join("MKVToolNix").join(&exe));
                }
            }
            Tool::Mp4box => {
                for pf in &program_files {
                    out.push(pf.join("GPAC").join("MP4Box.exe"));
                    out.push(pf.join("GPAC").join("mp4box.exe"));
                }
            }
            Tool::Ffmpeg | Tool::Ffprobe => {
                let exe = format!("{}.exe", self.label());
                for pf in &program_files {
                    out.push(pf.join("ffmpeg").join("bin").join(&exe));
                }
            }
        }
        out
    }
}

/// Resolves a single tool to an executable path, honouring an optional user override.
///
/// `override_path` may point directly at the executable or at a directory that contains it.
pub fn resolve(tool: Tool, override_path: Option<&Path>) -> Option<PathBuf> {
    // 1. Explicit override (file or containing directory).
    if let Some(p) = override_path {
        if p.is_file() {
            return Some(p.to_path_buf());
        }
        if p.is_dir() {
            for name in tool.program_names() {
                for candidate in [p.join(name), p.join(format!("{name}.exe"))] {
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    // 2. PATH.
    for name in tool.program_names() {
        if let Ok(found) = which::which(name) {
            return Some(found);
        }
    }

    // 3. Well-known install locations.
    tool.common_locations().into_iter().find(|c| c.is_file())
}

/// Per-tool override paths, typically loaded from the user's settings.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    /// Override for `ffmpeg`.
    pub ffmpeg: Option<PathBuf>,
    /// Override for `ffprobe`.
    pub ffprobe: Option<PathBuf>,
    /// Override for `mkvmerge`.
    pub mkvmerge: Option<PathBuf>,
    /// Override for `mkvextract`.
    pub mkvextract: Option<PathBuf>,
    /// Override for `MP4Box`.
    pub mp4box: Option<PathBuf>,
}

impl Overrides {
    fn for_tool(&self, tool: Tool) -> Option<&Path> {
        match tool {
            Tool::Ffmpeg => self.ffmpeg.as_deref(),
            Tool::Ffprobe => self.ffprobe.as_deref(),
            Tool::Mkvmerge => self.mkvmerge.as_deref(),
            Tool::Mkvextract => self.mkvextract.as_deref(),
            Tool::Mp4box => self.mp4box.as_deref(),
        }
    }
}

/// A resolved set of tools: every tool's location (or `None` if unavailable).
#[derive(Debug, Clone, Default)]
pub struct Toolset {
    /// `ffmpeg`.
    pub ffmpeg: Option<PathBuf>,
    /// `ffprobe`.
    pub ffprobe: Option<PathBuf>,
    /// `mkvmerge`.
    pub mkvmerge: Option<PathBuf>,
    /// `mkvextract`.
    pub mkvextract: Option<PathBuf>,
    /// `MP4Box`.
    pub mp4box: Option<PathBuf>,
}

impl Toolset {
    /// Resolves every tool, applying the given overrides. Missing tools are simply `None`.
    pub fn discover(overrides: &Overrides) -> Self {
        let r = |t: Tool| resolve(t, overrides.for_tool(t));
        Self {
            ffmpeg: r(Tool::Ffmpeg),
            ffprobe: r(Tool::Ffprobe),
            mkvmerge: r(Tool::Mkvmerge),
            mkvextract: r(Tool::Mkvextract),
            mp4box: r(Tool::Mp4box),
        }
    }

    /// Returns the resolved path for `tool`, or a [`Error::ToolNotFound`] with a fix hint.
    pub fn require(&self, tool: Tool) -> Result<&Path> {
        let (slot, hint) = match tool {
            Tool::Ffmpeg => (
                &self.ffmpeg,
                "install ffmpeg or set a custom path in settings",
            ),
            Tool::Ffprobe => (
                &self.ffprobe,
                "install ffmpeg (which provides ffprobe) or set a custom path in settings",
            ),
            Tool::Mkvmerge => (
                &self.mkvmerge,
                "install MKVToolNix or set a custom path in settings",
            ),
            Tool::Mkvextract => (
                &self.mkvextract,
                "install MKVToolNix or set a custom path in settings",
            ),
            Tool::Mp4box => (
                &self.mp4box,
                "install GPAC (MP4Box) or set a custom path in settings",
            ),
        };
        slot.as_deref().ok_or(Error::ToolNotFound {
            tool: tool.label(),
            hint,
        })
    }

    /// Whether `tool` was found.
    pub fn has(&self, tool: Tool) -> bool {
        match tool {
            Tool::Ffmpeg => self.ffmpeg.is_some(),
            Tool::Ffprobe => self.ffprobe.is_some(),
            Tool::Mkvmerge => self.mkvmerge.is_some(),
            Tool::Mkvextract => self.mkvextract.is_some(),
            Tool::Mp4box => self.mp4box.is_some(),
        }
    }
}

/// Renders a path as a command-line argument, erroring if it is not valid UTF-8.
pub fn path_arg(p: &Path) -> Result<&str> {
    p.to_str().ok_or_else(|| Error::Probe {
        what: "file path",
        detail: format!("{} is not valid UTF-8", p.display()),
    })
}

/// Runs a tool to completion, returning captured stdout. Errors include captured stderr.
pub fn run_capture(program: &Path, tool: Tool, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            tool: tool.label(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output.stdout)
}

/// Runs a tool to completion, discarding stdout. Errors include captured stderr.
pub fn run(program: &Path, tool: Tool, args: &[&str]) -> Result<()> {
    run_capture(program, tool, args).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_stable() {
        assert_eq!(Tool::Ffprobe.label(), "ffprobe");
        assert_eq!(Tool::Mkvmerge.label(), "mkvmerge");
    }

    #[test]
    fn nonexistent_override_falls_through() {
        // A bogus override should not resolve to itself.
        let bogus = Path::new("Z:/definitely/not/here/ffprobe.exe");
        let resolved = resolve(Tool::Ffprobe, Some(bogus));
        assert_ne!(resolved.as_deref(), Some(bogus));
    }

    #[test]
    fn require_reports_missing() {
        let empty = Toolset::default();
        let err = empty.require(Tool::Ffmpeg).unwrap_err();
        assert!(matches!(err, Error::ToolNotFound { tool: "ffmpeg", .. }));
    }
}
