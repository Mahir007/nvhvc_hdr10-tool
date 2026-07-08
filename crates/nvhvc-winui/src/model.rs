//! UI state model for the wizard. The pure presentation model (metadata rows, drafts,
//! value formatting) lives in `nvhvc_core::uimodel` and is shared with the GTK front-end.

use nvhvc_core::hevc::patch::Hdr10Metadata;
use nvhvc_core::probe::{ProbedHdr10, VideoInfo};

pub use nvhvc_core::uimodel::{
    Draft, MATRIX_OPTS, META_ROWS, PRIMARIES_OPTS, TRANSFER_OPTS, code_label, colour_summary,
    compact_path, default_output, file_name, human_size, row_overridden, row_value,
};

/// Which wizard stage is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Stage {
    #[default]
    Source,
    Encode,
    Result,
}

/// Theme preference, persisted via `Settings.ui_theme`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn from_setting(s: Option<&str>) -> Self {
        match s {
            Some("light") => ThemeMode::Light,
            Some("dark") => ThemeMode::Dark,
            _ => ThemeMode::System,
        }
    }

    pub fn as_setting(self) -> &'static str {
        match self {
            ThemeMode::System => "system",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }
}

/// Everything probed about a picked file, produced on a worker thread.
#[derive(Clone, PartialEq, Eq)]
pub struct FileProbe {
    /// The path this probe belongs to (guards against stale results after "Change").
    pub path: String,
    pub size_bytes: u64,
    pub info: VideoInfo,
    pub hdr: ProbedHdr10,
}

impl FileProbe {
    /// Whether the file carries full HDR10 signalling (mastering-display SEI present).
    pub fn has_hdr10(&self) -> bool {
        self.hdr.has_hdr10()
    }
}

/// The outcome of a pipeline run, reduced to what the result screen shows.
#[derive(Clone, PartialEq, Eq)]
pub struct RunSummary {
    /// Matches the `run_token` of the launch that produced it (guards stale results).
    pub token: u32,
    pub output: String,
    pub sps_patched: usize,
    pub sei_inserted: bool,
    pub frames: Option<usize>,
    /// Preformatted `"11 · Opus + 9 subtitles"` line, when the container was inspected.
    pub tracks: Option<String>,
    /// The remux fell back to ffmpeg (MKVToolNix absent); the result screen notes it.
    pub used_ffmpeg_fallback: bool,
}

/// An open metadata-row editor. Tagged with the source path so a drag-dropped
/// file swap (which can't touch this UI-thread state) invalidates it automatically.
#[derive(Clone, PartialEq)]
pub struct EditState {
    pub path: String,
    pub row: usize,
    pub draft: Draft,
}

/// All UI-thread wizard state (one `use_reducer` slot).
///
/// The picked file *paths* live in `use_async_state` slots instead (their setters are
/// `Send + Sync`, which drag-and-drop handlers require); anything here that depends on a
/// path carries that path so it self-invalidates when the file changes under it.
#[derive(Clone, PartialEq, Default)]
pub struct Ui {
    pub stage: Stage,
    /// Edited HDR10 metadata for a given source path; `None` = detected values as-is.
    pub overrides: Option<(String, Hdr10Metadata)>,
    /// The metadata row editor currently open, if any.
    pub editing: Option<EditState>,
    pub prefs_open: bool,
    pub about_open: bool,
    /// Preferences: tool row being edited (index into `Toolset::all()`) and its draft text.
    pub tool_edit: Option<(usize, String)>,
    /// The output path the user explicitly chose via the save dialog (which already warns
    /// about overwriting). Auto-derived names that already exist get our own confirmation.
    pub output_chosen_for: Option<String>,
    /// An overwrite confirmation dialog is showing for the current run request.
    pub confirm_overwrite: bool,
    pub theme: ThemeMode,
    /// Incremented for every pipeline launch; the matching [`RunSummary::token`] marks
    /// the result screen as current.
    pub run_token: u32,
    /// Results with `token <= ack_token` are dismissed (Back from the result screen).
    pub ack_token: u32,
    /// Bumped when custom tool paths change so Preferences re-discovers the toolset.
    pub tools_rev: u32,
    /// The custom (Border-based) action button the pointer is over, by id. Reactor buttons
    /// repaint their background from theme visual states on hover, so custom-styled buttons
    /// are Borders and track their hover here instead.
    pub hovered_action: Option<&'static str>,
    /// The app menu ("…") overlay is showing.
    pub menu_open: bool,
}

impl Ui {
    /// The effective metadata override for `source`, if one is set and still valid.
    pub fn override_for(&self, source: &str) -> Option<Hdr10Metadata> {
        match &self.overrides {
            Some((p, m)) if p == source => Some(*m),
            _ => None,
        }
    }

    /// The open editor for `source`, if any and still valid.
    pub fn editing_for(&self, source: &str) -> Option<&EditState> {
        self.editing.as_ref().filter(|e| e.path == source)
    }
}

/// `"~\…\scratch\source.mkv · 518 MB · 3840×2160 · HEVC · HDR10"`-style card detail line.
pub fn file_detail(path: &str, probe: Option<&FileProbe>) -> String {
    let path = compact_path(path);
    match probe {
        Some(p) => {
            let mut s = format!("{path} · {}", human_size(p.size_bytes));
            if p.info.width > 0 {
                s.push_str(&format!(" · {}×{}", p.info.width, p.info.height));
            }
            if !p.info.codec.is_empty() {
                s.push_str(&format!(" · {}", p.info.codec.to_uppercase()));
            }
            if p.has_hdr10() {
                s.push_str(" · HDR10");
            }
            s
        }
        None => path,
    }
}
