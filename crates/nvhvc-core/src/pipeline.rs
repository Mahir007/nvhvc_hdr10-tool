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
    /// Number of HDR10 SEI NALs inserted — one per IRAP (random-access) access unit that
    /// didn't already carry a mastering-display SEI. Zero means the encode already signalled
    /// HDR10 at every random-access point.
    pub sei_insertions: usize,
    /// Number of video frames whose timestamps were captured and reproduced (when known).
    pub frames: Option<usize>,
    /// The encode's track inventory that the remux carried over (when known).
    pub tracks: Option<crate::timing::MkvTracks>,
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

    // 4. Capture timing and remux into the chosen container. Only mkvmerge can re-apply the
    //    captured per-frame timing, so MKVToolNix is a hard requirement (front-ends surface
    //    missing tools before the run via [`required_tools`]).
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
    let mkvmerge = tools.require(Tool::Mkvmerge)?;
    progress(Stage::CapturingTiming);
    let mkv_tracks = timing::identify_mkv(mkvmerge, encoded)?;
    if matroska_input {
        let mkvextract = tools.require(Tool::Mkvextract)?;
        timing::extract_timestamps_mkv(mkvextract, encoded, mkv_tracks.video_track_id, &timecodes)?;
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
    let (tracks, dropped_tracks) = match container {
        Container::Mkv => {
            remux::remux_mkv(
                mkvmerge,
                &patched,
                encoded,
                &timecodes,
                &mkv_tracks.video,
                &metadata,
                output,
            )?;
            (Some(mkv_tracks), Vec::new())
        }
        Container::Mp4 => {
            // mkvmerge is the only muxer that can re-apply the captured per-frame timing,
            // so the video is retimed into a Matroska intermediate first; MP4Box then
            // assembles the MP4 from it plus the encode's MP4-compatible tracks. The
            // intermediate's own start offset is re-applied from the timecode file (MP4Box
            // rebases every import to zero).
            let mp4box = tools.require(Tool::Mp4box)?;
            let retimed = work.join("retimed.mkv");
            remux::remux_mkv_video_only(mkvmerge, &patched, &timecodes, &retimed)?;
            let mut plan = crate::probe::mp4_stream_plan(ffprobe, encoded)?;
            let spans = crate::probe::stream_spans(ffprobe, encoded)?;
            let (first_ms, end_ms) = timing::v2_span(&timecodes)?;
            let items = build_mp4_items(
                ffmpeg,
                encoded,
                &work,
                remux::Mp4Source::File(retimed),
                Some((first_ms, end_ms - first_ms)),
                &mut plan,
                &spans,
            )?;
            let chapters = prepare_mp4_chapters(
                tools,
                mp4box,
                encoded,
                &work,
                matroska_input,
                &mkv_tracks,
                &mut plan,
            );
            // Container-level HDR10 boxes (mdcv/clli) for players that read them instead of
            // the in-band SEI (Apple/QuickTime). Written from the injected metadata during the
            // assembly write; when there's no mastering display there's nothing to stamp.
            let hdr_xml = match &metadata.mastering_display {
                Some(md) => {
                    let xml = work.join("hdr.xml");
                    remux::write_hdr_xml(md, metadata.content_light_level.as_ref(), &xml)?;
                    Some(xml)
                }
                None => None,
            };
            // MP4Box seeks and rewrites heavily while assembling, and the cosmetic passes
            // below rewrite the whole file again — on a slow output filesystem (network
            // shares, VM mounts) that reads as a many-minute stall. Assemble next to the
            // other intermediates and move the finished file into place in one pass.
            let assembled = work.join("assembled.mp4");
            remux::remux_mp4box(
                mp4box,
                &items,
                chapters.as_deref(),
                hdr_xml.as_deref(),
                &assembled,
            )?;
            // Cosmetic passes never fail the run: cover art, then real (udta) track titles —
            // the import-time `:name=` only sets the invisible handler name. The finished
            // file's track IDs are read back first (GPAC's assignment isn't the -add order).
            apply_mp4_cover(ffmpeg, mp4box, encoded, &work, &plan, &assembled);
            if let Ok(ids) = crate::probe::mp4_track_ids(ffprobe, &assembled) {
                let _ = remux::mp4box_set_track_names(mp4box, &assembled, &items, &ids);
            }
            move_into_place(&assembled, output)?;
            let carried = plan.tracks_summary(mkv_tracks.video_track_id);
            (Some(carried), plan.dropped)
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
        sei_insertions: patch_report.sei_insertions,
        frames,
        tracks,
        dropped_tracks,
        output: output.to_path_buf(),
    })
}

