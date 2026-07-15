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

/// How a carried track gets into the MP4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mp4Import {
    /// MP4Box imports the track straight from the original container (bit-exact copy).
    /// Only used for codecs with no raw bitstream form — GPAC's `#track` selection during a
    /// multi-input assembly re-demuxes the whole container per track, which measured 8×
    /// slower per track than importing an extracted raw file.
    AudioDirect,
    /// Extracted to a raw bitstream file first (`ffmpeg -c copy -f <muxer>`), then imported.
    /// A lossless stream copy: self-framing codecs (AC-3/E-AC-3/MP2/MP3) byte for byte, AAC
    /// via ADTS framing that GPAC strips again on import.
    AudioRaw {
        /// The ffmpeg muxer that writes the raw form.
        muxer: &'static str,
        /// File extension GPAC recognises the raw form by.
        ext: &'static str,
    },
    /// The DTS core substream is extracted losslessly first (`dca_core` bitstream filter) —
    /// carries plain DTS as-is and the core of DTS-HD MA/HRA (the only part MP4 can hold).
    AudioDtsCore,
    /// Text subtitle, extracted to SRT (ASS/SSA lose styling) and imported as native tx3g.
    TextSub,
}

/// One original track the MP4 remux carries over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mp4Track {
    /// Absolute ffprobe stream index in the original file.
    pub index: u64,
    /// Friendly codec name for the result summary (e.g. `"Opus"`, `"DTS"`).
    pub codec: String,
    /// How the track is carried.
    pub import: Mp4Import,
    /// ISO 639 language, when the original tagged one (and it isn't `und`).
    pub lang: Option<String>,
    /// Track title/name, when the original had one.
    pub name: Option<String>,
}

/// Which of an encode's streams MP4 can carry, and what gets left behind.
///
/// MP4 holds far fewer track types than Matroska. Best effort, mirroring the "carbon copy"
/// the Matroska remux does: compatible audio is copied bit-exact, DTS-HD carries its lossless
/// core, text subtitles become native tx3g, an image attachment becomes cover art — and
/// whatever has no MP4 mapping (PGS/VobSub, TrueHD, fonts) is listed in `dropped` so the
/// front-ends can say so.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mp4Plan {
    /// Stream index of the video track being replaced (for its start-offset lookup).
    pub video_index: Option<u64>,
    /// The original video track's language, if tagged (re-applied to the patched video).
    pub video_lang: Option<String>,
    /// The original video track's title, if any.
    pub video_name: Option<String>,
    /// Every carried track, in original order.
    pub tracks: Vec<Mp4Track>,
    /// Stream index + file extension of an image attachment to carry as MP4 cover art.
    pub cover: Option<(u64, &'static str)>,
    /// Number of chapter-style text data streams seen (MP4/MOV inputs expose their chapter
    /// track this way). Not listed in `dropped` — chapters are carried via a chapter dump.
    pub chapter_candidates: usize,
    /// Human-readable notes on what the MP4 doesn't carry, e.g. `["2 PGS subtitles"]`.
    pub dropped: Vec<String>,
}

impl Mp4Plan {
    /// Summary of what the MP4 remux carries over, shaped like the Matroska track report so
    /// front-end result screens work unchanged.
    pub fn tracks_summary(&self, video_track_id: u64) -> crate::timing::MkvTracks {
        crate::timing::MkvTracks {
            video_track_id,
            total: 1 + self.tracks.len(),
            audio_codecs: self
                .tracks
                .iter()
                .filter(|t| t.import != Mp4Import::TextSub)
                .map(|t| t.codec.clone())
                .collect(),
            subtitle_count: self
                .tracks
                .iter()
                .filter(|t| t.import == Mp4Import::TextSub)
                .count(),
            chapters: 0,
            video: Default::default(),
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
            "stream=index,codec_type,codec_name,profile:stream_tags=language,title,mimetype:stream_disposition=attached_pic",
            crate::tools::path_arg(file)?,
        ],
    )?;
    parse_mp4_plan(&serde_json::from_slice(&stdout)?)
}

/// Audio codecs MP4Box imports straight from the original container, with display names.
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
        _ => None,
    }
}

