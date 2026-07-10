//! Whole-stream HDR10 patch: edit every SPS's VUI colour description and insert an HDR10 SEI NAL,
//! copying all other bytes through verbatim.
//!
//! The transform touches only non-VCL NAL units, so the coded pictures — their count and order —
//! are untouched. Container timing therefore stays valid; the remux step is responsible for
//! reproducing it exactly (CFR or VFR).

use crate::error::{Error, Result};
use crate::hevc::nal::{NAL_SPS, add_emulation_prevention, find_nals, remove_emulation_prevention};
use crate::hevc::sei::{ContentLightLevel, MasteringDisplay, build_hdr10_sei_rbsp};
use crate::hevc::sps::{VuiColorEdit, edit_color};

/// The full set of HDR10 signalling to inject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hdr10Metadata {
    /// VUI colour description written into every SPS.
    pub color: VuiColorEdit,
    /// Optional mastering display colour volume SEI (payload type 137).
    pub mastering_display: Option<MasteringDisplay>,
    /// Optional content light level SEI (payload type 144).
    pub content_light_level: Option<ContentLightLevel>,
}

/// What a patch changed, for reporting back to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchReport {
    /// Number of SPS NAL units that were rewritten.
    pub sps_patched: usize,
    /// Whether an HDR10 SEI NAL was inserted.
    pub sei_inserted: bool,
}

