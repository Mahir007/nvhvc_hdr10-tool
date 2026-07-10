//! Persisted user settings (custom tool paths and defaults).
//!
//! Stored as JSON in the platform config directory (`%AppData%\nvhvc-hdr10-tool\` on Windows,
//! `~/.config/nvhvc-hdr10-tool/` on Linux) via the `directories` crate.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::tools::Overrides;

/// Everything the app persists between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Custom executable paths for the external tools (each always wins over auto-detection).
    pub tool_overrides: Overrides,
    /// Default chroma sample location to write, if the user wants to force one.
    pub default_chroma_loc: Option<u32>,
    /// Preferred output container extension (e.g. "mkv"), when not implied by the output path.
    pub default_container: Option<String>,
    /// GUI theme preference: `"system"` (follow the OS), `"light"` or `"dark"`.
    pub ui_theme: Option<String>,
}

impl Settings {
    /// The settings file path, or `None` if no config directory can be determined.
    pub fn config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("io.github", "Mahir007", "nvhvc-hdr10-tool")
            .map(|dirs| dirs.config_dir().join("settings.json"))
    }

    /// Loads settings, falling back to defaults if the file is missing or unreadable.
    pub fn load() -> Self {
        Self::config_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes settings to the config file, creating the directory if needed.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path().ok_or_else(|| Error::Probe {
            what: "config directory",
            detail: "no home/config directory could be determined".to_owned(),
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_preserves_overrides() {
        let mut settings = Settings::default();
        settings.tool_overrides.mkvmerge = Some(PathBuf::from(r"C:\Program Files\MKVToolNix"));
        settings.default_chroma_loc = Some(2);

        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.tool_overrides.mkvmerge,
            settings.tool_overrides.mkvmerge
        );
        assert_eq!(back.default_chroma_loc, Some(2));
    }

    #[test]
    fn partial_json_fills_defaults() {
        // Missing fields should default rather than fail to parse.
        let back: Settings = serde_json::from_str("{}").unwrap();
        assert!(back.tool_overrides.ffmpeg.is_none());
        assert!(back.default_chroma_loc.is_none());
    }
}
