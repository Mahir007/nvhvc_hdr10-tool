//! Probing a source file for its HDR10 metadata via `ffprobe`.
//!
//! Reads the video stream's colour description and the first frame's HDR10 SEI side-data
//! (mastering display + content light level), mapping ffmpeg's names to HEVC VUI/SEI integer codes.

use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::hevc::patch::Hdr10Metadata;
use crate::hevc::sei::{ContentLightLevel, MasteringDisplay};
use crate::hevc::sps::VuiColorEdit;
use crate::tools::{self, Tool};

/// HDR10 metadata probed from a source, ready to inject into an encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbedHdr10 {
    /// VUI colour description (primaries / transfer / matrix / range). `chroma_loc` is left `None`
    /// so the encode's own chroma siting is preserved unless the caller overrides it.
    pub color: VuiColorEdit,
    /// Mastering display colour volume, if the source carried it.
    pub mastering_display: Option<MasteringDisplay>,
    /// Content light level, if the source carried it (and it was non-zero).
    pub content_light_level: Option<ContentLightLevel>,
    /// The source's chroma sample location (informational / for an explicit override).
    pub source_chroma_loc: Option<u32>,
}

impl ProbedHdr10 {
    /// Bundles the probed values into the [`Hdr10Metadata`] consumed by the patcher.
    pub fn to_metadata(&self) -> Hdr10Metadata {
        Hdr10Metadata {
            color: self.color,
            mastering_display: self.mastering_display,
            content_light_level: self.content_light_level,
        }
    }

    /// Whether the stream carries *full* HDR10 signalling — i.e. a mastering-display SEI.
    /// (PQ transfer alone doesn't count: an NVENC encode typically signals PQ in its VUI
    /// while missing the SEI, and adding that SEI is this tool's whole job.)
    pub fn has_hdr10(&self) -> bool {
        self.mastering_display.is_some()
    }
}

/// Basic video-stream facts for display in a front-end (file cards and summaries).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoInfo {
    /// Coded width in pixels.
    pub width: u32,
    /// Coded height in pixels.
    pub height: u32,
    /// Codec name as reported by ffprobe (e.g. `"hevc"`).
    pub codec: String,
}

/// Probes `source` for HDR10 metadata using the given `ffprobe` executable.
pub fn probe_hdr10(ffprobe: &Path, source: &Path) -> Result<ProbedHdr10> {
    parse_probe_json(&fetch_probe_json(ffprobe, source)?)
}

/// Probes `source` for HDR10 metadata *and* basic stream facts in one ffprobe call.
pub fn probe_media(ffprobe: &Path, source: &Path) -> Result<(ProbedHdr10, VideoInfo)> {
    let json = fetch_probe_json(ffprobe, source)?;
    Ok((parse_probe_json(&json)?, parse_video_info(&json)?))
}

/// Whether the ffmpeg remux fallback can reproduce this encode's timing bit-exactly.
///
/// A raw-stream ffmpeg remux stamps frames in decode order, so it only reproduces the original
/// timing when there's **no B-frame reordering** (`has_b_frames == 0`) **and a constant frame
/// rate** (`r_frame_rate == avg_frame_rate`). Returns the frame rate to replay at when it's safe,
/// or `None` when only MKVToolNix will do (B-frames or VFR — ffmpeg would scramble the order).
pub fn ffmpeg_cfr_remuxable(ffprobe: &Path, encoded: &Path) -> Result<Option<String>> {
    let f = encoded.to_str().ok_or_else(|| Error::Probe {
        what: "encode path",
        detail: "path is not valid UTF-8".to_owned(),
    })?;
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
            "stream=has_b_frames,r_frame_rate,avg_frame_rate",
            f,
        ],
    )?;
    let json: Value = serde_json::from_slice(&stdout)?;
    let stream = &json["streams"][0];
    // Unknown reorder depth → assume B-frames (don't risk a scrambled remux).
    let has_b_frames = stream["has_b_frames"].as_u64().unwrap_or(1);
    let r_rate = stream["r_frame_rate"].as_str().unwrap_or("");
    let avg_rate = stream["avg_frame_rate"].as_str().unwrap_or("");
    let safe = has_b_frames == 0 && !r_rate.is_empty() && r_rate != "0/0" && r_rate == avg_rate;
    Ok(safe.then(|| r_rate.to_owned()))
}

