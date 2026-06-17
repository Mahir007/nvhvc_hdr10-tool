//! Capturing the encoded video track's exact per-frame timestamps.
//!
//! Because extracting to a raw Annex B elementary stream discards container timing, we capture the
//! original timestamps separately and re-apply them at remux. This reproduces the encode's timing
//! exactly — CFR or VFR alike — instead of forcing a constant frame rate.

use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::tools::{self, Tool, path_arg};

/// Identifies the first video track's mkvmerge track ID in a Matroska file.
pub fn video_track_id(mkvmerge: &Path, file: &Path) -> Result<u64> {
    let stdout = tools::run_capture(mkvmerge, Tool::Mkvmerge, &["-J", path_arg(file)?])?;
    let json: Value = serde_json::from_slice(&stdout)?;
    json["tracks"]
        .as_array()
        .and_then(|tracks| tracks.iter().find(|t| t["type"].as_str() == Some("video")))
        .and_then(|t| t["id"].as_u64())
        .ok_or(Error::Probe {
            what: "mkvmerge -J",
            detail: "no video track found".to_owned(),
        })
}

/// Writes the exact per-frame timestamps of `track_id` in `file` to a v2 timecode file at `out`.
///
/// mkvextract emits a **v2** file (timestamps in display/presentation order), which is exactly what
/// `mkvmerge --timestamps` expects; feeding it back reproduces every frame's PTS bit-for-bit
/// (verified against real encodes). Note: re-timing a B-frame HEVC stream this way makes ffmpeg's
/// strict decode path emit a few "non-monotonic DTS" notices — these are a decoder DTS-reconstruction
/// artifact (the muxed container is valid; `ffmpeg -c copy` is clean) and do not affect playback.
pub fn extract_timestamps_mkv(
    mkvextract: &Path,
    file: &Path,
    track_id: u64,
    out: &Path,
) -> Result<()> {
    let spec = format!("{}:{}", track_id, path_arg(out)?);
    tools::run(
        mkvextract,
        Tool::Mkvextract,
        &["timestamps_v2", path_arg(file)?, &spec],
    )
}
