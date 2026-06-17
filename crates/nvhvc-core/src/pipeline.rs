//! End-to-end orchestration: probe → extract → patch → capture timing → remux.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::hevc::patch::patch_stream;
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
}

/// What a pipeline run produced.
#[derive(Debug, Clone)]
pub struct PipelineReport {
    /// The HDR10 metadata probed from the source and injected.
    pub probed: ProbedHdr10,
    /// Number of SPS NAL units rewritten in the encode.
    pub sps_patched: usize,
    /// Whether an HDR10 SEI was inserted.
    pub sei_inserted: bool,
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
    let ffprobe = tools.require(Tool::Ffprobe)?;
    let ffmpeg = tools.require(Tool::Ffmpeg)?;

    // 1. Probe the source for HDR10 metadata.
    let mut probed = probe_hdr10(ffprobe, source)?;
    if let Some(chroma) = opts.chroma_loc {
        probed.color.chroma_loc = Some(chroma);
    }

    let work = make_workdir()?;
    let extracted = work.join("extracted.hevc");
    let patched = work.join("patched.hevc");
    let timecodes = work.join("timestamps.txt");

    // 2. Extract the raw HEVC from the encode.
    extract::extract_hevc(ffmpeg, encoded, &extracted)?;

    // 3. Patch the bitstream.
    let input = std::fs::read(&extracted)?;
    let (patched_bytes, patch_report) = patch_stream(&input, &probed.to_metadata())?;
    std::fs::write(&patched, &patched_bytes)?;

    // 4. Capture timing and remux into the chosen container.
    let container = output
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    match container.as_deref() {
        Some("mkv") => {
            let mkvmerge = tools.require(Tool::Mkvmerge)?;
            let mkvextract = tools.require(Tool::Mkvextract)?;
            let track_id = timing::video_track_id(mkvmerge, encoded)?;
            timing::extract_timestamps_mkv(mkvextract, encoded, track_id, &timecodes)?;
            remux::remux_mkv(mkvmerge, &patched, encoded, &timecodes, output)?;
        }
        other => {
            return Err(Error::Probe {
                what: "output container",
                detail: format!(
                    "only .mkv output is implemented so far (got {other:?}); MP4/ffmpeg backends are next"
                ),
            });
        }
    }

    if !opts.keep_intermediates {
        let _ = std::fs::remove_dir_all(&work);
    }

    Ok(PipelineReport {
        probed,
        sps_patched: patch_report.sps_patched,
        sei_inserted: patch_report.sei_inserted,
        output: output.to_path_buf(),
    })
}

/// Creates a per-process scratch directory for intermediate files.
fn make_workdir() -> Result<PathBuf> {
    let base = std::env::temp_dir().join(format!("nvhvc-{}", std::process::id()));
    std::fs::create_dir_all(&base)?;
    Ok(base)
}
