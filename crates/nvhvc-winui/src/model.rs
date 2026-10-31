//! UI state model for the wizard. The pure presentation model (metadata rows, drafts,
//! value formatting) lives in `nvhvc_core::uimodel` and is shared with the GTK front-end.

use nvhvc_core::probe::{ProbedHdr10, VideoInfo};

pub use nvhvc_core::pipeline::Container;
pub use nvhvc_core::uimodel::{
    DEFAULT_OUTPUT_SUFFIX, Draft, MATRIX_OPTS, META_ROWS, PRIMARIES_OPTS, TRANSFER_OPTS,
    code_label, colour_summary, compact_path, default_output, file_name, human_size,
    normalize_output_pick, row_overridden, row_value,
};

/// The output path the pipeline will actually write: the stored (picked or derived) path with
/// its extension following the wizard's container selection. The stored path's own extension
/// is never shown or used directly; everything derives at render/run time.
pub fn effective_output(output: &str, container: Container) -> String {
    if output.is_empty() {
        return String::new();
    }
    std::path::Path::new(output)
        .with_extension(container.extension())
        .display()
        .to_string()
}

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
    pub output: String,
    pub sps_patched: usize,
    pub sei_insertions: usize,
    pub frames: Option<usize>,
    /// Preformatted `"11 · Opus + 9 subtitles"` line, when the container was inspected.
    pub tracks: Option<String>,
    /// Tracks the output container couldn't hold (MP4 only); the result screen lists them.
    pub dropped_tracks: Vec<String>,
}

/// An open metadata-row editor and its unsaved values.
#[derive(Clone, PartialEq)]
pub struct EditState {
    pub row: usize,
    pub draft: Draft,
}

/// Where a picked file's ffprobe stands.
#[derive(Clone, Default, PartialEq)]
pub enum Probe {
    #[default]
    Idle,
    Loading,
    Ready(FileProbe),
    Failed(String),
}

impl Probe {
    pub fn ready(&self) -> Option<&FileProbe> {
        match self {
            Probe::Ready(p) => Some(p),
            _ => None,
        }
    }
}

/// The pipeline run for the current wizard.
#[derive(Clone, Default, PartialEq)]
pub enum Run {
    #[default]
    Idle,
    /// `stage` is the 0..=5 progress index; 5 is the Done hold before the result screen.
    Running {
        token: u32,
        stage: usize,
    },
    Failed(String),
    /// Shown as the result screen until Back or "Patch another" dismisses it.
    Done(RunSummary),
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
