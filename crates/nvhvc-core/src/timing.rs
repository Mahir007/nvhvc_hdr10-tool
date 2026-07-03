//! Capturing the encoded video track's exact per-frame timestamps.
//!
//! Because extracting to a raw Annex B elementary stream discards container timing, we capture the
//! original timestamps separately and re-apply them at remux. This reproduces the encode's timing
//! exactly — CFR or VFR alike — instead of forcing a constant frame rate.

use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::tools::{self, Tool, path_arg};

/// What `mkvmerge -J` reports about a Matroska file's tracks: the video track to re-time,
/// plus a summary of everything a remux carries over (for the front-ends' result screens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MkvTracks {
    /// The first video track's mkvmerge track ID.
    pub video_track_id: u64,
    /// Total number of tracks in the container (video + audio + subtitles + other).
    pub total: usize,
    /// Human-readable codec name of each audio track, in order (e.g. `["Opus"]`).
    pub audio_codecs: Vec<String>,
    /// Number of subtitle tracks.
    pub subtitle_count: usize,
}

/// Identifies a Matroska file's tracks via `mkvmerge -J`.
pub fn identify_mkv(mkvmerge: &Path, file: &Path) -> Result<MkvTracks> {
    let stdout = tools::run_capture(mkvmerge, Tool::Mkvmerge, &["-J", path_arg(file)?])?;
    let json: Value = serde_json::from_slice(&stdout)?;
    parse_identify_json(&json)
}

/// Parses an `mkvmerge -J` document (separated out for testing).
fn parse_identify_json(json: &Value) -> Result<MkvTracks> {
    let tracks = json["tracks"].as_array().ok_or(Error::Probe {
        what: "mkvmerge -J",
        detail: "no tracks array".to_owned(),
    })?;
    let video_track_id = tracks
        .iter()
        .find(|t| t["type"].as_str() == Some("video"))
        .and_then(|t| t["id"].as_u64())
        .ok_or(Error::Probe {
            what: "mkvmerge -J",
            detail: "no video track found".to_owned(),
        })?;
    let audio_codecs = tracks
        .iter()
        .filter(|t| t["type"].as_str() == Some("audio"))
        .map(|t| t["codec"].as_str().unwrap_or("audio").to_owned())
        .collect();
    let subtitle_count = tracks
        .iter()
        .filter(|t| t["type"].as_str() == Some("subtitles"))
        .count();
    Ok(MkvTracks {
        video_track_id,
        total: tracks.len(),
        audio_codecs,
        subtitle_count,
    })
}

/// Identifies the first video track's mkvmerge track ID in a Matroska file.
pub fn video_track_id(mkvmerge: &Path, file: &Path) -> Result<u64> {
    Ok(identify_mkv(mkvmerge, file)?.video_track_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_identify_json() {
        let json: Value = serde_json::from_str(
            r#"{"tracks":[
                {"id":0,"type":"video","codec":"HEVC/H.265/MPEG-H"},
                {"id":1,"type":"audio","codec":"Opus"},
                {"id":2,"type":"subtitles","codec":"HDMV PGS"},
                {"id":3,"type":"subtitles","codec":"SubRip/SRT"}
            ]}"#,
        )
        .unwrap();
        let info = parse_identify_json(&json).unwrap();
        assert_eq!(info.video_track_id, 0);
        assert_eq!(info.total, 4);
        assert_eq!(info.audio_codecs, vec!["Opus".to_owned()]);
        assert_eq!(info.subtitle_count, 2);
    }

    #[test]
    fn errors_without_video_track() {
        let json: Value =
            serde_json::from_str(r#"{"tracks":[{"id":0,"type":"audio","codec":"AAC"}]}"#).unwrap();
        assert!(parse_identify_json(&json).is_err());
    }
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