/// Patches an Annex B HEVC elementary stream, returning the new stream bytes.
///
/// Every SPS gets the HDR10 colour description; a single HDR10 SEI NAL is inserted immediately
/// before the first VCL (slice) NAL — i.e. after the leading VPS/SPS/PPS, where a prefix SEI
/// belongs. Errors if the input has no start code or no SPS.
pub fn patch_stream(input: &[u8], meta: &Hdr10Metadata) -> Result<(Vec<u8>, PatchReport)> {
    let nals = find_nals(input);
    if nals.is_empty() {
        return Err(Error::NoStartCode);
    }
    if !nals.iter().any(|n| n.nal_type == NAL_SPS) {
        return Err(Error::NoSps);
    }

    let sei_wire = build_hdr10_sei_rbsp(
        meta.mastering_display.as_ref(),
        meta.content_light_level.as_ref(),
    )
    .map(|rbsp| add_emulation_prevention(&rbsp));

    let mut out =
        Vec::with_capacity(input.len() + sei_wire.as_ref().map_or(0, |s| s.len() + 4) + 64);
    let mut report = PatchReport {
        sps_patched: 0,
        sei_inserted: false,
    };
    // `sei_pending` stays true only while there is an SEI still to place (false outright if none).
    let mut sei_pending = sei_wire.is_some();
    let mut cursor = 0usize;

    for nal in &nals {
        // Copy anything between the previous NAL and this one (leading bytes / inter-NAL padding).
        out.extend_from_slice(&input[cursor..nal.start_code]);

        // A prefix SEI belongs right before the first slice of the first access unit.
        if sei_pending && nal.is_vcl() {
            let sei = sei_wire.as_ref().expect("sei_pending implies Some");
            out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
            out.extend_from_slice(sei);
            report.sei_inserted = true;
            sei_pending = false;
        }

        // Preserve the original start code exactly.
        out.extend_from_slice(&input[nal.start_code..nal.header]);

        if nal.nal_type == NAL_SPS {
            let rbsp = remove_emulation_prevention(nal.raw(input));
            let new_rbsp = edit_color(&rbsp, &meta.color)?;
            out.extend_from_slice(&add_emulation_prevention(&new_rbsp));
            report.sps_patched += 1;
        } else {
            out.extend_from_slice(&input[nal.header..nal.end]);
        }

        cursor = nal.end;
    }
    out.extend_from_slice(&input[cursor..]);

    // No VCL NAL at all (parameter-set-only stream): place the SEI after everything.
    if sei_pending {
        let sei = sei_wire.as_ref().expect("sei_pending implies Some");
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        out.extend_from_slice(sei);
        report.sei_inserted = true;
    }

    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hevc::nal::{NAL_PREFIX_SEI, NAL_VPS, find_nals};
    use crate::hevc::sps::read_color;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // A real 8-bit SPS (no HDR colour description) from x265.
    const SPS_PLAIN: &str =
        "42010101600000030090000003000003003ca00a080f165959a4932bc05a020000030002000003003010";

    fn build_stream() -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x40, 0x01, 0xAA, 0xBB]); // VPS (type 32)
        s.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // SPS start code
        s.extend_from_slice(&hex(SPS_PLAIN)); // SPS (type 33)
        s.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x44, 0x01, 0xCC]); // PPS (type 34)
        s.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x26, 0x01, 0xDD, 0xEE]); // IDR slice (type 19, VCL)
        s
    }

    fn meta() -> Hdr10Metadata {
        Hdr10Metadata {
            color: VuiColorEdit {
                video_format: 5,
                video_full_range: false,
                colour_primaries: 9,
                transfer_characteristics: 16,
                matrix_coeffs: 9,
                chroma_loc: Some(2),
            },
            mastering_display: Some(MasteringDisplay {
                primaries_x: [13250, 7500, 34000],
                primaries_y: [34500, 3000, 16000],
                white_point_x: 15635,
                white_point_y: 16450,
                max_luminance: 10_000_000,
                min_luminance: 1,
            }),
            content_light_level: Some(ContentLightLevel {
                max_cll: 1000,
                max_fall: 400,
            }),
        }
    }

    #[test]
    fn patches_sps_and_inserts_sei_in_order() {
        let input = build_stream();
        let (out, report) = patch_stream(&input, &meta()).unwrap();
        assert_eq!(report.sps_patched, 1);
        assert!(report.sei_inserted);

        let nals = find_nals(&out);
        let types: Vec<u8> = nals.iter().map(|n| n.nal_type).collect();
        // VPS, SPS, PPS, SEI (inserted before the VCL), IDR.
        assert_eq!(types, vec![NAL_VPS, NAL_SPS, 34, NAL_PREFIX_SEI, 19]);

        // The SPS now carries the injected colour description.
        let sps = nals.iter().find(|n| n.nal_type == NAL_SPS).unwrap();
        let color = read_color(&remove_emulation_prevention(sps.raw(&out)))
            .unwrap()
            .unwrap();
        assert_eq!(color.colour_primaries, 9);
        assert_eq!(color.transfer_characteristics, 16);
        assert_eq!(color.matrix_coeffs, 9);
    }

    #[test]
    fn non_sps_nals_are_byte_identical() {
        let input = build_stream();
        let (out, _) = patch_stream(&input, &meta()).unwrap();
        let out_nals = find_nals(&out);

        // VPS, PPS and the IDR slice must come through untouched.
        for ty in [NAL_VPS, 34u8, 19u8] {
            let a = find_nals(&input)
                .into_iter()
                .find(|n| n.nal_type == ty)
                .map(|n| n.raw(&input).to_vec())
                .unwrap();
            let b = out_nals
                .iter()
                .find(|n| n.nal_type == ty)
                .map(|n| n.raw(&out).to_vec())
                .unwrap();
            assert_eq!(a, b, "NAL type {ty} changed");
        }
    }

    #[test]
    fn errors_without_sps() {
        let mut s = Vec::new();
        s.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x40, 0x01, 0xAA]); // VPS only
        assert!(matches!(patch_stream(&s, &meta()), Err(Error::NoSps)));
    }

    #[test]
    fn sei_omitted_when_no_static_metadata() {
        let input = build_stream();
        let mut m = meta();
        m.mastering_display = None;
        m.content_light_level = None;
        let (out, report) = patch_stream(&input, &m).unwrap();
        assert!(!report.sei_inserted);
        assert!(!find_nals(&out).iter().any(|n| n.nal_type == NAL_PREFIX_SEI));
    }
}
