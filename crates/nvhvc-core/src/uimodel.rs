//! Presentation model shared by the GUI front-ends: the editable HDR10 metadata rows,
//! friendly value formatting, and small file/path helpers.
//!
//! This is pure logic (no UI toolkit types) so the WinUI and GTK front-ends can share
//! one implementation of "what the five metadata rows are and how editing them works".

use std::path::Path;

use crate::hevc::patch::Hdr10Metadata;
use crate::hevc::sei::{ContentLightLevel, MasteringDisplay};
use crate::naming;

/// The five editable metadata rows of the source stage.
pub const META_ROWS: [&str; 5] = [
    "Colour signalling",
    "Signal range",
    "Mastering display",
    "Peak luminance",
    "Content light level",
];

/// Combo options: HEVC codes offered per enum field (a practical shortlist).
pub const PRIMARIES_OPTS: [u8; 6] = [1, 6, 9, 11, 12, 2];
pub const TRANSFER_OPTS: [u8; 8] = [1, 6, 13, 14, 15, 16, 18, 2];
pub const MATRIX_OPTS: [u8; 6] = [0, 1, 6, 9, 10, 2];

/// `"9 · BT.2020"`-style option label.
pub fn code_label(code: u8, name: &str) -> String {
    format!("{code} · {name}")
}

/// Flat, editable copy of the effective metadata while a row editor is open.
///
/// Mastering-display and content-light-level values are flattened to zeros when absent;
/// on apply, a CLL of `0/0` maps back to "not signalled" (matching the SEI convention).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Draft {
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
    pub full_range: bool,
    // Mastering display (green, blue, red, white — 0.00002 units).
    pub gx: u16,
    pub gy: u16,
    pub bx: u16,
    pub by: u16,
    pub rx: u16,
    pub ry: u16,
    pub wx: u16,
    pub wy: u16,
    // 0.0001 cd/m² units.
    pub max_lum: u32,
    pub min_lum: u32,
    pub max_cll: u16,
    pub max_fall: u16,
}

impl Draft {
    pub fn from_meta(m: &Hdr10Metadata) -> Self {
        let md = m.mastering_display.unwrap_or(MasteringDisplay {
            primaries_x: [0; 3],
            primaries_y: [0; 3],
            white_point_x: 0,
            white_point_y: 0,
            max_luminance: 0,
            min_luminance: 0,
        });
        let cll = m.content_light_level.unwrap_or(ContentLightLevel {
            max_cll: 0,
            max_fall: 0,
        });
        Draft {
            primaries: m.color.colour_primaries,
            transfer: m.color.transfer_characteristics,
            matrix: m.color.matrix_coeffs,
            full_range: m.color.video_full_range,
            gx: md.primaries_x[0],
            gy: md.primaries_y[0],
            bx: md.primaries_x[1],
            by: md.primaries_y[1],
            rx: md.primaries_x[2],
            ry: md.primaries_y[2],
            wx: md.white_point_x,
            wy: md.white_point_y,
            max_lum: md.max_luminance,
            min_lum: md.min_luminance,
            max_cll: cll.max_cll,
            max_fall: cll.max_fall,
        }
    }

    /// Merges this draft's fields for `row` into `base`, returning the new metadata.
    pub fn apply_row(&self, row: usize, mut base: Hdr10Metadata) -> Hdr10Metadata {
        match row {
            0 => {
                base.color.colour_primaries = self.primaries;
                base.color.transfer_characteristics = self.transfer;
                base.color.matrix_coeffs = self.matrix;
            }
            1 => base.color.video_full_range = self.full_range,
            2 | 3 => {
                let mut md = Draft::from_meta(&base);
                if row == 2 {
                    md.gx = self.gx;
                    md.gy = self.gy;
                    md.bx = self.bx;
                    md.by = self.by;
                    md.rx = self.rx;
                    md.ry = self.ry;
                    md.wx = self.wx;
                    md.wy = self.wy;
                } else {
                    md.max_lum = self.max_lum;
                    md.min_lum = self.min_lum;
                }
                base.mastering_display = Some(MasteringDisplay {
                    primaries_x: [md.gx, md.bx, md.rx],
                    primaries_y: [md.gy, md.by, md.ry],
                    white_point_x: md.wx,
                    white_point_y: md.wy,
                    max_luminance: md.max_lum,
                    min_luminance: md.min_lum,
                });
            }
            _ => {
                base.content_light_level = if self.max_cll == 0 && self.max_fall == 0 {
                    None
                } else {
                    Some(ContentLightLevel {
                        max_cll: self.max_cll,
                        max_fall: self.max_fall,
                    })
                };
            }
        }
        base
    }
}

