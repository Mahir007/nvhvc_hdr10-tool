//! The wizard's mutable state, shared by all handlers via `Rc<Ctx>` and re-rendered
//! wholesale after every change (the widget tree is cheap to rebuild).

use std::path::PathBuf;

use nvhvc_core::config::Settings;
use nvhvc_core::hevc::patch::Hdr10Metadata;
use nvhvc_core::pipeline::{Container, Stage as PipeStage};
use nvhvc_core::probe::{ProbedHdr10, VideoInfo};
use nvhvc_core::uimodel::Draft;

/// Everything probed about a picked file.
#[derive(Clone)]
pub struct FileInfo {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub info: VideoInfo,
    pub hdr: ProbedHdr10,
}

impl FileInfo {
    /// `"path · 518 MB · 3840×2160 · HEVC · HDR10"`-style card detail line.
    pub fn detail(&self) -> String {
        let mut s = format!(
            "{} · {}",
            nvhvc_core::uimodel::compact_path(&self.path.display().to_string()),
            nvhvc_core::uimodel::human_size(self.size_bytes)
        );
        if self.info.width > 0 {
            s.push_str(&format!(" · {}×{}", self.info.width, self.info.height));
        }
        if !self.info.codec.is_empty() {
            s.push_str(&format!(" · {}", self.info.codec.to_uppercase()));
        }
        if self.hdr.has_hdr10() {
            s.push_str(" · HDR10");
        }
        s
    }
}

/// What a finished pipeline run left behind (for the result screen).
#[derive(Clone)]
pub struct RunOutcome {
    pub output: PathBuf,
    pub sps_patched: usize,
    pub sei_insertions: usize,
    pub frames: Option<usize>,
    pub tracks: Option<nvhvc_core::timing::MkvTracks>,
    /// Tracks the output container couldn't hold (MP4 only); the result screen lists them.
    pub dropped_tracks: Vec<String>,
}

/// The whole wizard state.
#[derive(Default)]
pub struct AppState {
    /// 1 = source, 2 = encode, 3 = result.
    pub stage: u8,
    pub source: Option<FileInfo>,
    pub src_probing: bool,
    pub src_error: Option<String>,
    pub encode: Option<FileInfo>,
    pub enc_probing: bool,
    pub enc_error: Option<String>,
    pub output: Option<PathBuf>,
    /// Whether the user picked `output` via the save dialog (which already warns about
    /// overwriting); auto-derived names get our own confirmation instead.
    pub output_user_chosen: bool,
    /// The output container for this wizard run. Starts at the persisted default; the
    /// stage-2 dropdown and a save-dialog pick with an explicit .mkv/.mp4 both change it.
    pub container: Container,
    /// Edited HDR10 metadata; `None` = detected values as-is.
    pub overrides: Option<Hdr10Metadata>,
    /// Open metadata-row editor: row index + field draft.
    pub editing: Option<(usize, Draft)>,
    /// Pipeline stage while a run is in flight.
    pub processing: Option<PipeStage>,
    pub run_error: Option<String>,
    pub result: Option<RunOutcome>,
    /// Monotonic guard so a stale probe result can't clobber a newer pick.
    pub probe_seq: u64,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            stage: 1,
            container: Container::from_setting(Settings::load().default_container.as_deref()),
            ..Default::default()
        }
    }

    /// The metadata detected from the current source.
    pub fn detected(&self) -> Option<Hdr10Metadata> {
        self.source.as_ref().map(|f| f.hdr.to_metadata())
    }

    /// The metadata that would be injected (overrides win over detected).
    pub fn effective(&self) -> Option<Hdr10Metadata> {
        self.overrides.or_else(|| self.detected())
    }

    /// Resets everything for "Patch another". Only ever called from the completed result
    /// screen, where no probe is in flight — and `start_probe` bumps `probe_seq` before every
    /// probe anyway, so a fresh (zeroed) counter still discards any stale result.
    pub fn reset_wizard(&mut self) {
        *self = AppState::new();
    }
}
