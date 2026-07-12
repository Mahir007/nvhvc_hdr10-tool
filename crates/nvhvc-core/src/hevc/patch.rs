//! Whole-stream HDR10 patch: edit every SPS's VUI colour description and insert an HDR10 SEI NAL
//! before every IRAP (random-access) picture, copying all other bytes through verbatim.
//!
//! The transform touches only non-VCL NAL units, so the coded pictures — their count and order —
//! are untouched. Container timing therefore stays valid; the remux step is responsible for
//! reproducing it exactly (CFR or VFR).
//!
//! **Why per-IRAP:** the mastering-display (137) and content-light-level (144) SEI messages
//! apply to the coded (layer-wise) video sequence that carries them (H.265 Annex D) — they are
//! not repeated implicitly. A decoder that begins decoding at a random-access point (an IRAP)
//! only has the metadata if it is present in the sequence it starts from, so a single SEI at
//! the head of the stream leaves everything reached by a seek unsignalled. Emitting the SEI at
//! every IRAP guarantees HDR10 at every random-access point; mid-stream IRAPs that don't
//! formally begin a new sequence simply get a redundant-but-valid copy. This matches what HDR10
//! encoders (e.g. x265 with `--master-display`) emit.

use crate::error::{Error, Result};
use crate::hevc::nal::{
    NAL_PREFIX_SEI, NAL_SPS, add_emulation_prevention, find_nals, remove_emulation_prevention,
};
use crate::hevc::sei::{
    ContentLightLevel, MasteringDisplay, SEI_CONTENT_LIGHT_LEVEL, SEI_MASTERING_DISPLAY,
    build_hdr10_sei_rbsp, rbsp_has_sei_payload,
};
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
    /// Number of HDR10 SEI NAL units inserted — one per IRAP access unit that didn't already
    /// carry a mastering-display SEI (so the count is the number of random-access points now
    /// signalling HDR10).
    pub sei_insertions: usize,
}

impl PatchReport {
    /// Whether any HDR10 SEI was inserted.
    pub fn sei_inserted(&self) -> bool {
        self.sei_insertions > 0
    }
}

