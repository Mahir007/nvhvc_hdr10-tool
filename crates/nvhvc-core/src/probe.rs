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
}

/// Probes `source` for HDR10 metadata using the given `ffprobe` executable.
pub fn probe_hdr10(ffprobe: &Path, source: &Path) -> Result<ProbedHdr10> {
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
    let json: Value = serde_json::from_slice(&stdout)?;
    parse_probe_json(&json)
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
}