/// Moves a finished intermediate into its final location: a cheap rename on the same
/// filesystem, a streaming copy when the workdir and the output live on different mounts
/// (or the target exists on Windows, where rename won't replace it).
fn move_into_place(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// Creates a per-process scratch directory for intermediate files.
fn make_workdir() -> Result<PathBuf> {
    let base = std::env::temp_dir().join(format!("nvhvc-{}", std::process::id()));
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

/// The external tools a run with these inputs would need — for front-ends to check (and
/// surface) *before* the user reaches the patch button, instead of failing mid-run.
pub fn required_tools(encoded: &Path, container: Container) -> Vec<Tool> {
    let mut required = vec![Tool::Ffprobe, Tool::Ffmpeg, Tool::Mkvmerge];
    if timing::is_matroska(encoded) {
        required.push(Tool::Mkvextract);
    }
    if container == Container::Mp4 {
        required.push(Tool::Mp4box);
    }
    required
}

/// Produces the OGM chapter file for the MP4 assembly, when the encode has chapters. An
/// explicit file gets written in both flavours (chpl + QT chapter track) and supersedes the
/// stray chapter track GPAC's Matroska demux would otherwise emit alongside direct audio
/// imports — deterministic either way. Best-effort: a failed extraction becomes a dropped
/// note.
fn prepare_mp4_chapters(
    tools: &Toolset,
    mp4box: &Path,
    encoded: &Path,
    work: &Path,
    matroska_input: bool,
    mkv_tracks: &crate::timing::MkvTracks,
    plan: &mut crate::probe::Mp4Plan,
) -> Option<PathBuf> {
    let chap_file = work.join("chapters.txt");
    if matroska_input {
        if mkv_tracks.chapters == 0 {
            return None;
        }
        match tools
            .require(Tool::Mkvextract)
            .and_then(|mkvextract| extract::extract_chapters_ogm(mkvextract, encoded, &chap_file))
        {
            Ok(()) => Some(chap_file),
            Err(_) => {
                plan.dropped.push("chapters".to_owned());
                None
            }
        }
    } else {
        // MP4/MOV inputs never carry chapters through track imports; dump whatever exists.
        let dumped = remux::mp4box_dump_chapters(mp4box, encoded, &chap_file);
        if !dumped && plan.chapter_candidates > 0 {
            plan.dropped.push("chapters".to_owned());
        }
        dumped.then_some(chap_file)
    }
}

/// A track's `(start_ms, media_duration_ms)` edit from its packet span, when it starts late
/// enough to matter (MP4Box rebases imports to zero; sub-ms starts are treated as zero).
fn span_edit(spans: &[crate::probe::StreamSpan], index: u64) -> Option<(f64, f64)> {
    let span = spans.iter().find(|s| s.index == index)?;
    let start = span.start_ms.max(0.0);
    (start >= 1.0).then_some((start, span.end_ms - start))
}

/// Prepares the MP4Box `-add` items: the patched video first, then every carried track from
/// the plan. Direct audio imports come straight from the original (bit-exact); DTS cores and
/// SRT subtitles are extracted into `work` first. A piece that fails to extract degrades to a
/// dropped-track note instead of failing the whole run.
fn build_mp4_items(
    ffmpeg: &Path,
    encoded: &Path,
    work: &Path,
    video_source: remux::Mp4Source,
    video_edit: Option<(f64, f64)>,
    plan: &mut crate::probe::Mp4Plan,
    spans: &[crate::probe::StreamSpan],
) -> Result<Vec<remux::Mp4Add>> {
    use crate::probe::Mp4Import;
    let mut items = vec![remux::Mp4Add {
        source: video_source,
        lang: plan.video_lang.clone(),
        name: plan.video_name.clone(),
        edit: video_edit,
    }];
    // Disjoint borrows: the loop reads `tracks` while extraction failures append to `dropped`.
    let crate::probe::Mp4Plan {
        tracks, dropped, ..
    } = plan;
    for track in tracks.iter() {
        let source = match track.import {
            Mp4Import::AudioDirect => Some(remux::Mp4Source::Track {
                file: encoded.to_path_buf(),
                num: track.index + 1,
            }),
            Mp4Import::AudioDtsCore => {
                let out = work.join(format!("core_{}.dts", track.index));
                match extract::extract_dts_core(ffmpeg, encoded, track.index, &out) {
                    Ok(()) => Some(remux::Mp4Source::File(out)),
                    Err(_) => {
                        dropped.push("1 DTS audio track (core extraction failed)".to_owned());
                        None
                    }
                }
            }
            Mp4Import::TextSub => {
                let out = work.join(format!("sub_{}.srt", track.index));
                match extract::extract_srt(ffmpeg, encoded, track.index, &out) {
                    Ok(()) => Some(remux::Mp4Source::File(out)),
                    Err(_) => {
                        dropped.push("1 subtitle (extraction failed)".to_owned());
                        None
                    }
                }
            }
        };
        if let Some(source) = source {
            // SRT cue times are absolute, so subtitles need no edit; audio gets its original
            // container start offset back.
            let edit = match track.import {
                Mp4Import::TextSub => None,
                _ => span_edit(spans, track.index),
            };
            items.push(remux::Mp4Add {
                source,
                lang: track.lang.clone(),
                name: track.name.clone(),
                edit,
            });
        }
    }
    Ok(items)
}

/// Carries the plan's image attachment into the finished MP4 as cover art. Best-effort: cover
/// art is cosmetic, so extraction or tagging failures never fail the pipeline.
fn apply_mp4_cover(
    ffmpeg: &Path,
    mp4box: &Path,
    encoded: &Path,
    work: &Path,
    plan: &crate::probe::Mp4Plan,
    output: &Path,
) {
    let Some((index, ext)) = plan.cover else {
        return;
    };
    let img = work.join(format!("cover.{ext}"));
    if extract::extract_cover(ffmpeg, encoded, index, &img) {
        let _ = remux::mp4box_set_cover(mp4box, output, &img);
    }
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
