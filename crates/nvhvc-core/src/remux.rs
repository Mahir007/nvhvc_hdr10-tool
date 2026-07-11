//! Remuxing the patched HEVC back into a container while preserving the encode's exact timing and
//! carrying over its non-video tracks (audio, subtitles, chapters, attachments).
//!
//! Matroska output is a single mkvmerge call. MP4 output is an MP4Box assembly: mkvmerge first
//! re-times the patched video into a Matroska intermediate (it is the only muxer that can
//! re-apply a captured per-frame timecode file), then MP4Box imports that video plus the
//! original's MP4-compatible tracks. ffmpeg never muxes MP4 here — it only extracts pieces
//! (SRT subtitles, DTS cores, cover images) for MP4Box to import.

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::timing::VideoProps;
use crate::tools::{self, Tool, path_arg};

/// Muxes the patched HEVC into a Matroska file using mkvmerge.
///
/// The patched video gets its timing from `timestamps` (a v2 timecode file captured from the
/// original) and the original video track's container properties from `video` — reproducing
/// CFR/VFR exactly and keeping the output a carbon copy of the encode with the video swapped.
/// Every non-video track from `original` (audio, subtitles, chapters, attachments) is carried
/// over, so the result is complete and in sync.
pub fn remux_mkv(
    mkvmerge: &Path,
    patched_hevc: &Path,
    original: &Path,
    timestamps: &Path,
    video: &VideoProps,
    out: &Path,
) -> Result<()> {
    let mut args: Vec<String> = vec![
        "-o".to_owned(),
        path_arg(out)?.to_owned(),
        // Patched video track (track 0 of the raw stream), timed from the captured timecodes.
        "--timestamps".to_owned(),
        format!("0:{}", path_arg(timestamps)?),
    ];
    if let Some(lang) = &video.language {
        args.push("--language".to_owned());
        args.push(format!("0:{lang}"));
    }
    if let Some(name) = &video.name {
        args.push("--track-name".to_owned());
        args.push(format!("0:{name}"));
    }
    if let Some(default) = video.default {
        args.push("--default-track-flag".to_owned());
        args.push(format!("0:{}", if default { "yes" } else { "no" }));
    }
    if let Some(forced) = video.forced {
        args.push("--forced-display-flag".to_owned());
        args.push(format!("0:{}", if forced { "yes" } else { "no" }));
    }
    if let Some(dims) = &video.display_dimensions {
        args.push("--display-dimensions".to_owned());
        args.push(format!("0:{dims}"));
    }
    args.push(path_arg(patched_hevc)?.to_owned());
    // Everything except video from the original encode.
    args.push("--no-video".to_owned());
    args.push(path_arg(original)?.to_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tools::run(mkvmerge, Tool::Mkvmerge, &refs)
}

/// Muxes only the patched video into a Matroska file, timed from `timestamps` — the
/// intermediate step for MP4 output. mkvmerge is the only muxer that can re-apply a captured
/// per-frame timecode file, so the video is retimed here first; MP4Box then assembles the
/// final MP4 from it. Matroska's 1 ms timestamps read back exactly as written, so nothing is
/// lost in the round-trip.
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

/// One `-add` item of the MP4Box assembly. The first item is always the patched video.
#[derive(Debug, Clone)]
pub struct Mp4Add {
    pub source: Mp4Source,
    /// ISO 639 language applied via `:lang=`.
    pub lang: Option<String>,
    /// Track handler name applied via `:name=`.
    pub name: Option<String>,
    /// `(start_ms, media_duration_ms)`: re-applies the track's original start offset via an
    /// explicit edit list. MP4Box rebases every imported track to zero, and its own `:delay`
    /// option quantizes through the 600-tick default movie timescale (up to ~1.7 ms off), so
    /// the edit list is written manually at the exact millisecond.
    pub edit: Option<(f64, f64)>,
}

/// Where an [`Mp4Add`] item's media comes from.
#[derive(Debug, Clone)]
pub enum Mp4Source {
    /// A standalone file: the retimed video MKV, an extracted `.srt` or `.dts`.
    File(PathBuf),
    /// Track `num` (1-based; ffprobe stream index + 1) imported straight from a container.
    Track { file: PathBuf, num: u64 },
}

/// Assembles the final `.mp4` with MP4Box from the prepared `items` (video first), plus the
/// original's chapters when the caller extracted them to an OGM file.
///
/// The movie timescale is forced to 1000 so the hand-written edit lists are millisecond-exact.
/// MP4Box writes `hvc1` sample entries by default (what Apple players require), keeps the
/// in-band HDR10 SEI, and converts SRT to native tx3g. Chapters are written in both flavours —
/// `chpl` (`-chap`, what ffmpeg/VLC read) and a QT chapter track (`-chapqt`, what Apple
/// players read); an explicit chapter file also supersedes the stray chapter track GPAC's
/// Matroska demux would otherwise emit alongside direct audio imports.
pub fn remux_mp4box(
    mp4box: &Path,
    items: &[Mp4Add],
    chapters: Option<&Path>,
    out: &Path,
) -> Result<()> {
    let mut args: Vec<String> = vec!["-timescale".to_owned(), "1000".to_owned()];
    let mut edits: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        args.push("-add".to_owned());
        args.push(format_add(item)?);
        // Tracks are numbered 1.. in -add order. Sub-millisecond starts round away — the
        // whole pipeline is ms-quantized by the Matroska intermediate anyway.
        if let Some((start_ms, media_dur_ms)) = item.edit
            && start_ms >= 1.0
            && media_dur_ms > 0.0
        {
            edits.push(format!(
                "{}=re0-{d}e{d}-{m},0",
                i + 1,
                d = fmt_seconds(start_ms),
                m = fmt_seconds(media_dur_ms),
            ));
        }
    }
    for e in &edits {
        args.push("-edits".to_owned());
        args.push(e.clone());
    }
    if let Some(chap) = chapters {
        args.push("-chap".to_owned());
        args.push(path_arg(chap)?.to_owned());
        args.push("-chapqt".to_owned());
        args.push(path_arg(chap)?.to_owned());
    }
    args.push("-new".to_owned());
    args.push(path_arg(out)?.to_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tools::run(mp4box, Tool::Mp4box, &refs)
}

/// Tags `file` with `image` as its cover art (iTunes-style `covr`), in place.
pub fn mp4box_set_cover(mp4box: &Path, file: &Path, image: &Path) -> Result<()> {
    let tag = format!("cover={}", path_arg(image)?);
    tools::run(mp4box, Tool::Mp4box, &["-itags", &tag, path_arg(file)?])
}

/// Writes each named track's title into its `udta` `name` box (what players and ffprobe read
/// as the track name — the import-time `:name=` only sets the invisible handler name), in
/// place. `track_ids` are the finished file's real track IDs aligned with `items` — GPAC's
/// ID assignment is NOT the `-add` order (the first text import can get ID 65535), so the
/// caller reads them back from the output.
pub fn mp4box_set_track_names(
    mp4box: &Path,
    file: &Path,
    items: &[Mp4Add],
    track_ids: &[u64],
) -> Result<()> {
    let mut args: Vec<String> = Vec::new();
    for (item, id) in items.iter().zip(track_ids) {
        if let Some(name) = &item.name {
            args.push("-udta".to_owned());
            args.push(format!("{id}:type=name:str={}", sanitize_opt(name)));
        }
    }
    if args.is_empty() {
        return Ok(());
    }
    args.push(path_arg(file)?.to_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tools::run(mp4box, Tool::Mp4box, &refs)
}

/// Dumps `input`'s chapters to an OGM chapter file for re-import via `-chap`. Best-effort:
/// returns whether a non-empty chapter file appeared (MP4/MOV inputs don't auto-carry
/// chapters the way Matroska imports do).
pub fn mp4box_dump_chapters(mp4box: &Path, input: &Path, out_txt: &Path) -> bool {
    let (Ok(input), Ok(out)) = (path_arg(input), path_arg(out_txt)) else {
        return false;
    };
    let _ = tools::run(
        mp4box,
        Tool::Mp4box,
        &["-dump-chap-ogg", "-out", out, input],
    );
    std::fs::metadata(out_txt)
        .map(|m| m.len() > 0)
        .unwrap_or(false)
}

/// Renders one `-add` argument: `path[#N][:lang=L][:name=S]`.
fn format_add(item: &Mp4Add) -> Result<String> {
    let mut arg = match &item.source {
        Mp4Source::File(p) => path_arg(p)?.to_owned(),
        Mp4Source::Track { file, num } => format!("{}#{num}", path_arg(file)?),
    };
    if let Some(lang) = item
        .lang
        .as_deref()
        .filter(|l| l.chars().all(|c| c.is_ascii_alphabetic()))
    {
        arg.push_str(&format!(":lang={lang}"));
    }
    if let Some(name) = &item.name {
        arg.push_str(&format!(":name={}", sanitize_opt(name)));
    }
    Ok(arg)
}

/// Strips the characters MP4Box's colon-separated option parser would trip on.
fn sanitize_opt(s: &str) -> String {
    s.chars()
        .map(|c| if matches!(c, ':' | '=' | '#') { ' ' } else { c })
        .collect()
}

/// Milliseconds as a trimmed seconds string for `-edits` ("1084 → "1.084", 500 → "0.5").
fn fmt_seconds(ms: f64) -> String {
    let s = format!("{:.3}", ms / 1000.0);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_arg_shapes() {
        let item = Mp4Add {
            source: Mp4Source::Track {
                file: PathBuf::from("C:/x/encode.mkv"),
                num: 2,
            },
            lang: Some("eng".to_owned()),
            name: Some("Original: 6-Track #Mix".to_owned()),
            edit: None,
        };
        assert_eq!(
            format_add(&item).unwrap(),
            "C:/x/encode.mkv#2:lang=eng:name=Original  6-Track  Mix"
        );
        let file = Mp4Add {
            source: Mp4Source::File(PathBuf::from("/w/sub_2.srt")),
            lang: None,
            name: None,
            edit: None,
        };
        assert_eq!(format_add(&file).unwrap(), "/w/sub_2.srt");
    }

    #[test]
    fn seconds_formatting() {
        assert_eq!(fmt_seconds(1084.0), "1.084");
        assert_eq!(fmt_seconds(500.0), "0.5");
        assert_eq!(fmt_seconds(59142.0), "59.142");
        assert_eq!(fmt_seconds(60000.0), "60");
    }
}
