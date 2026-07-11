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

/// Identifies a file's tracks via `mkvmerge -J` (mkvmerge reads MP4/MOV and more, not just
/// Matroska, so this works for any encode the remux can carry tracks from).
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

/// Whether `file` is (by extension) a Matroska container — i.e. whether mkvextract can read it.
pub fn is_matroska(file: &Path) -> bool {
    matches!(
        file.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("mkv" | "mka" | "mks" | "mk3d" | "webm")
    )
}

/// Writes the exact per-frame timestamps of `file`'s first video stream to a v2 timecode file,
/// synthesized from `ffprobe` packet timestamps — the capture path for encodes that aren't
/// Matroska (mkvextract only reads Matroska).
///
/// Packets arrive in decode order while a v2 file wants presentation order, so the PTS values
/// are sorted. Millisecond values keep their fractional (sub-ms) precision, though mkvmerge
/// quantizes to its 1 ms timestamp scale at mux — the same granularity every Matroska encode
/// has natively. A trailing end marker mirrors mkvextract's output so the final frame's
/// duration survives the remux (and frame counting stays uniform across both capture paths).
pub fn extract_timestamps_ffprobe(ffprobe: &Path, file: &Path, out: &Path) -> Result<()> {
    let stdout = tools::run_capture(
        ffprobe,
        Tool::Ffprobe,
        &[
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-select_streams",
            "v:0",
            "-show_entries",
            "packet=pts_time",
            path_arg(file)?,
        ],
    )?;
    let json: Value = serde_json::from_slice(&stdout)?;
    std::fs::write(out, timestamps_v2_from_packets(&json)?)?;
    Ok(())
}

/// Builds the v2 timecode file body from an ffprobe `-show_entries packet=pts_time` document
/// (separated out for testing).
fn timestamps_v2_from_packets(json: &Value) -> Result<String> {
    let packets = json["packets"].as_array().ok_or(Error::Probe {
        what: "ffprobe packets",
        detail: "no packets array".to_owned(),
    })?;
    let mut pts_ms: Vec<f64> = packets
        .iter()
        .filter_map(|p| p["pts_time"].as_str()?.parse::<f64>().ok())
        .map(|s| s * 1000.0)
        .collect();
    if pts_ms.is_empty() {
        return Err(Error::Probe {
            what: "ffprobe packets",
            detail: "no video packet timestamps found".to_owned(),
        });
    }
    if pts_ms.len() != packets.len() {
        // A frame without a PTS can't be re-timed; bail rather than silently desync.
        return Err(Error::Probe {
            what: "ffprobe packets",
            detail: format!(
                "{} of {} video packets have no pts_time",
                packets.len() - pts_ms.len(),
                packets.len()
            ),
        });
    }
    pts_ms.sort_by(f64::total_cmp);
    // End marker: the final frame's end timestamp, estimated from the last frame interval
    // (a lone frame gets a nominal 40 ms).
    let end = match pts_ms.len() {
        1 => pts_ms[0] + 40.0,
        n => pts_ms[n - 1] + (pts_ms[n - 1] - pts_ms[n - 2]),
    };
    let mut body = String::from("# timestamp format v2\n");
    for t in pts_ms.iter().chain(std::iter::once(&end)) {
        // Sub-ms precision, trimmed so plain-ms inputs stay plain ("1084" not "1084.000000").
        let s = format!("{t:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        body.push_str(s);
        body.push('\n');
    }
    Ok(body)
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

    #[test]
    fn matroska_by_extension() {
        assert!(is_matroska(Path::new("/x/encode.mkv")));
        assert!(is_matroska(Path::new(r"C:\x\ENCODE.MKV")));
        assert!(is_matroska(Path::new("/x/a.webm")));
        assert!(!is_matroska(Path::new("/x/encode.mp4")));
        assert!(!is_matroska(Path::new("/x/encode")));
    }

    #[test]
    fn synthesizes_v2_from_packets() {
        // Decode order with a B-frame reorder (85 before 43); ms-quantized like a real NVENC MKV.
        let json: Value = serde_json::from_str(
            r#"{"packets":[
                {"pts_time":"1.084000"},
                {"pts_time":"1.167000"},
                {"pts_time":"1.126000"}
            ]}"#,
        )
        .unwrap();
        let body = timestamps_v2_from_packets(&json).unwrap();
        // Sorted to presentation order, plus an end marker one frame interval after the last.
        assert_eq!(body, "# timestamp format v2\n1084\n1126\n1167\n1208\n");
    }

    #[test]
    fn v2_keeps_sub_ms_precision() {
        // 24000/1001 fps MP4 timestamps aren't ms-aligned; the fractional part must survive.
        let json: Value = serde_json::from_str(
            r#"{"packets":[{"pts_time":"0.000000"},{"pts_time":"0.041708"}]}"#,
        )
        .unwrap();
        let body = timestamps_v2_from_packets(&json).unwrap();
        assert_eq!(body, "# timestamp format v2\n0\n41.708\n83.416\n");
    }

    #[test]
    fn v2_rejects_missing_pts() {
        let json: Value = serde_json::from_str(r#"{"packets":[{"pts_time":"0.0"},{}]}"#).unwrap();
        assert!(timestamps_v2_from_packets(&json).is_err());
        let empty: Value = serde_json::from_str(r#"{"packets":[]}"#).unwrap();
        assert!(timestamps_v2_from_packets(&empty).is_err());
    }
}
