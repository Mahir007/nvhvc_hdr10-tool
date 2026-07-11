//! End-to-end orchestration: probe → extract → patch → capture timing → remux.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::hevc::patch::{Hdr10Metadata, patch_stream};
use crate::probe::{ProbedHdr10, probe_hdr10};
use crate::tools::{Tool, Toolset};
use crate::{extract, remux, timing};

/// Output container formats the pipeline can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Container {
    /// Matroska — holds every track type; the lossless default.
    #[default]
    Mkv,
    /// MP4 — broad device compatibility, but only carries MP4-compatible tracks.
    Mp4,
}

impl Container {
    /// Every supported container, in display order.
    pub const ALL: [Container; 2] = [Container::Mkv, Container::Mp4];

    /// The container implied by a path's extension, if it's one we can write.
    pub fn from_path(path: &Path) -> Option<Container> {
        match path
            .extension()
            .and_then(|e| e.to_str())?
            .to_ascii_lowercase()
            .as_str()
        {
            "mkv" => Some(Container::Mkv),
            "mp4" => Some(Container::Mp4),
            _ => None,
        }
    }

    /// The file extension (without the dot).
    pub fn extension(self) -> &'static str {
        match self {
            Container::Mkv => "mkv",
            Container::Mp4 => "mp4",
        }
    }

    /// Human-readable label for pickers and summaries.
    pub fn label(self) -> &'static str {
        match self {
            Container::Mkv => "Matroska (.mkv)",
            Container::Mp4 => "MP4 (.mp4)",
        }
    }

    /// Parses the persisted `Settings::default_container` value (unknown → Mkv).
    pub fn from_setting(s: Option<&str>) -> Container {
        match s {
            Some("mp4") => Container::Mp4,
            _ => Container::Mkv,
        }
    }

    /// The value persisted in `Settings::default_container`.
    pub fn as_setting(self) -> &'static str {
        self.extension()
    }
}

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
    /// The encode's track inventory that the remux carried over (when known).
    pub tracks: Option<crate::timing::MkvTracks>,
    /// Whether the remux fell back to ffmpeg (MKVToolNix absent). Only happens for encodes ffmpeg
    /// re-times exactly (no B-frames + CFR); a front-end should still surface that it was used.
    pub used_ffmpeg_fallback: bool,
    /// Human-readable descriptions of tracks the output container couldn't hold (MP4 only;
    /// always empty for Matroska). E.g. `["2 PGS subtitles", "1 attachment"]`.
    pub dropped_tracks: Vec<String>,
    /// The finished output file.
    pub output: PathBuf,
}