/// Patches an Annex B HEVC elementary stream, returning the new stream bytes.
///
/// Every SPS gets the HDR10 colour description; an HDR10 SEI NAL is inserted immediately before
/// the first slice of every IRAP picture (after that access unit's VPS/SPS/PPS, where a prefix
/// SEI belongs) — unless the access unit already carries a mastering-display SEI, in which case
/// it's left alone (keeping the patch idempotent). Errors if the input has no start code or no
/// SPS.
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

    // Rough upper bound: one SEI per NAL is a wild over-estimate but cheap to reserve against.
    let sei_len = sei_wire.as_ref().map_or(0, |s| s.len() + 4);
    let mut out = Vec::with_capacity(input.len() + sei_len * 4 + 64);
    let mut report = PatchReport {
        sps_patched: 0,
        sei_insertions: 0,
    };
    // The payload type whose presence in an access unit means our HDR10 SEI is already there:
    // the mastering-display message when we carry it (the normal HDR10 case), otherwise the
    // content-light-level message (a CLL-only configuration). Dedup on it keeps the patch
    // idempotent for both.
    let dedup_type = if meta.mastering_display.is_some() {
        SEI_MASTERING_DISPLAY
    } else {
        SEI_CONTENT_LIGHT_LEVEL
    };
    // Does the access unit currently being copied already carry that SEI? Reset at each picture
    // boundary; set by any prefix SEI that precedes the picture's slices.
    let mut au_has_hdr10 = false;
    // Ensures the very first picture always gets an SEI even in a (malformed) stream that
    // doesn't open on an IRAP — preserving the previous "SEI before the first VCL" guarantee.
    let mut seen_first_pic = false;
    let mut cursor = 0usize;

    for nal in &nals {
        // Copy anything between the previous NAL and this one (leading bytes / inter-NAL padding).
        out.extend_from_slice(&input[cursor..nal.start_code]);

        // Track HDR10 signalling already present in this access unit (prefix SEIs precede the
        // picture's slices, so this is known by the time we reach the first slice).
        if nal.nal_type == NAL_PREFIX_SEI
            && rbsp_has_sei_payload(&remove_emulation_prevention(nal.raw(input)), dedup_type)
        {
            au_has_hdr10 = true;
        }

        // At the first slice of each picture, insert the SEI if this is a random-access point
        // (or the stream's first picture) and the access unit doesn't already signal HDR10.
        if nal.is_first_slice(input) {
            if let Some(sei) = &sei_wire
                && !au_has_hdr10
                && (nal.is_irap() || !seen_first_pic)
            {
                out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                out.extend_from_slice(sei);
                report.sei_insertions += 1;
            }
            seen_first_pic = true;
            au_has_hdr10 = false; // the next access unit starts fresh
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

    // No VCL NAL at all (parameter-set-only stream): place a single SEI after everything —
    // but not if that lone access unit already signals HDR10 (keeps re-patching idempotent).
    if let Some(sei) = &sei_wire
        && !seen_first_pic
        && !au_has_hdr10
    {
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        out.extend_from_slice(sei);
        report.sei_insertions += 1;
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
        // IDR slice (type 19, VCL): 0x26 header, then a slice byte with first_slice flag set (0x80).
        s.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x26, 0x01, 0x80, 0xEE]);
        s
    }

    /// A VCL slice NAL of `nal_type` whose `first_slice_segment_in_pic_flag` is `first`.
    fn slice(nal_type: u8, first: bool) -> Vec<u8> {
        let header0 = nal_type << 1; // forbidden_zero_bit=0, layer high bit=0
        let slice_byte = if first { 0x80 } else { 0x00 };
        vec![0x00, 0x00, 0x00, 0x01, header0, 0x01, slice_byte, 0x11]
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
        assert_eq!(report.sei_insertions, 1);
        assert!(report.sei_inserted());

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
    fn inserts_sei_before_every_irap() {
        // Two GOPs: IDR (19) opening, a trailing P slice (type 1, not IRAP), then a mid-stream
        // CRA (21) opening the second GOP with a repeated SPS for random access.
        let mut input = build_stream(); // VPS, SPS, PPS, IDR(19)
        input.extend_from_slice(&slice(1, true)); // TRAIL_R P-slice — no SEI here
        input.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // repeated SPS at the random-access point
        input.extend_from_slice(&hex(SPS_PLAIN));
        input.extend_from_slice(&slice(21, true)); // CRA IRAP — gets its own SEI

        let (out, report) = patch_stream(&input, &meta()).unwrap();
        assert_eq!(report.sps_patched, 2); // both SPS copies rewritten
        assert_eq!(report.sei_insertions, 2); // one SEI per IRAP (IDR + CRA), none at the P-slice

        // Each IRAP slice is immediately preceded by a prefix SEI; the P-slice is not.
        let nals = find_nals(&out);
        for (i, n) in nals.iter().enumerate() {
            if n.is_irap() {
                assert_eq!(nals[i - 1].nal_type, NAL_PREFIX_SEI, "IRAP not preceded by SEI");
            }
        }
        let p_slice = nals.iter().position(|n| n.nal_type == 1).unwrap();
        assert_ne!(nals[p_slice - 1].nal_type, NAL_PREFIX_SEI);
    }

    #[test]
    fn multi_slice_irap_gets_one_sei() {
        // An IRAP picture split into two slices: only the first slice segment triggers a SEI.
        let mut input = build_stream(); // ends with IDR first-slice
        input.extend_from_slice(&slice(19, false)); // dependent/second slice of the same picture
        let (out, report) = patch_stream(&input, &meta()).unwrap();
        assert_eq!(report.sei_insertions, 1);
        assert_eq!(
            find_nals(&out)
                .iter()
                .filter(|n| n.nal_type == NAL_PREFIX_SEI)
                .count(),
            1
        );
    }

    #[test]
    fn skips_irap_that_already_has_mastering_display() {
        // Build a stream whose IDR access unit already carries an HDR10 SEI: running the patch
        // must not add a second one (idempotency). Feed the output of one patch back in.
        let input = build_stream();
        let (once, r1) = patch_stream(&input, &meta()).unwrap();
        assert_eq!(r1.sei_insertions, 1);
        let (twice, r2) = patch_stream(&once, &meta()).unwrap();
        assert_eq!(r2.sei_insertions, 0, "re-patch must not duplicate the SEI");
        assert_eq!(
            find_nals(&twice)
                .iter()
                .filter(|n| n.nal_type == NAL_PREFIX_SEI)
                .count(),
            1
        );
    }

    #[test]
    fn content_light_only_is_idempotent() {
        // A content-light-level-only configuration must also dedup on re-patch (the scanner
        // keys on whichever message we write, not just mastering-display).
        let input = build_stream();
        let mut m = meta();
        m.mastering_display = None; // CLL-only SEI
        let (once, r1) = patch_stream(&input, &m).unwrap();
        assert_eq!(r1.sei_insertions, 1);
        let (_twice, r2) = patch_stream(&once, &m).unwrap();
        assert_eq!(r2.sei_insertions, 0, "CLL-only re-patch must not duplicate");
    }

    #[test]
    fn parameter_set_only_stream_is_idempotent() {
        // A VCL-less stream (VPS/SPS/PPS only) gets one SEI at the end; re-patching it — now
        // that the lone access unit carries the SEI — must not add a second.
        let mut s = Vec::new();
        s.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x40, 0x01, 0xAA, 0xBB]); // VPS
        s.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        s.extend_from_slice(&hex(SPS_PLAIN)); // SPS
        s.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x44, 0x01, 0xCC]); // PPS
        let (once, r1) = patch_stream(&s, &meta()).unwrap();
        assert_eq!(r1.sei_insertions, 1);
        let (_twice, r2) = patch_stream(&once, &meta()).unwrap();
        assert_eq!(r2.sei_insertions, 0, "no-VCL re-patch must not duplicate");
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
        assert_eq!(report.sei_insertions, 0);
        assert!(!report.sei_inserted());
        assert!(!find_nals(&out).iter().any(|n| n.nal_type == NAL_PREFIX_SEI));
    }
}
