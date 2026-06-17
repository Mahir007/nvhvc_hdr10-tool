//! Human-readable names for HEVC colour codes and mastering-display gamuts.
//!
//! These map the integer VUI codes (and the raw mastering-display chromaticity coordinates) to
//! friendly labels for display in the CLI/GUI. They are pure lookups over data ffprobe already
//! provides — no extra tooling (e.g. MediaInfo) is required.

use crate::hevc::sei::MasteringDisplay;

/// Name for a `colour_primaries` code (H.265 Table E.3).
pub fn primaries_name(code: u8) -> &'static str {
    match code {
        1 => "BT.709",
        2 => "Unspecified",
        4 => "BT.470 System M",
        5 => "BT.470 System B/G",
        6 => "SMPTE 170M (BT.601)",
        7 => "SMPTE 240M",
        8 => "Generic film",
        9 => "BT.2020",
        10 => "SMPTE ST 428-1 (XYZ)",
        11 => "SMPTE RP 431-2 (DCI P3)",
        12 => "SMPTE EG 432-1 (Display P3)",
        _ => "Unknown",
    }
}

/// Name for a `transfer_characteristics` code (H.265 Table E.4).
pub fn transfer_name(code: u8) -> &'static str {
    match code {
        1 => "BT.709",
        2 => "Unspecified",
        4 => "Gamma 2.2",
        5 => "Gamma 2.8",
        6 => "SMPTE 170M (BT.601)",
        7 => "SMPTE 240M",
        8 => "Linear",
        9 => "Log 100:1",
        10 => "Log 316:1",
        11 => "IEC 61966-2-4",
        12 => "BT.1361",
        13 => "sRGB / IEC 61966-2-1",
        14 => "BT.2020 (10-bit)",
        15 => "BT.2020 (12-bit)",
        16 => "PQ (SMPTE ST 2084)",
        17 => "SMPTE ST 428-1",
        18 => "HLG (ARIB STD-B67)",
        _ => "Unknown",
    }
}

/// Name for a `matrix_coeffs` code (H.265 Table E.5).
pub fn matrix_name(code: u8) -> &'static str {
    match code {
        0 => "Identity (GBR)",
        1 => "BT.709",
        2 => "Unspecified",
        4 => "FCC",
        5 => "BT.470 System B/G",
        6 => "SMPTE 170M (BT.601)",
        7 => "SMPTE 240M",
        8 => "YCgCo",
        9 => "BT.2020 non-constant",
        10 => "BT.2020 constant",
        11 => "SMPTE ST 2085",
        12 => "Chroma-derived non-constant",
        13 => "Chroma-derived constant",
        14 => "ICtCp",
        _ => "Unknown",
    }
}

/// Name for the `video_full_range_flag`.
pub fn range_name(full_range: bool) -> &'static str {
    if full_range { "Full" } else { "Limited" }
}

/// Name for a `chroma_sample_loc_type`.
pub fn chroma_loc_name(loc: u32) -> &'static str {
    match loc {
        0 => "Type 0 (left)",
        1 => "Type 1 (center)",
        2 => "Type 2 (top-left)",
        3 => "Type 3 (top)",
        4 => "Type 4 (bottom-left)",
        5 => "Type 5 (bottom)",
        _ => "Unknown",
    }
}

/// Identifies the gamut named by a mastering display's primaries (and white point for P3).
///
/// Coordinates are in the SEI's 0.00002 units. Returns `None` for an unrecognised primary set.
pub fn gamut_name(md: &MasteringDisplay) -> Option<&'static str> {
    // Standard mastering-display primaries in SEI units, as [green, blue, red] x/y.
    const TOL: u16 = 50; // ~0.001 chromaticity

    if primaries_match(md, [8500, 39850], [6550, 2300], [35400, 14600], TOL) {
        return Some("BT.2020");
    }
    if primaries_match(md, [15000, 30000], [7500, 3000], [32000, 16500], TOL) {
        return Some("BT.709");
    }
    if primaries_match(md, [13250, 34500], [7500, 3000], [34000, 16000], TOL) {
        // P3 RGB primaries — the white point distinguishes Display P3 (D65) from DCI-P3.
        if close(md.white_point_x, 15700, 80) && close(md.white_point_y, 17550, 80) {
            return Some("DCI-P3");
        }
        return Some("Display P3");
    }
    None
}

fn close(a: u16, b: u16, tol: u16) -> bool {
    a.abs_diff(b) <= tol
}

fn primaries_match(
    md: &MasteringDisplay,
    green: [u16; 2],
    blue: [u16; 2],
    red: [u16; 2],
    tol: u16,
) -> bool {
    let pairs = [
        (md.primaries_x[0], green[0]),
        (md.primaries_y[0], green[1]),
        (md.primaries_x[1], blue[0]),
        (md.primaries_y[1], blue[1]),
        (md.primaries_x[2], red[0]),
        (md.primaries_y[2], red[1]),
    ];
    pairs.iter().all(|&(a, b)| close(a, b, tol))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p3_d65() -> MasteringDisplay {
        // The real source's mastering display.
        MasteringDisplay {
            primaries_x: [13250, 7500, 34000],
            primaries_y: [34500, 3000, 16000],
            white_point_x: 15635,
            white_point_y: 16450,
            max_luminance: 40_000_000,
            min_luminance: 50,
        }
    }

    #[test]
    fn code_names() {
        assert_eq!(primaries_name(9), "BT.2020");
        assert_eq!(transfer_name(16), "PQ (SMPTE ST 2084)");
        assert_eq!(transfer_name(18), "HLG (ARIB STD-B67)");
        assert_eq!(matrix_name(9), "BT.2020 non-constant");
        assert_eq!(range_name(false), "Limited");
        assert_eq!(chroma_loc_name(2), "Type 2 (top-left)");
    }

    #[test]
    fn detects_display_p3() {
        assert_eq!(gamut_name(&p3_d65()), Some("Display P3"));
    }

    #[test]
    fn detects_dci_p3_by_white_point() {
        let mut md = p3_d65();
        md.white_point_x = 15700;
        md.white_point_y = 17550;
        assert_eq!(gamut_name(&md), Some("DCI-P3"));
    }

    #[test]
    fn detects_bt2020() {
        let md = MasteringDisplay {
            primaries_x: [8500, 6550, 35400],
            primaries_y: [39850, 2300, 14600],
            white_point_x: 15635,
            white_point_y: 16450,
            max_luminance: 10_000_000,
            min_luminance: 1,
        };
        assert_eq!(gamut_name(&md), Some("BT.2020"));
    }

    #[test]
    fn unknown_primaries_return_none() {
        let md = MasteringDisplay {
            primaries_x: [1, 2, 3],
            primaries_y: [4, 5, 6],
            white_point_x: 7,
            white_point_y: 8,
            max_luminance: 9,
            min_luminance: 10,
        };
        assert_eq!(gamut_name(&md), None);
    }
}
