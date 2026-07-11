//! Remuxing the patched HEVC back into a container while preserving the encode's exact timing and
//! carrying over its non-video tracks (audio, subtitles, chapters, attachments).

use std::path::Path;

use crate::error::Result;
use crate::probe::Mp4Plan;
use crate::tools::{self, Tool, path_arg};

/// Muxes the patched HEVC into a Matroska file using mkvmerge.
///
/// The patched video gets its timing from `timestamps` (a v2 timecode file captured from the
/// original), reproducing CFR/VFR exactly. Every non-video track from `original` (audio, subtitles,
/// chapters, attachments) is carried over, so the result is complete and in sync.
pub fn remux_mkv(
    mkvmerge: &Path,
    patched_hevc: &Path,
    original: &Path,
    timestamps: &Path,
    out: &Path,
) -> Result<()> {
    let ts_spec = format!("0:{}", path_arg(timestamps)?);
    tools::run(
        mkvmerge,
        Tool::Mkvmerge,
        &[
            "-o",
            path_arg(out)?,
            // Patched video track (track 0 of the raw stream), timed from the captured timecodes.
            "--timestamps",
            &ts_spec,
            path_arg(patched_hevc)?,
            // Everything except video from the original encode.
            "--no-video",
            path_arg(original)?,
        ],
    )
}

/// Muxes only the patched video into a Matroska file, timed from `timestamps` — the
/// intermediate step for MP4 output. mkvmerge is the only muxer that can re-apply a captured
/// per-frame timecode file, so the video is retimed here first; ffmpeg then does the final MP4
/// join (which needs its track-compatibility handling anyway). Matroska's 1 ms timestamps read
/// back exactly as written, so nothing is lost in the round-trip.
pub fn remux_mkv_video_only(
    mkvmerge: &Path,
    patched_hevc: &Path,
    timestamps: &Path,
    out: &Path,
) -> Result<()> {
    let ts_spec = format!("0:{}", path_arg(timestamps)?);
    tools::run(
        mkvmerge,
        Tool::Mkvmerge,
        &[
            "-o",
            path_arg(out)?,
            "--timestamps",
            &ts_spec,
            path_arg(patched_hevc)?,
        ],
    )
}

/// Joins the (already retimed) patched video with the original encode's MP4-compatible tracks
/// into an `.mp4`, per `plan` (see [`crate::probe::mp4_stream_plan`]).
///
/// Video and audio are stream-copied; text subtitles are converted to `mov_text` (the only
/// subtitle format MP4 players broadly support); chapters carry over. `-strict experimental`
/// lets ffmpeg write Opus/FLAC-in-MP4 on versions that still gate them; `-tag:v hvc1` writes
/// the sample-entry code Apple players require (the parameter sets are in the header either
/// way, so nothing else changes).
pub fn remux_mp4(
    ffmpeg: &Path,
    video: &Path,
    original: &Path,
    plan: &Mp4Plan,
    out: &Path,
) -> Result<()> {
    let mut args: Vec<String> = [
        "-y",
        "-loglevel",
        "error",
        "-i",
        path_arg(video)?,
        "-i",
        path_arg(original)?,
        "-map",
        "0:v:0",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    for index in plan.audio.iter().chain(&plan.subtitles) {
        args.push("-map".to_owned());
        args.push(format!("1:{index}"));
    }
    // Without -copyts ffmpeg zeroes each input's own start time, which destroys the
    // inter-stream alignment (a video track starting at 1084 ms would land 1084 ms early
    // against its audio). With it, timestamps pass through untouched; the muxer's default
    // avoid_negative_ts then shifts all streams *uniformly* if anything is negative, so
    // relative sync is always preserved.
    args.extend(["-copyts", "-map_chapters", "1", "-c", "copy"].map(str::to_owned));
    if !plan.subtitles.is_empty() {
        args.extend(["-c:s", "mov_text"].map(str::to_owned));
    }
    args.extend(["-tag:v", "hvc1", "-strict", "experimental"].map(str::to_owned));
    args.push(path_arg(out)?.to_owned());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tools::run(ffmpeg, Tool::Ffmpeg, &arg_refs)
}

/// Wraps the raw patched stream into an MP4 at a constant `fps` — step one of the ffmpeg
/// fallback (only correct for no-B-frame CFR encodes; the caller must gate on
/// [`crate::probe::ffmpeg_cfr_remuxable`]). The MP4 muxer accepts a raw Annex B stream's
/// generated timestamps (matroska's rejects them) and the round-trip keeps the in-band SEI.
pub fn ffmpeg_wrap_cfr(ffmpeg: &Path, patched_hevc: &Path, fps: &str, out: &Path) -> Result<()> {
    tools::run(
        ffmpeg,
        Tool::Ffmpeg,
        &[
            "-y",
            "-loglevel",
            "error",
            "-r",
            fps,
            "-i",
            path_arg(patched_hevc)?,
            "-c",
            "copy",
            path_arg(out)?,
        ],
    )
}

/// Remuxes the patched HEVC into Matroska with ffmpeg, the fallback when MKVToolNix is absent.
///
/// Only correct for encodes ffmpeg can re-time exactly — no B-frames and a constant frame rate
/// (see [`crate::probe::ffmpeg_cfr_remuxable`]); the caller must gate on that. It goes through a
/// temporary MP4 because the matroska muxer rejects a raw Annex B stream's generated timestamps
/// while the MP4 muxer accepts them — and the round-trip keeps the in-band HDR10 SEI intact. The
/// resulting video is then joined with every non-video track and the chapters of `original`.
pub fn remux_ffmpeg(
    ffmpeg: &Path,
    patched_hevc: &Path,
    original: &Path,
    fps: &str,
    temp_mp4: &Path,
    out: &Path,
) -> Result<()> {
    // 1. Raw patched stream → MP4, stamping constant-rate timing at `fps`.
    ffmpeg_wrap_cfr(ffmpeg, patched_hevc, fps, temp_mp4)?;
    // 2. That video + everything-but-video (audio/subs/attachments) + chapters from the original.
    tools::run(
        ffmpeg,
        Tool::Ffmpeg,
        &[
            "-y",
            "-loglevel",
            "error",
            "-i",
            path_arg(temp_mp4)?,
            "-i",
            path_arg(original)?,
            "-map",
            "0:v:0",
            "-map",
            "1",
            "-map",
            "-1:v",
            "-map_chapters",
            "1",
            "-c",
            "copy",
            path_arg(out)?,
        ],
    )
}
