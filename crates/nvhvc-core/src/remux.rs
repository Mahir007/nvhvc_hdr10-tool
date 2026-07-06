//! Remuxing the patched HEVC back into a container while preserving the encode's exact timing and
//! carrying over its non-video tracks (audio, subtitles, chapters, attachments).

use std::path::Path;

use crate::error::Result;
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
            path_arg(temp_mp4)?,
        ],
    )?;
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