/// The display value of a metadata row.
pub fn row_value(row: usize, m: &Hdr10Metadata) -> String {
    match row {
        0 => format!(
            "{} · {} · {}",
            naming::primaries_name(m.color.colour_primaries),
            naming::transfer_name(m.color.transfer_characteristics),
            naming::matrix_name(m.color.matrix_coeffs),
        ),
        1 => naming::range_name(m.color.video_full_range).to_owned(),
        2 => match &m.mastering_display {
            Some(md) => format!("{} primaries", naming::gamut_name(md).unwrap_or("Custom")),
            None => "Not signalled".to_owned(),
        },
        3 => match &m.mastering_display {
            Some(md) => format!(
                "{} nits · min {}",
                md.max_luminance as f64 / 10_000.0,
                md.min_luminance as f64 / 10_000.0
            ),
            None => "Not signalled".to_owned(),
        },
        _ => match &m.content_light_level {
            Some(c) => format!("MaxCLL {} · MaxFALL {}", c.max_cll, c.max_fall),
            None => "Not signalled".to_owned(),
        },
    }
}

/// Whether a row's fields differ between the effective and the detected metadata.
pub fn row_overridden(row: usize, effective: &Hdr10Metadata, detected: &Hdr10Metadata) -> bool {
    match row {
        0 => {
            effective.color.colour_primaries != detected.color.colour_primaries
                || effective.color.transfer_characteristics
                    != detected.color.transfer_characteristics
                || effective.color.matrix_coeffs != detected.color.matrix_coeffs
        }
        1 => effective.color.video_full_range != detected.color.video_full_range,
        2 => {
            let f = |m: &Hdr10Metadata| {
                m.mastering_display.map(|md| {
                    (
                        md.primaries_x,
                        md.primaries_y,
                        md.white_point_x,
                        md.white_point_y,
                    )
                })
            };
            f(effective) != f(detected)
        }
        3 => {
            let f = |m: &Hdr10Metadata| {
                m.mastering_display
                    .map(|md| (md.max_luminance, md.min_luminance))
            };
            f(effective) != f(detected)
        }
        _ => effective.content_light_level != detected.content_light_level,
    }
}

/// One-line friendly colour summary, e.g. `"BT.2020 · PQ · Display P3 · 4000 nits"`.
pub fn colour_summary(m: &Hdr10Metadata) -> String {
    let mut parts = vec![
        naming::primaries_name(m.color.colour_primaries).to_owned(),
        short_transfer(m.color.transfer_characteristics).to_owned(),
    ];
    if let Some(md) = &m.mastering_display {
        parts.push(naming::gamut_name(md).unwrap_or("Custom").to_owned());
        parts.push(format!("{} nits", md.max_luminance as f64 / 10_000.0));
    }
    parts.join(" · ")
}

/// Shorter transfer names for one-line summaries.
fn short_transfer(code: u8) -> &'static str {
    match code {
        16 => "PQ",
        18 => "HLG",
        other => naming::transfer_name(other),
    }
}