/// Which of an encode's streams MP4 can carry, and what gets left behind.
///
/// MP4 holds far fewer track types than Matroska: image subtitles (PGS/VobSub), TrueHD/PCM
/// audio, and font attachments have no MP4 mapping. The plan lists the streams to carry
/// (audio stream-copied, text subtitles converted to `mov_text`) and describes the rest so
/// front-ends can tell the user what was left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mp4Plan {
    /// Absolute ffmpeg stream indices of audio tracks to stream-copy.
    pub audio: Vec<u64>,
    /// Friendly codec names of the kept audio tracks, in order (e.g. `["Opus"]`).
    pub audio_codecs: Vec<String>,
    /// Absolute ffmpeg stream indices of text subtitle tracks (converted to `mov_text`).
    pub subtitles: Vec<u64>,
    /// Human-readable descriptions of tracks MP4 can't hold, e.g. `["2 PGS subtitles"]`.
    pub dropped: Vec<String>,
}

impl Mp4Plan {
    /// Summary of what the MP4 remux carries over, shaped like the Matroska track report so
    /// front-end result screens work unchanged.
    pub fn tracks(&self, video_track_id: u64) -> crate::timing::MkvTracks {
        crate::timing::MkvTracks {
            video_track_id,
            total: 1 + self.audio.len() + self.subtitles.len(),
            audio_codecs: self.audio_codecs.clone(),
            subtitle_count: self.subtitles.len(),
        }
    }
}

/// Classifies `file`'s streams for an MP4 remux (see [`Mp4Plan`]).
pub fn mp4_stream_plan(ffprobe: &Path, file: &Path) -> Result<Mp4Plan> {
    let stdout = tools::run_capture(
        ffprobe,
        Tool::Ffprobe,
        &[
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_entries",
            "stream=index,codec_type,codec_name",
            crate::tools::path_arg(file)?,
        ],
    )?;
    parse_mp4_plan(&serde_json::from_slice(&stdout)?)
}

/// Audio codecs ffmpeg can stream-copy into MP4, with their friendly display names.
fn mp4_audio_name(codec: &str) -> Option<&'static str> {
    match codec {
        "aac" => Some("AAC"),
        "ac3" => Some("AC-3"),
        "eac3" => Some("E-AC-3"),
        "mp2" => Some("MP2"),
        "mp3" => Some("MP3"),
        "opus" => Some("Opus"),
        "flac" => Some("FLAC"),
        "alac" => Some("ALAC"),
        "dts" => Some("DTS"),
        _ => None,
    }
}

/// Text subtitle codecs ffmpeg can convert to `mov_text` for MP4.
fn mp4_text_subtitle(codec: &str) -> bool {
    matches!(
        codec,
        "subrip" | "srt" | "ass" | "ssa" | "mov_text" | "text" | "webvtt"
    )
}

/// A singular description for a stream MP4 can't hold ("PGS subtitle", "TrueHD audio track").
fn dropped_description(codec_type: &str, codec: &str) -> String {
    match (codec_type, codec) {
        ("subtitle", "hdmv_pgs_subtitle") => "PGS subtitle".to_owned(),
        ("subtitle", "dvd_subtitle") => "VobSub subtitle".to_owned(),
        ("subtitle", "dvb_subtitle") => "DVB subtitle".to_owned(),
        ("subtitle", c) => format!("{c} subtitle"),
        ("audio", "truehd") => "TrueHD audio track".to_owned(),
        ("audio", c) if c.starts_with("pcm_") => "PCM audio track".to_owned(),
        ("audio", c) => format!("{} audio track", mkvish_name(c)),
        ("attachment", _) => "attachment".to_owned(),
        ("video", _) => "extra video track".to_owned(),
        (_, c) => format!("{c} data track"),
    }
}

/// Capitalized-ish codec name for descriptions ("vorbis" → "Vorbis").
fn mkvish_name(codec: &str) -> String {
    let mut chars = codec.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => codec.to_owned(),
    }
}