/// Runs the full patch pipeline: read HDR10 from `source`, inject it into `encoded`, write `output`.
///
/// The output container is chosen from `output`'s extension (`.mkv` or `.mp4`). Timing is taken
/// from `encoded` and reproduced exactly; its non-video tracks are carried over (for MP4, only
/// the tracks MP4 can hold — the rest are reported in [`PipelineReport::dropped_tracks`]).
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

    // 4. Capture timing and remux into the chosen container. The mkvmerge path reproduces the
    //    encode's exact per-frame timing. If MKVToolNix is absent we fall back to ffmpeg, but only
    //    for encodes it can re-time exactly (no B-frames + CFR) — otherwise MKVToolNix is required.
    let container = Container::from_path(output).ok_or_else(|| Error::Probe {
        what: "output container",
        detail: format!(
            "output must be .mkv or .mp4 (got {:?})",
            output.extension().and_then(|e| e.to_str()).unwrap_or("")
        ),
    })?;
    // Timing comes from mkvextract for Matroska encodes (bit-exact, validated) and from
    // ffprobe-synthesized packet timestamps for anything else (mkvextract only reads Matroska).
    let matroska_input = timing::is_matroska(encoded);
    let merge_ready = tools.resolution(Tool::Mkvmerge).is_found()
        && (!matroska_input || tools.resolution(Tool::Mkvextract).is_found());
    let (frames, tracks, used_ffmpeg_fallback, dropped_tracks) = if merge_ready {
        let mkvmerge = tools.require(Tool::Mkvmerge)?;
        progress(Stage::CapturingTiming);
        let mkv_tracks = timing::identify_mkv(mkvmerge, encoded)?;
        if matroska_input {
            let mkvextract = tools.require(Tool::Mkvextract)?;
            timing::extract_timestamps_mkv(
                mkvextract,
                encoded,
                mkv_tracks.video_track_id,
                &timecodes,
            )?;
        } else {
            timing::extract_timestamps_ffprobe(ffprobe, encoded, &timecodes)?;
        }
        // One timestamp per frame, plus one: the v2 file ends with the final frame's end
        // timestamp (so its duration survives the remux). Skip the "# timestamp format
        // v2" header and blank lines, then subtract that trailing end marker.
        let frames = std::fs::read_to_string(&timecodes).ok().map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .count()
                .saturating_sub(1)
        });
        progress(Stage::Remuxing);
        match container {
            Container::Mkv => {
                remux::remux_mkv(mkvmerge, &patched, encoded, &timecodes, output)?;
                (frames, Some(mkv_tracks), false, Vec::new())
            }
            Container::Mp4 => {
                // mkvmerge is the only muxer that can re-apply the captured per-frame timing,
                // so the retimed video goes through a Matroska intermediate; ffmpeg then joins
                // it with the encode's MP4-compatible tracks (stream copy, exact timestamps).
                let retimed = work.join("retimed.mkv");
                remux::remux_mkv_video_only(mkvmerge, &patched, &timecodes, &retimed)?;
                let plan = crate::probe::mp4_stream_plan(ffprobe, encoded)?;
                remux::remux_mp4(ffmpeg, &retimed, encoded, &plan, output)?;
                let carried = plan.tracks(mkv_tracks.video_track_id);
                (frames, Some(carried), false, plan.dropped)
            }
        }
    } else {
        match crate::probe::ffmpeg_cfr_remuxable(ffprobe, encoded)? {
            // MKVToolNix missing but the encode has no B-frames and constant frame rate, so an
            // ffmpeg remux reproduces its timing exactly.
            Some(fps) => {
                let temp_mp4 = work.join("fallback.mp4");
                progress(Stage::Remuxing);
                match container {
                    Container::Mkv => {
                        remux::remux_ffmpeg(ffmpeg, &patched, encoded, &fps, &temp_mp4, output)?;
                        (None, None, true, Vec::new())
                    }
                    Container::Mp4 => {
                        remux::ffmpeg_wrap_cfr(ffmpeg, &patched, &fps, &temp_mp4)?;
                        let plan = crate::probe::mp4_stream_plan(ffprobe, encoded)?;
                        remux::remux_mp4(ffmpeg, &temp_mp4, encoded, &plan, output)?;
                        let carried = plan.tracks(0);
                        (None, Some(carried), true, plan.dropped)
                    }
                }
            }
            // B-frames or VFR: only MKVToolNix can remux this losslessly. Surface the missing tool.
            None => {
                tools.require(Tool::Mkvmerge)?;
                tools.require(Tool::Mkvextract)?;
                unreachable!("merge_ready was false, so one of the required tools is missing");
            }
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
        tracks,
        used_ffmpeg_fallback,
        dropped_tracks,
        output: output.to_path_buf(),
    })
}

/// Creates a per-process scratch directory for intermediate files.
fn make_workdir() -> Result<PathBuf> {
    let base = std::env::temp_dir().join(format!("nvhvc-{}", std::process::id()));
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_from_path_maps_known_extensions() {
        assert_eq!(
            Container::from_path(Path::new("/x/out.mkv")),
            Some(Container::Mkv)
        );
        assert_eq!(
            Container::from_path(Path::new(r"C:\x\OUT.MP4")),
            Some(Container::Mp4)
        );
        assert_eq!(Container::from_path(Path::new("/x/out.avi")), None);
        assert_eq!(Container::from_path(Path::new("/x/out")), None);
    }

    #[test]
    fn container_setting_roundtrip() {
        assert_eq!(Container::from_setting(Some("mp4")), Container::Mp4);
        assert_eq!(Container::from_setting(Some("mkv")), Container::Mkv);
        assert_eq!(Container::from_setting(Some("bogus")), Container::Mkv);
        assert_eq!(Container::from_setting(None), Container::Mkv);
        for c in Container::ALL {
            assert_eq!(Container::from_setting(Some(c.as_setting())), c);
        }
    }
}
