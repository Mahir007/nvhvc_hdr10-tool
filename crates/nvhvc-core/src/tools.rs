//! Discovery and invocation of the external command-line tools the pipeline shells out to.
//!
//! Resolution order for each tool: an explicit user override (a full path to the executable, or a
//! directory containing it) → the system `PATH` → a few well-known install locations. This means a
//! user-configured path always wins, even when a system copy exists (as requested).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

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

    /// A hint for the user on how to provide this tool when it is missing.
    pub fn install_hint(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "install ffmpeg or set a custom path in settings",
            Tool::Ffprobe => "install ffmpeg (provides ffprobe) or set a custom path in settings",
            Tool::Mkvmerge | Tool::Mkvextract => {
                "install MKVToolNix or set a custom path in settings"
            }
            Tool::Mp4box => "install GPAC (MP4Box) or set a custom path in settings",
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

/// How a tool was resolved — surfaced so a UI can show *where* each tool came from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Resolution {
    /// Found via the user's explicit override.
    Override(PathBuf),
    /// Found on the system `PATH`.
    OnPath(PathBuf),
    /// Found at a well-known install location (not on `PATH`).
    Fallback(PathBuf),
    /// An override was set, but the path doesn't point to an executable. Kept (with the bad
    /// path) so the UI can warn about it instead of silently falling back to `PATH`.
    OverrideMissing(PathBuf),
    /// Not found anywhere.
    #[default]
    NotFound,
}

impl Resolution {
    /// The resolved executable path, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Resolution::Override(p) | Resolution::OnPath(p) | Resolution::Fallback(p) => Some(p),
            Resolution::OverrideMissing(_) | Resolution::NotFound => None,
        }
    }

    /// Whether the tool was found.
    pub fn is_found(&self) -> bool {
        self.path().is_some()
    }
}

/// Resolves a tool, reporting *how* it was found: override → `PATH` → well-known install dirs.
///
/// `override_path` may point directly at the executable or at a directory that contains it.
pub fn resolve_detailed(tool: Tool, override_path: Option<&Path>) -> Resolution {
    // 1. Explicit override (file or containing directory). An explicit override always wins: if
    //    it's set but resolves to nothing, report it missing rather than silently using `PATH`.
    if let Some(p) = override_path {
        if p.is_file() {
            return Resolution::Override(p.to_path_buf());
        }
        if p.is_dir() {
            for name in tool.program_names() {
                for candidate in [p.join(name), p.join(format!("{name}.exe"))] {
                    if candidate.is_file() {
                        return Resolution::Override(candidate);
                    }
                }
            }
        }
        return Resolution::OverrideMissing(p.to_path_buf());
    }

    // 2. PATH.
    for name in tool.program_names() {
        if let Ok(found) = which::which(name) {
            return Resolution::OnPath(found);
        }
    }

    // 3. Well-known install locations.
    if let Some(found) = tool.common_locations().into_iter().find(|c| c.is_file()) {
        return Resolution::Fallback(found);
    }

    Resolution::NotFound
}

/// Resolves a tool to a path (override → `PATH` → well-known dirs), discarding the source.
pub fn resolve(tool: Tool, override_path: Option<&Path>) -> Option<PathBuf> {
    resolve_detailed(tool, override_path)
        .path()
        .map(Path::to_path_buf)
}

/// Per-tool override paths, typically loaded from the user's settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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

/// A resolved set of tools, each carrying *how* it was found.
#[derive(Debug, Clone, Default)]
pub struct Toolset {
    ffmpeg: Resolution,
    ffprobe: Resolution,
    mkvmerge: Resolution,
    mkvextract: Resolution,
    mp4box: Resolution,
}

impl Toolset {
    /// Resolves every tool, applying the given overrides.
    pub fn discover(overrides: &Overrides) -> Self {
        let r = |t: Tool| resolve_detailed(t, overrides.for_tool(t));
        Self {
            ffmpeg: r(Tool::Ffmpeg),
            ffprobe: r(Tool::Ffprobe),
            mkvmerge: r(Tool::Mkvmerge),
            mkvextract: r(Tool::Mkvextract),
            mp4box: r(Tool::Mp4box),
        }
    }

    /// How `tool` was resolved.
    pub fn resolution(&self, tool: Tool) -> &Resolution {
        match tool {
            Tool::Ffmpeg => &self.ffmpeg,
            Tool::Ffprobe => &self.ffprobe,
            Tool::Mkvmerge => &self.mkvmerge,
            Tool::Mkvextract => &self.mkvextract,
            Tool::Mp4box => &self.mp4box,
        }
    }

    /// Every tool paired with how it was resolved, in display order (for listing in a UI).
    pub fn all(&self) -> [(Tool, &Resolution); 5] {
        [
            (Tool::Ffmpeg, &self.ffmpeg),
            (Tool::Ffprobe, &self.ffprobe),
            (Tool::Mkvmerge, &self.mkvmerge),
            (Tool::Mkvextract, &self.mkvextract),
            (Tool::Mp4box, &self.mp4box),
        ]
    }

    /// Returns the resolved path for `tool`, or a [`Error::ToolNotFound`] with a fix hint.
    pub fn require(&self, tool: Tool) -> Result<&Path> {
        self.resolution(tool).path().ok_or(Error::ToolNotFound {
            tool: tool.label(),
            hint: tool.install_hint(),
        })
    }

    /// Whether `tool` was found.
    pub fn has(&self, tool: Tool) -> bool {
        self.resolution(tool).is_found()
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
    let mut cmd = Command::new(program);
    cmd.args(args);
    // Console children pop up their own console window when the parent is a GUI app — every
    // pipeline stage flashed a terminal on Windows (and paid the console-creation cost). All
    // output is piped, so the children never need a console.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output()?;
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
    fn nonexistent_override_is_reported_missing() {
        // An explicit but broken override must NOT silently fall back to PATH — it resolves to
        // `OverrideMissing` (carrying the bad path) and yields no usable executable.
        let bogus = Path::new("Z:/definitely/not/here/ffprobe.exe");
        assert_eq!(
            resolve_detailed(Tool::Ffprobe, Some(bogus)),
            Resolution::OverrideMissing(bogus.to_path_buf())
        );
        assert_eq!(resolve(Tool::Ffprobe, Some(bogus)), None);
    }

    #[test]
    fn require_reports_missing() {
        let empty = Toolset::default();
        let err = empty.require(Tool::Ffmpeg).unwrap_err();
        assert!(matches!(err, Error::ToolNotFound { tool: "ffmpeg", .. }));
    }
}
