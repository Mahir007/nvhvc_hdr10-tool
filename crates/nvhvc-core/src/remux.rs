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
