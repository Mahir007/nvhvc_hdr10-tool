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
use crate::hevc::patch::Hdr10Metadata;
use crate::hevc::sei::{ContentLightLevel, MasteringDisplay};
use crate::timing::VideoProps;
use crate::tools::{self, Tool, path_arg};

/// Muxes the patched HEVC into a Matroska file using mkvmerge.
///
/// The patched video gets its timing from `timestamps` (a v2 timecode file captured from the
/// original) and the original video track's container properties from `video` — reproducing
/// CFR/VFR exactly and keeping the output a carbon copy of the encode with the video swapped.
/// The injected `hdr` metadata is also written into the Matroska `Colour` (+ `MasteringMetadata`)
/// track element, so players that read HDR10 from the container rather than parsing the in-band
/// stream still see it. Every non-video track from `original` (audio, subtitles, chapters,
/// attachments) is carried over, so the result is complete and in sync.
pub fn remux_mkv(
    mkvmerge: &Path,
    patched_hevc: &Path,
    original: &Path,
    timestamps: &Path,
    video: &VideoProps,
    hdr: &Hdr10Metadata,
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
    args.extend(mkv_colour_args(hdr));
    args.extend(mkv_colour_args(hdr));
    args.push(path_arg(patched_hevc)?.to_owned());
    // Everything except video from the original encode.
    args.push("--no-video".to_owned());
    args.push(path_arg(original)?.to_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tools::run(mkvmerge, Tool::Mkvmerge, &refs)
}

/// mkvmerge track-0 options that write the HDR10 signalling into the Matroska `Colour` element
/// (plus its `MasteringMetadata` sub-element when a mastering display is present), so container-
/// reading players trigger HDR10 without parsing the in-band stream. Primaries/transfer/matrix
/// use the same H.273 codes as the VUI; chromaticity and white point are CIE 1931 coordinates
/// (SEI value × 0.00002) and luminance is cd/m² (SEI value × 0.0001), the units mkvmerge wants.
fn mkv_colour_args(hdr: &Hdr10Metadata) -> Vec<String> {
    let c = &hdr.color;
    let mut a = vec![
        "--colour-matrix-coefficients".to_owned(),
        format!("0:{}", c.matrix_coeffs),
        "--colour-primaries".to_owned(),
        format!("0:{}", c.colour_primaries),
        "--colour-transfer-characteristics".to_owned(),
        format!("0:{}", c.transfer_characteristics),
        // mkvmerge colour range: 1 = broadcast/limited, 2 = full.
        "--colour-range".to_owned(),
        format!("0:{}", if c.video_full_range { 2 } else { 1 }),
    ];
    if let Some(md) = &hdr.mastering_display {
        // The SEI primary order is green(0), blue(1), red(2); mkvmerge wants red, green, blue.
        let xy = |v: u16| (f64::from(v) / 50_000.0).to_string();
        let lum = |v: u32| (f64::from(v) / 10_000.0).to_string();
        a.push("--chromaticity-coordinates".to_owned());
        a.push(format!(
            "0:{},{},{},{},{},{}",
            xy(md.primaries_x[2]),
            xy(md.primaries_y[2]),
            xy(md.primaries_x[0]),
            xy(md.primaries_y[0]),
            xy(md.primaries_x[1]),
            xy(md.primaries_y[1]),
        ));
        a.push("--white-colour-coordinates".to_owned());
        a.push(format!(
            "0:{},{}",
            xy(md.white_point_x),
            xy(md.white_point_y)
        ));
        a.push("--max-luminance".to_owned());
        a.push(format!("0:{}", lum(md.max_luminance)));
        a.push("--min-luminance".to_owned());
        a.push(format!("0:{}", lum(md.min_luminance)));
    }
    if let Some(cll) = &hdr.content_light_level {
        a.push("--max-content-light".to_owned());
        a.push(format!("0:{}", cll.max_cll));
        a.push("--max-frame-light".to_owned());
        a.push(format!("0:{}", cll.max_fall));
    }
    a
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
/// original's chapters (when the caller extracted them to an OGM file) and, optionally,
/// container-level HDR10 boxes (when the caller wrote a GPAC HDR descriptor via
/// [`write_hdr_xml`]).
///
/// The movie timescale is forced to 1000 so the hand-written edit lists are millisecond-exact.
/// MP4Box writes `hvc1` sample entries by default (what Apple players require), keeps the
/// in-band HDR10 SEI, and converts SRT to native tx3g. Chapters are written in both flavours —
/// `chpl` (`-chap`, what ffmpeg/VLC read) and a QT chapter track (`-chapqt`, what Apple
/// players read); an explicit chapter file also supersedes the stray chapter track GPAC's
/// Matroska demux would otherwise emit alongside direct audio imports. `-hdr` stamps the
/// `mdcv`/`clli` sample-entry boxes during this write (the option is a no-op unless the file
/// is being rewritten, which `-new` always does).
pub fn remux_mp4box(
    mp4box: &Path,
    items: &[Mp4Add],
    chapters: Option<&Path>,
    hdr_xml: Option<&Path>,
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
    if let Some(xml) = hdr_xml {
        args.push("-hdr".to_owned());
        args.push(path_arg(xml)?.to_owned());
    }
    args.push("-new".to_owned());
    args.push(path_arg(out)?.to_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    tools::run(mp4box, Tool::Mp4box, &refs)
}

/// Writes a GPAC `-hdr` XML descriptor for the HDR10 static metadata to `out`, so MP4Box can
/// stamp container-level `mdcv` (mastering display) and, when present, `clli` (content light
/// level) sample-entry boxes. Apple players and some tools read HDR10 from these boxes rather
/// than the in-band SEI. The integer units match the SEI exactly (chromaticity in 0.00002,
/// luminance in 0.0001 cd/m²), so the values pass straight through.
pub fn write_hdr_xml(
    md: &MasteringDisplay,
    cll: Option<&ContentLightLevel>,
    out: &Path,
) -> Result<()> {
    // GPAC's -hdr parser wants <HDR><Track>… with lowercase <mdcv>/<clli> boxes; the display
    // primaries are in green, blue, red order (index 0..2) — the same order as the SEI.
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<HDR>\n <Track>\n");
    xml.push_str(&format!(
        "  <mdcv display_primaries_0_x=\"{}\" display_primaries_0_y=\"{}\" \
         display_primaries_1_x=\"{}\" display_primaries_1_y=\"{}\" \
         display_primaries_2_x=\"{}\" display_primaries_2_y=\"{}\" \
         white_point_x=\"{}\" white_point_y=\"{}\" \
         max_display_mastering_luminance=\"{}\" min_display_mastering_luminance=\"{}\"/>\n",
        md.primaries_x[0],
        md.primaries_y[0],
        md.primaries_x[1],
        md.primaries_y[1],
        md.primaries_x[2],
        md.primaries_y[2],
        md.white_point_x,
        md.white_point_y,
        md.max_luminance,
        md.min_luminance,
    ));
    if let Some(cll) = cll {
        xml.push_str(&format!(
            "  <clli max_content_light_level=\"{}\" max_pic_average_light_level=\"{}\"/>\n",
            cll.max_cll, cll.max_fall,
        ));
    }
    xml.push_str(" </Track>\n</HDR>\n");
    std::fs::write(out, xml)?;
    Ok(())
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

    #[test]
    fn mkv_colour_args_map_hdr10() {
        use crate::hevc::sps::VuiColorEdit;
        let hdr = Hdr10Metadata {
            color: VuiColorEdit {
                video_format: 5,
                video_full_range: false,
                colour_primaries: 9,
                transfer_characteristics: 16,
                matrix_coeffs: 9,
                chroma_loc: None,
            },
            mastering_display: Some(MasteringDisplay {
                primaries_x: [13250, 7500, 34000], // G, B, R
                primaries_y: [34500, 3000, 16000],
                white_point_x: 15635,
                white_point_y: 16450,
                max_luminance: 40_000_000,
                min_luminance: 50,
            }),
            content_light_level: Some(ContentLightLevel {
                max_cll: 1000,
                max_fall: 400,
            }),
        };
        let a = mkv_colour_args(&hdr);
        let joined = a.join(" ");
        assert!(joined.contains("--colour-primaries 0:9"));
        assert!(joined.contains("--colour-transfer-characteristics 0:16"));
        assert!(joined.contains("--colour-matrix-coefficients 0:9"));
        assert!(joined.contains("--colour-range 0:1")); // limited
        // Chromaticity reordered to red,green,blue and scaled to CIE 1931 units.
        assert!(joined.contains("--chromaticity-coordinates 0:0.68,0.32,0.265,0.69,0.15,0.06"));
        assert!(joined.contains("--white-colour-coordinates 0:0.3127,0.329"));
        assert!(joined.contains("--max-luminance 0:4000")); // 40000000 * 0.0001
        assert!(joined.contains("--min-luminance 0:0.005"));
        assert!(joined.contains("--max-content-light 0:1000"));
        assert!(joined.contains("--max-frame-light 0:400"));
    }

    #[test]
    fn hdr_xml_maps_sei_values() {
        let md = MasteringDisplay {
            primaries_x: [13250, 7500, 34000], // G, B, R
            primaries_y: [34500, 3000, 16000],
            white_point_x: 15635,
            white_point_y: 16450,
            max_luminance: 40_000_000,
            min_luminance: 50,
        };
        let dir = std::env::temp_dir().join(format!("nvhvc-hdr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Mastering display only: GPAC's green/blue/red primary indices map straight through,
        // and no <clli> element is emitted.
        let p = dir.join("md.xml");
        write_hdr_xml(&md, None, &p).unwrap();
        let xml = std::fs::read_to_string(&p).unwrap();
        assert!(xml.contains("<HDR>") && xml.contains("<Track>") && xml.contains("<mdcv "));
        assert!(xml.contains("display_primaries_0_x=\"13250\""));
        assert!(xml.contains("display_primaries_2_y=\"16000\""));
        assert!(xml.contains("white_point_x=\"15635\""));
        assert!(xml.contains("max_display_mastering_luminance=\"40000000\""));
        assert!(xml.contains("min_display_mastering_luminance=\"50\""));
        assert!(!xml.contains("<clli"));

        // With content light level, the <clli> box is added.
        let p2 = dir.join("both.xml");
        write_hdr_xml(
            &md,
            Some(&ContentLightLevel {
                max_cll: 1000,
                max_fall: 400,
            }),
            &p2,
        )
        .unwrap();
        let xml2 = std::fs::read_to_string(&p2).unwrap();
        assert!(xml2.contains("max_content_light_level=\"1000\""));
        assert!(xml2.contains("max_pic_average_light_level=\"400\""));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