/// Parses an ffprobe all-streams document into an [`Mp4Plan`] (separated out for testing).
fn parse_mp4_plan(json: &Value) -> Result<Mp4Plan> {
    let streams = json["streams"].as_array().ok_or(Error::Probe {
        what: "ffprobe streams",
        detail: "no streams array".to_owned(),
    })?;
    let mut plan = Mp4Plan::default();
    let mut dropped: Vec<String> = Vec::new(); // singular descriptions, one per stream
    let mut video_seen = false;
    for s in streams {
        let index = s["index"].as_u64().unwrap_or(u64::MAX);
        let codec_type = s["codec_type"].as_str().unwrap_or("");
        let codec = s["codec_name"].as_str().unwrap_or("");
        match codec_type {
            // The first video stream is replaced by the patched one; extras have no slot.
            "video" if !video_seen => video_seen = true,
            "audio" => match mp4_audio_name(codec) {
                Some(name) => {
                    plan.audio.push(index);
                    plan.audio_codecs.push(name.to_owned());
                }
                None => dropped.push(dropped_description(codec_type, codec)),
            },
            "subtitle" if mp4_text_subtitle(codec) => plan.subtitles.push(index),
            _ => dropped.push(dropped_description(codec_type, codec)),
        }
    }
    // Collapse repeats: ["PGS subtitle", "PGS subtitle"] → "2 PGS subtitles".
    let mut counted: Vec<(String, usize)> = Vec::new();
    for d in dropped {
        match counted.iter_mut().find(|(desc, _)| *desc == d) {
            Some((_, n)) => *n += 1,
            None => counted.push((d, 1)),
        }
    }
    plan.dropped = counted
        .into_iter()
        .map(|(desc, n)| {
            if n > 1 {
                format!("{n} {desc}s")
            } else {
                format!("1 {desc}")
            }
        })
        .collect();
    Ok(plan)
}

/// Runs ffprobe on `source` and returns the parsed JSON document.
fn fetch_probe_json(ffprobe: &Path, source: &Path) -> Result<Value> {
    let src = source.to_str().ok_or_else(|| Error::Probe {
        what: "source path",
        detail: "path is not valid UTF-8".to_owned(),
    })?;
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
            "-show_streams",
            "-read_intervals",
            "%+#1",
            "-show_frames",
            src,
        ],
    )?;
    Ok(serde_json::from_slice(&stdout)?)
}

/// Parses an `ffprobe -print_format json` document (separated out for testing).
fn parse_probe_json(json: &Value) -> Result<ProbedHdr10> {
    let stream = json["streams"].get(0).ok_or_else(|| Error::Probe {
        what: "ffprobe JSON",
        detail: "no video stream found".to_owned(),
    })?;

    let color = VuiColorEdit {
        video_format: 5, // unspecified
        video_full_range: matches!(
            stream["color_range"].as_str(),
            Some("pc") | Some("full") | Some("jpeg")
        ),
        colour_primaries: map_primaries(stream["color_primaries"].as_str()),
        transfer_characteristics: map_transfer(stream["color_transfer"].as_str()),
        matrix_coeffs: map_matrix(stream["color_space"].as_str()),
        chroma_loc: None,
    };
    let source_chroma_loc = map_chroma(stream["chroma_location"].as_str());

    let mut mastering_display = None;
    let mut content_light_level = None;
    if let Some(side_data) = json["frames"]
        .get(0)
        .and_then(|f| f["side_data_list"].as_array())
    {
        for sd in side_data {
            match sd["side_data_type"].as_str() {
                Some("Mastering display metadata") => mastering_display = parse_mastering(sd),
                Some("Content light level metadata") => content_light_level = parse_cll(sd),
                _ => {}
            }
        }
    }

    Ok(ProbedHdr10 {
        color,
        mastering_display,
        content_light_level,
        source_chroma_loc,
    })
}

/// Parses the video stream's basic facts out of the same ffprobe JSON document.
fn parse_video_info(json: &Value) -> Result<VideoInfo> {
    let stream = json["streams"].get(0).ok_or_else(|| Error::Probe {
        what: "ffprobe JSON",
        detail: "no video stream found".to_owned(),
    })?;
    Ok(VideoInfo {
        width: stream["width"].as_u64().unwrap_or(0) as u32,
        height: stream["height"].as_u64().unwrap_or(0) as u32,
        codec: stream["codec_name"].as_str().unwrap_or("").to_owned(),
    })
}

/// Numerator of an `ffprobe` fraction string like `"34000/50000"` (the raw SEI integer value).
fn frac_num(sd: &Value, key: &str) -> Option<u64> {
    let s = sd[key].as_str()?;
    s.split('/').next()?.trim().parse::<u64>().ok()
}

fn parse_mastering(sd: &Value) -> Option<MasteringDisplay> {
    Some(MasteringDisplay {
        // SEI primary order is green, blue, red.
        primaries_x: [
            frac_num(sd, "green_x")? as u16,
            frac_num(sd, "blue_x")? as u16,
            frac_num(sd, "red_x")? as u16,
        ],
        primaries_y: [
            frac_num(sd, "green_y")? as u16,
            frac_num(sd, "blue_y")? as u16,
            frac_num(sd, "red_y")? as u16,
        ],
        white_point_x: frac_num(sd, "white_point_x")? as u16,
        white_point_y: frac_num(sd, "white_point_y")? as u16,
        max_luminance: frac_num(sd, "max_luminance")? as u32,
        min_luminance: frac_num(sd, "min_luminance")? as u32,
    })
}

