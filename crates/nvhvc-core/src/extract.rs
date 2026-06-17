//! Extracting a raw Annex B HEVC elementary stream from a container, via ffmpeg.

use std::path::Path;

use crate::error::Result;
use crate::tools::{self, Tool, path_arg};

/// Copies the first video track out of `encoded` into a raw Annex B HEVC file at `out_hevc`.
///
/// Uses stream copy (no re-encode) plus the `hevc_mp4toannexb` bitstream filter, which converts
/// the container's length-prefixed NALs (hvcC) into Annex B start-code framing.
pub fn extract_hevc(ffmpeg: &Path, encoded: &Path, out_hevc: &Path) -> Result<()> {
    tools::run(
        ffmpeg,
        Tool::Ffmpeg,
        &[
            "-y",
            "-loglevel",
            "error",
            "-i",
            path_arg(encoded)?,
            "-map",
            "0:v:0",
            "-c:v",
            "copy",
            "-bsf:v",
            "hevc_mp4toannexb",
            "-f",
            "hevc",
            path_arg(out_hevc)?,
        ],
    )
}