/// The ffmpeg muxer + file extension for a lossless raw extraction of an MP4-compatible
/// audio codec, when one exists. Codecs without a raw form (Opus/FLAC/ALAC) stay on the
/// direct container import.
fn raw_audio_format(codec: &str) -> Option<(&'static str, &'static str)> {
    match codec {
        "aac" => Some(("adts", "aac")),
        "ac3" => Some(("ac3", "ac3")),
        "eac3" => Some(("eac3", "eac3")),
        "mp2" => Some(("mp2", "mp2")),
        "mp3" => Some(("mp3", "mp3")),
        _ => None,
    }
}

/// Text subtitle codecs that can be extracted to SRT and imported as tx3g.
fn mp4_text_subtitle(codec: &str) -> bool {
    matches!(
        codec,
        "subrip" | "srt" | "ass" | "ssa" | "mov_text" | "text" | "webvtt"
    )
}

/// Image codecs an attachment/attached-pic stream may carry (→ MP4 cover art).
fn image_codec(codec: &str, mimetype: Option<&str>) -> bool {
    matches!(codec, "mjpeg" | "jpeg" | "png" | "bmp" | "gif" | "webp")
        || mimetype.is_some_and(|m| m.starts_with("image/"))
}

/// A singular description for a stream MP4 can't hold ("PGS subtitle", "TrueHD audio track").
fn dropped_description(codec_type: &str, codec: &str) -> String {
    match (codec_type, codec) {
        ("subtitle", "hdmv_pgs_subtitle") => "PGS subtitle".to_owned(),
        ("subtitle", "dvd_subtitle") => "VobSub subtitle".to_owned(),
        ("subtitle", "dvb_subtitle") => "DVB subtitle".to_owned(),
        ("subtitle", c) => format!("{c} subtitle"),
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
    let has_ac3_companion = streams.iter().any(|s| {
        s["codec_type"].as_str() == Some("audio")
            && matches!(s["codec_name"].as_str(), Some("ac3") | Some("eac3"))
    });
    let mut plan = Mp4Plan::default();
    let mut dropped: Vec<String> = Vec::new(); // singular descriptions, one per stream
    let mut video_seen = false;
    for s in streams {
        let index = s["index"].as_u64().unwrap_or(u64::MAX);
        let codec_type = s["codec_type"].as_str().unwrap_or("");
        let codec = s["codec_name"].as_str().unwrap_or("");
        let profile = s["profile"].as_str().unwrap_or("");
        let lang = s["tags"]["language"]
            .as_str()
            .filter(|l| !l.is_empty() && *l != "und")
            .map(str::to_owned);
        let name = s["tags"]["title"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(str::to_owned);
        let mimetype = s["tags"]["mimetype"].as_str();
        let attached_pic = s["disposition"]["attached_pic"].as_u64() == Some(1);
        let track = |import, codec: &str| Mp4Track {
            index,
            codec: codec.to_owned(),
            import,
            lang: lang.clone(),
            name: name.clone(),
        };
        match codec_type {
            // The first real video stream is replaced by the patched one; extras have no slot.
            "video" if !video_seen && !attached_pic => {
                video_seen = true;
                plan.video_index = Some(index);
                plan.video_lang = lang.clone();
                plan.video_name = name.clone();
            }
            // Cover art: the first image attachment (or attached picture) is carried.
            "video" | "attachment" if image_codec(codec, mimetype) || attached_pic => {
                if plan.cover.is_none() {
                    let ext = if codec == "png" || mimetype == Some("image/png") {
                        "png"
                    } else {
                        "jpg"
                    };
                    plan.cover = Some((index, ext));
                } else {
                    dropped.push("extra image attachment".to_owned());
                }
            }
            "audio" if codec == "dts" => {
                // Every DTS goes through lossless core extraction: identity for plain DTS,
                // and the only MP4-compatible representation for DTS-HD MA/HRA.
                plan.tracks.push(track(Mp4Import::AudioDtsCore, "DTS"));
                if profile.contains("HD") || profile.contains("MA") || profile.contains("X") {
                    dropped.push(format!("{profile} extension (its DTS core is carried)"));
                }
            }
            "audio" if codec == "truehd" => {
                dropped.push(if has_ac3_companion {
                    "TrueHD audio track (its AC-3 companion is carried)".to_owned()
                } else {
                    "TrueHD audio track".to_owned()
                });
            }
            "audio" => match mp4_audio_name(codec) {
                Some(display) => {
                    let import = match raw_audio_format(codec) {
                        Some((muxer, ext)) => Mp4Import::AudioRaw { muxer, ext },
                        None => Mp4Import::AudioDirect,
                    };
                    plan.tracks.push(track(import, display));
                }
                None => dropped.push(dropped_description(codec_type, codec)),
            },
            "subtitle" if mp4_text_subtitle(codec) => {
                plan.tracks.push(track(Mp4Import::TextSub, "tx3g"));
            }
            // MP4/MOV inputs expose their chapter track as a text-ish data stream; chapters
            // are carried via a chapter dump instead of a track copy, so don't report it.
            "data" if matches!(codec, "bin_data" | "mov_text" | "text") => {
                plan.chapter_candidates += 1;
            }
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

/// The finished MP4's real track IDs for its video/audio/subtitle streams, in stream order —
/// aligned with the assembly's `-add` order. Chapter/data tracks are skipped. Needed because
/// GPAC's track-ID assignment is not the `-add` order.
pub fn mp4_track_ids(ffprobe: &Path, file: &Path) -> Result<Vec<u64>> {
    let stdout = tools::run_capture(
        ffprobe,
        Tool::Ffprobe,
        &[
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_entries",
            "stream=index,id,codec_type",
            crate::tools::path_arg(file)?,
        ],
    )?;
    let json: Value = serde_json::from_slice(&stdout)?;
    let streams = json["streams"].as_array().ok_or(Error::Probe {
        what: "ffprobe streams",
        detail: "no streams array".to_owned(),
    })?;
    Ok(streams
        .iter()
        .filter(|s| {
            matches!(
                s["codec_type"].as_str(),
                Some("video") | Some("audio") | Some("subtitle")
            )
        })
        .filter_map(|s| {
            let id = s["id"].as_str()?;
            u64::from_str_radix(id.trim_start_matches("0x"), 16).ok()
        })
        .collect())
}

/// The first/last presentation extent of every stream in `file`, in milliseconds.
///
/// One full packet pass; used to re-apply each track's original start offset in the MP4
/// (MP4Box rebases every imported track to zero) and to size its edit list.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamSpan {
    /// Absolute ffprobe stream index.
    pub index: u64,
    /// Earliest packet PTS, ms.
    pub start_ms: f64,
    /// Latest packet end (PTS + duration), ms.
    pub end_ms: f64,
}

/// Scans `file` once and returns each stream's [`StreamSpan`].
pub fn stream_spans(ffprobe: &Path, file: &Path) -> Result<Vec<StreamSpan>> {
    let stdout = tools::run_capture(
        ffprobe,
        Tool::Ffprobe,
        &[
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_entries",
            "packet=stream_index,pts_time,duration_time",
            crate::tools::path_arg(file)?,
        ],
    )?;
    parse_stream_spans(&serde_json::from_slice(&stdout)?)
}

/// Parses the packet document into per-stream spans (separated out for testing).
fn parse_stream_spans(json: &Value) -> Result<Vec<StreamSpan>> {
    let packets = json["packets"].as_array().ok_or(Error::Probe {
        what: "ffprobe packets",
        detail: "no packets array".to_owned(),
    })?;
    let mut spans: Vec<StreamSpan> = Vec::new();
    for p in packets {
        let Some(index) = p["stream_index"].as_u64() else {
            continue;
        };
        let Some(pts) = p["pts_time"].as_str().and_then(|s| s.parse::<f64>().ok()) else {
            continue;
        };
        let dur = p["duration_time"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let (start, end) = (pts * 1000.0, (pts + dur) * 1000.0);
        match spans.iter_mut().find(|s| s.index == index) {
            Some(s) => {
                s.start_ms = s.start_ms.min(start);
                s.end_ms = s.end_ms.max(end);
            }
            None => spans.push(StreamSpan {
                index,
                start_ms: start,
                end_ms: end,
            }),
        }
    }
    Ok(spans)
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
                {"index":0,"codec_type":"video","codec_name":"hevc",
                 "tags":{"language":"eng"}},
                {"index":1,"codec_type":"audio","codec_name":"opus",
                 "tags":{"language":"eng","title":"Original 6-Track Mix"}},
                {"index":2,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"}},
                {"index":3,"codec_type":"subtitle","codec_name":"ass"},
                {"index":4,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"},
                {"index":5,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"},
                {"index":6,"codec_type":"attachment","codec_name":"ttf"}
            ]}"#,
        )
        .unwrap();
        let plan = parse_mp4_plan(&json).unwrap();
        assert_eq!(plan.video_lang.as_deref(), Some("eng"));
        assert_eq!(plan.tracks.len(), 3); // Opus + srt + ass
        assert_eq!(plan.tracks[0].import, Mp4Import::AudioDirect);
        assert_eq!(plan.tracks[0].codec, "Opus");
        assert_eq!(plan.tracks[0].name.as_deref(), Some("Original 6-Track Mix"));
        assert_eq!(plan.tracks[1].import, Mp4Import::TextSub);
        assert_eq!(plan.tracks[2].import, Mp4Import::TextSub);
        assert_eq!(plan.tracks[2].lang, None);
        assert_eq!(
            plan.dropped,
            vec!["2 PGS subtitles".to_owned(), "1 attachment".to_owned()]
        );
        let tracks = plan.tracks_summary(0);
        assert_eq!(tracks.total, 4); // patched video + Opus + 2 text subs
        assert_eq!(tracks.audio_codecs, vec!["Opus".to_owned()]);
        assert_eq!(tracks.subtitle_count, 2);
    }

    #[test]
    fn mp4_plan_dts_and_truehd() {
        let json: Value = serde_json::from_str(
            r#"{"streams":[
                {"index":0,"codec_type":"video","codec_name":"hevc"},
                {"index":1,"codec_type":"audio","codec_name":"dts","profile":"DTS-HD MA"},
                {"index":2,"codec_type":"audio","codec_name":"dts","profile":"DTS"},
                {"index":3,"codec_type":"audio","codec_name":"truehd"},
                {"index":4,"codec_type":"audio","codec_name":"ac3"}
            ]}"#,
        )
        .unwrap();
        let plan = parse_mp4_plan(&json).unwrap();
        // Both DTS flavours go through core extraction; plain DTS passes through unchanged.
        assert_eq!(plan.tracks.len(), 3);
        assert_eq!(plan.tracks[0].import, Mp4Import::AudioDtsCore);
        assert_eq!(plan.tracks[1].import, Mp4Import::AudioDtsCore);
        assert_eq!(plan.tracks[2].codec, "AC-3");
        // Self-framing codecs are extracted raw for the import (see Mp4Import::AudioRaw).
        assert_eq!(
            plan.tracks[2].import,
            Mp4Import::AudioRaw {
                muxer: "ac3",
                ext: "ac3"
            }
        );
        // The MA extension is noted; TrueHD points at its carried AC-3 companion.
        assert_eq!(
            plan.dropped,
            vec![
                "1 DTS-HD MA extension (its DTS core is carried)".to_owned(),
                "1 TrueHD audio track (its AC-3 companion is carried)".to_owned(),
            ]
        );
    }

    #[test]
    fn mp4_plan_cover_and_extra_video() {
        let json: Value = serde_json::from_str(
            r#"{"streams":[
                {"index":0,"codec_type":"video","codec_name":"hevc"},
                {"index":1,"codec_type":"attachment","codec_name":"mjpeg","tags":{"mimetype":"image/jpeg"}},
                {"index":2,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}},
                {"index":3,"codec_type":"video","codec_name":"h264"}
            ]}"#,
        )
        .unwrap();
        let plan = parse_mp4_plan(&json).unwrap();
        assert_eq!(plan.cover, Some((1, "jpg"))); // first image attachment wins
        assert_eq!(
            plan.dropped,
            vec![
                "1 extra image attachment".to_owned(),
                "1 extra video track".to_owned()
            ]
        );
    }

    #[test]
    fn stream_spans_fold_per_stream() {
        let json: Value = serde_json::from_str(
            r#"{"packets":[
                {"stream_index":0,"pts_time":"1.084000","duration_time":"0.042000"},
                {"stream_index":1,"pts_time":"0.500000","duration_time":"0.020000"},
                {"stream_index":0,"pts_time":"1.126000","duration_time":"0.042000"},
                {"stream_index":1,"pts_time":"0.520000","duration_time":"0.020000"}
            ]}"#,
        )
        .unwrap();
        let spans = parse_stream_spans(&json).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(
            (spans[0].index, spans[0].start_ms, spans[0].end_ms),
            (0, 1084.0, 1168.0)
        );
        assert_eq!(
            (spans[1].index, spans[1].start_ms, spans[1].end_ms),
            (1, 500.0, 540.0)
        );
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
