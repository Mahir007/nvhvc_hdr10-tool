//! End-to-end orchestration: probe → extract → patch → capture timing → remux.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::hevc::patch::{Hdr10Metadata, patch_stream};
use crate::probe::{ProbedHdr10, probe_hdr10};
use crate::tools::{Tool, Toolset};
use crate::{extract, remux, timing};

/// Knobs for a pipeline run.
#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    /// Override the chroma sample location written to the VUI. `None` preserves the encode's own.
    pub chroma_loc: Option<u32>,
    /// Keep the intermediate extracted/patched/timecode files instead of deleting them.
    pub keep_intermediates: bool,
    /// Inject these values instead of the source's probed metadata (user overrides from a GUI).
    /// The source is still probed for the report; only the injected signalling changes.
    pub metadata_override: Option<Hdr10Metadata>,
}

/// Stages emitted by the pipeline so a front-end can show progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Reading HDR10 metadata from the source.
    Probing,
    /// Extracting the raw HEVC from the encode.
    Extracting,
    /// Editing the bitstream (SPS VUI + SEI).
    Patching,
    /// Capturing the encode's per-frame timing.
    CapturingTiming,
    /// Muxing the patched video back into a container.
    Remuxing,
    /// Finished.
    Done,
}

impl Stage {
    /// A short human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Stage::Probing => "Probing source metadata",
            Stage::Extracting => "Extracting HEVC stream",
            Stage::Patching => "Patching bitstream",
            Stage::CapturingTiming => "Capturing timing",
            Stage::Remuxing => "Remuxing",
            Stage::Done => "Done",
        }
    }
}

/// What a pipeline run produced.
#[derive(Debug, Clone)]
pub struct PipelineReport {
    /// The HDR10 metadata probed from the source.
    pub probed: ProbedHdr10,
    /// The metadata actually written into the encode (the probe, or the caller's override).
    pub injected: Hdr10Metadata,
    /// Number of SPS NAL units rewritten in the encode.
    pub sps_patched: usize,
    /// Whether an HDR10 SEI was inserted.
    pub sei_inserted: bool,
    /// Number of video frames whose timestamps were captured and reproduced (when known).
    pub frames: Option<usize>,
    /// The finished output file.
    pub output: PathBuf,
}

/// Runs the full patch pipeline: read HDR10 from `source`, inject it into `encoded`, write `output`.
///
/// The output container is chosen from `output`'s extension. Timing is taken from `encoded` and
/// reproduced exactly; its non-video tracks are carried over.
pub fn run(
    tools: &Toolset,
    source: &Path,
    encoded: &Path,
    output: &Path,
    opts: &PipelineOptions,
) -> Result<PipelineReport> {
    run_with_progress(tools, source, encoded, output, opts, &mut |_| {})
}

/// Like [`run`], but reports each [`Stage`] to `progress` as it begins.
pub fn run_with_progress(
    tools: &Toolset,
    source: &Path,
    encoded: &Path,
    output: &Path,
    opts: &PipelineOptions,
    progress: &mut dyn FnMut(Stage),
) -> Result<PipelineReport> {
    let ffprobe = tools.require(Tool::Ffprobe)?;
    let ffmpeg = tools.require(Tool::Ffmpeg)?;

    // 1. Probe the source for HDR10 metadata.
    progress(Stage::Probing);
    let mut probed = probe_hdr10(ffprobe, source)?;
    if let Some(chroma) = opts.chroma_loc {
        probed.color.chroma_loc = Some(chroma);
    }
    // The user's override (if any) wins over the probe; an explicit chroma_loc wins over both.
    let mut metadata = opts
        .metadata_override
        .unwrap_or_else(|| probed.to_metadata());
    if let Some(chroma) = opts.chroma_loc {
        metadata.color.chroma_loc = Some(chroma);
    }

    let work = make_workdir()?;
    let extracted = work.join("extracted.hevc");
    let patched = work.join("patched.hevc");
    let timecodes = work.join("timestamps.txt");

    // 2. Extract the raw HEVC from the encode.
    progress(Stage::Extracting);
    extract::extract_hevc(ffmpeg, encoded, &extracted)?;

    // 3. Patch the bitstream.
    progress(Stage::Patching);
    let input = std::fs::read(&extracted)?;
    let (patched_bytes, patch_report) = patch_stream(&input, &metadata)?;
    std::fs::write(&patched, &patched_bytes)?;

    // 4. Capture timing and remux into the chosen container.
    let container = output
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    let frames = match container.as_deref() {
        Some("mkv") => {
            let mkvmerge = tools.require(Tool::Mkvmerge)?;
            let mkvextract = tools.require(Tool::Mkvextract)?;
            progress(Stage::CapturingTiming);
            let track_id = timing::video_track_id(mkvmerge, encoded)?;
            timing::extract_timestamps_mkv(mkvextract, encoded, track_id, &timecodes)?;
            // One timestamp per frame, plus one: mkvextract appends the end timestamp of the
            // final frame (so its duration survives the remux). Skip the "# timestamp format
            // v2" header and blank lines, then subtract that trailing end marker.
            let frames = std::fs::read_to_string(&timecodes).ok().map(|t| {
                t.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .count()
                    .saturating_sub(1)
            });
            progress(Stage::Remuxing);
            remux::remux_mkv(mkvmerge, &patched, encoded, &timecodes, output)?;
            frames
        }
        other => {
            return Err(Error::Probe {
                what: "output container",
                detail: format!(
                    "only .mkv output is implemented so far (got {other:?}); MP4/ffmpeg backends are next"
                ),
            });
        }
    };

    if !opts.keep_intermediates {
        let _ = std::fs::remove_dir_all(&work);
    }

    progress(Stage::Done);
    Ok(PipelineReport {
        probed,
        injected: metadata,
        sps_patched: patch_report.sps_patched,
        sei_inserted: patch_report.sei_inserted,
        frames,
        output: output.to_path_buf(),
    })
}

/// Creates a per-process scratch directory for intermediate files.
fn make_workdir() -> Result<PathBuf> {
    let base = std::env::temp_dir().join(format!("nvhvc-{}", std::process::id()));
    std::fs::create_dir_all(&base)?;
    Ok(base)
}