/// `"11 · Opus + 9 subtitles"`-style summary of the tracks a remux carried over.
pub fn tracks_line(tracks: &crate::timing::MkvTracks) -> String {
    let mut parts: Vec<String> = Vec::new();
    // Collapse repeated audio codecs: ["AAC", "AAC"] → "2× AAC".
    let mut seen: Vec<(String, usize)> = Vec::new();
    for codec in &tracks.audio_codecs {
        match seen.iter_mut().find(|(c, _)| c == codec) {
            Some((_, n)) => *n += 1,
            None => seen.push((codec.clone(), 1)),
        }
    }
    for (codec, n) in seen {
        parts.push(if n > 1 {
            format!("{n}× {codec}")
        } else {
            codec
        });
    }
    if tracks.subtitle_count > 0 {
        parts.push(format!(
            "{} subtitle{}",
            tracks.subtitle_count,
            if tracks.subtitle_count == 1 { "" } else { "s" }
        ));
    }
    if parts.is_empty() {
        format!(
            "{} track{}",
            tracks.total,
            if tracks.total == 1 { "" } else { "s" }
        )
    } else {
        format!("{} · {}", tracks.total, parts.join(" + "))
    }
}

/// `"518 MB"`-style file size.
pub fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

/// File name portion of a path (for card titles).
pub fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

/// Default output path for an encode: `<dir>/<stem> [HDR10].mkv`.
pub fn default_output(encode: &str) -> String {
    let p = Path::new(encode);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_owned());
    let name = format!("{stem} [HDR10].mkv");
    match p.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name).display().to_string(),
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hevc::sps::VuiColorEdit;

    fn meta() -> Hdr10Metadata {
        Hdr10Metadata {
            color: VuiColorEdit {
                video_format: 5,
                video_full_range: false,
                colour_primaries: 9,
                transfer_characteristics: 16,
                matrix_coeffs: 9,
                chroma_loc: None,
            },
            mastering_display: Some(MasteringDisplay {
                primaries_x: [13250, 7500, 34000],
                primaries_y: [34500, 3000, 16000],
                white_point_x: 15635,
                white_point_y: 16450,
                max_luminance: 40_000_000,
                min_luminance: 50,
            }),
            content_light_level: None,
        }
    }

    #[test]
    fn draft_roundtrip_is_identity() {
        let m = meta();
        let d = Draft::from_meta(&m);
        let mut back = m;
        for row in 0..META_ROWS.len() {
            back = d.apply_row(row, back);
        }
        assert_eq!(back, m);
        for row in 0..META_ROWS.len() {
            assert!(!row_overridden(row, &back, &m));
        }
    }

    #[test]
    fn apply_row_scopes_to_one_row() {
        let m = meta();
        let mut d = Draft::from_meta(&m);
        d.primaries = 1;
        d.max_lum = 10_000_000; // would change row 3 if applied
        let out = d.apply_row(0, m);
        assert_eq!(out.color.colour_primaries, 1);
        assert_eq!(out.mastering_display, m.mastering_display);
        assert!(row_overridden(0, &out, &m));
        assert!(!row_overridden(3, &out, &m));
    }

    #[test]
    fn cll_zero_zero_means_unsignalled() {
        let m = meta();
        let d = Draft::from_meta(&m); // cll flattened to 0/0
        let out = d.apply_row(4, m);
        assert!(out.content_light_level.is_none());
        assert_eq!(row_value(4, &out), "Not signalled");
    }

    #[test]
    fn tracks_line_matches_mockup_shape() {
        use crate::timing::MkvTracks;
        let t = MkvTracks {
            video_track_id: 0,
            total: 11,
            audio_codecs: vec!["Opus".to_owned()],
            subtitle_count: 9,
        };
        assert_eq!(tracks_line(&t), "11 · Opus + 9 subtitles");
        let dual = MkvTracks {
            video_track_id: 0,
            total: 4,
            audio_codecs: vec!["AAC".to_owned(), "AAC".to_owned()],
            subtitle_count: 1,
        };
        assert_eq!(tracks_line(&dual), "4 · 2× AAC + 1 subtitle");
        let bare = MkvTracks {
            video_track_id: 0,
            total: 1,
            audio_codecs: vec![],
            subtitle_count: 0,
        };
        assert_eq!(tracks_line(&bare), "1 track");
    }

    #[test]
    fn default_output_derives_from_encode() {
        let out = default_output("/videos/encode.mkv");
        let p = Path::new(&out);
        assert_eq!(
            p.file_name().unwrap().to_str().unwrap(),
            "encode [HDR10].mkv"
        );
        assert_eq!(p.parent().unwrap(), Path::new("/videos"));
    }
}