fn parse_cll(sd: &Value) -> Option<ContentLightLevel> {
    let field = |key: &str| -> Option<u64> {
        sd[key]
            .as_u64()
            .or_else(|| sd[key].as_str().and_then(|s| s.trim().parse().ok()))
    };
    let max_content = field("max_content")?;
    let max_average = field("max_average")?;
    // 0/0 means "unknown" — treat as absent rather than writing a meaningless SEI.
    if max_content == 0 && max_average == 0 {
        return None;
    }
    Some(ContentLightLevel {
        max_cll: max_content as u16,
        max_fall: max_average as u16,
    })
}

/// Falls back to parsing a bare integer, else `unspecified` (2).
fn numeric_or(name: Option<&str>, unspecified: u8) -> u8 {
    name.and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(unspecified)
}

fn map_primaries(name: Option<&str>) -> u8 {
    match name {
        Some("bt709") => 1,
        Some("bt470m") => 4,
        Some("bt470bg") => 5,
        Some("smpte170m") => 6,
        Some("smpte240m") => 7,
        Some("film") => 8,
        Some("bt2020") => 9,
        Some("smpte428") | Some("smpte428_1") => 10,
        Some("smpte431") => 11,
        Some("smpte432") => 12,
        Some("unknown") | Some("unspecified") | None => 2,
        other => numeric_or(other, 2),
    }
}

fn map_transfer(name: Option<&str>) -> u8 {
    match name {
        Some("bt709") => 1,
        Some("gamma22") | Some("bt470m") => 4,
        Some("gamma28") | Some("bt470bg") => 5,
        Some("smpte170m") => 6,
        Some("smpte240m") => 7,
        Some("linear") => 8,
        Some("log100") => 9,
        Some("log316") => 10,
        Some("iec61966-2-4") => 11,
        Some("bt1361e") => 12,
        Some("iec61966-2-1") => 13,
        Some("bt2020-10") => 14,
        Some("bt2020-12") => 15,
        Some("smpte2084") => 16,
        Some("smpte428") => 17,
        Some("arib-std-b67") => 18,
        Some("unknown") | Some("unspecified") | None => 2,
        other => numeric_or(other, 2),
    }
}

fn map_matrix(name: Option<&str>) -> u8 {
    match name {
        Some("gbr") | Some("rgb") => 0,
        Some("bt709") => 1,
        Some("fcc") => 4,
        Some("bt470bg") => 5,
        Some("smpte170m") => 6,
        Some("smpte240m") => 7,
        Some("ycgco") => 8,
        Some("bt2020nc") => 9,
        Some("bt2020c") => 10,
        Some("smpte2085") => 11,
        Some("chroma-derived-nc") => 12,
        Some("chroma-derived-c") => 13,
        Some("ictcp") => 14,
        Some("unknown") | Some("unspecified") | None => 2,
        other => numeric_or(other, 2),
    }
}

fn map_chroma(name: Option<&str>) -> Option<u32> {
    match name {
        Some("left") => Some(0),
        Some("center") => Some(1),
        Some("topleft") => Some(2),
        Some("top") => Some(3),
        Some("bottomleft") => Some(4),
        Some("bottom") => Some(5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped exactly like the real `source.mkv` ffprobe output.
    const SAMPLE: &str = r#"{
        "streams": [{
            "codec_name": "hevc",
            "width": 3840,
            "height": 2160,
            "color_range": "tv",
            "color_space": "bt2020nc",
            "color_transfer": "smpte2084",
            "color_primaries": "bt2020",
            "chroma_location": "topleft"
        }],
        "frames": [{
            "side_data_list": [
                {
                    "side_data_type": "Mastering display metadata",
                    "red_x": "34000/50000", "red_y": "16000/50000",
                    "green_x": "13250/50000", "green_y": "34500/50000",
                    "blue_x": "7500/50000", "blue_y": "3000/50000",
                    "white_point_x": "15635/50000", "white_point_y": "16450/50000",
                    "min_luminance": "50/10000", "max_luminance": "40000000/10000"
                },
                { "side_data_type": "Content light level metadata", "max_content": 0, "max_average": 0 }
            ]
        }]
    }"#;

    #[test]
    fn parses_real_source_shape() {
        let json: Value = serde_json::from_str(SAMPLE).unwrap();
        let probed = parse_probe_json(&json).unwrap();

        assert_eq!(probed.color.colour_primaries, 9); // bt2020
        assert_eq!(probed.color.transfer_characteristics, 16); // smpte2084
        assert_eq!(probed.color.matrix_coeffs, 9); // bt2020nc
        assert!(!probed.color.video_full_range); // tv
        assert_eq!(probed.source_chroma_loc, Some(2)); // topleft

        let md = probed.mastering_display.unwrap();
        assert_eq!(md.primaries_x, [13250, 7500, 34000]); // G, B, R
        assert_eq!(md.primaries_y, [34500, 3000, 16000]);
        assert_eq!(md.white_point_x, 15635);
        assert_eq!(md.max_luminance, 40_000_000); // 4000 nits
        assert_eq!(md.min_luminance, 50);

        // MaxCLL/FALL of 0/0 is "unknown" -> no content-light SEI.
        assert!(probed.content_light_level.is_none());
    }

    #[test]
    fn cll_kept_when_nonzero() {
        let json: Value = serde_json::from_str(
            r#"{"streams":[{}],"frames":[{"side_data_list":[
                {"side_data_type":"Content light level metadata","max_content":1000,"max_average":400}]}]}"#,
        )
        .unwrap();
        let probed = parse_probe_json(&json).unwrap();
        let cll = probed.content_light_level.unwrap();
        assert_eq!(cll.max_cll, 1000);
        assert_eq!(cll.max_fall, 400);
    }

    #[test]
    fn unknown_color_defaults_to_unspecified() {
        let json: Value = serde_json::from_str(r#"{"streams":[{}],"frames":[]}"#).unwrap();
        let probed = parse_probe_json(&json).unwrap();
        assert_eq!(probed.color.colour_primaries, 2);
        assert_eq!(probed.color.transfer_characteristics, 2);
        assert_eq!(probed.color.matrix_coeffs, 2);
        assert!(probed.mastering_display.is_none());
    }

    #[test]
    fn parses_video_info() {
        let json: Value = serde_json::from_str(SAMPLE).unwrap();
        let info = parse_video_info(&json).unwrap();
        assert_eq!(info.width, 3840);
        assert_eq!(info.height, 2160);
        assert_eq!(info.codec, "hevc");
    }

    #[test]
    fn mp4_plan_classifies_streams() {
        // Mirrors the user's real encode shape: HEVC + Opus + text and image subs + a font.
        let json: Value = serde_json::from_str(
            r#"{"streams":[
                {"index":0,"codec_type":"video","codec_name":"hevc"},
                {"index":1,"codec_type":"audio","codec_name":"opus"},
                {"index":2,"codec_type":"subtitle","codec_name":"subrip"},
                {"index":3,"codec_type":"subtitle","codec_name":"ass"},
                {"index":4,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"},
                {"index":5,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"},
                {"index":6,"codec_type":"audio","codec_name":"truehd"},
                {"index":7,"codec_type":"attachment","codec_name":"ttf"}
            ]}"#,
        )
        .unwrap();
        let plan = parse_mp4_plan(&json).unwrap();
        assert_eq!(plan.audio, vec![1]);
        assert_eq!(plan.audio_codecs, vec!["Opus".to_owned()]);
        assert_eq!(plan.subtitles, vec![2, 3]);
        assert_eq!(
            plan.dropped,
            vec![
                "2 PGS subtitles".to_owned(),
                "1 TrueHD audio track".to_owned(),
                "1 attachment".to_owned(),
            ]
        );
        let tracks = plan.tracks(0);
        assert_eq!(tracks.total, 4); // patched video + Opus + 2 text subs
        assert_eq!(tracks.subtitle_count, 2);
    }

    #[test]
    fn mp4_plan_keeps_first_video_only() {
        let json: Value = serde_json::from_str(
            r#"{"streams":[
                {"index":0,"codec_type":"video","codec_name":"hevc"},
                {"index":1,"codec_type":"video","codec_name":"mjpeg"}
            ]}"#,
        )
        .unwrap();
        let plan = parse_mp4_plan(&json).unwrap();
        assert!(plan.audio.is_empty());
        assert_eq!(plan.dropped, vec!["1 extra video track".to_owned()]);
    }

    #[test]
    fn video_info_tolerates_missing_fields() {
        let json: Value = serde_json::from_str(r#"{"streams":[{}],"frames":[]}"#).unwrap();
        let info = parse_video_info(&json).unwrap();
        assert_eq!(info.width, 0);
        assert_eq!(info.height, 0);
        assert_eq!(info.codec, "");
    }
}
