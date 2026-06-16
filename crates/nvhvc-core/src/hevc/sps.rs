//! Surgical edit of the SPS VUI colour signalling.
//!
//! HDR10 in-band signalling lives in the SPS `vui_parameters()` colour description
//! (`colour_primaries`, `transfer_characteristics`, `matrix_coeffs`, `video_full_range_flag`,
//! `video_format`) and, optionally, the chroma sample location. To change it without disturbing
//! anything else, we:
//!
//! 1. Traverse the SPS from the start, *correctly* skipping every preceding structure
//!    (`profile_tier_level`, `scaling_list_data`, `st_ref_pic_set`, …) so the cursor lands exactly
//!    on the VUI colour region.
//! 2. Copy the bits before that region (the "prefix") and after it (the "tail") **verbatim** as raw
//!    bit runs. Nothing in those regions is re-serialised, so HRD parameters, timing info and SPS
//!    extensions are bit-preserved.
//! 3. Re-emit only the small colour region from the requested values.
//! 4. Re-attach the tail, then byte-align — which reconstructs valid `rbsp_trailing_bits` at the
//!    new alignment.
//!
//! All bit offsets are relative to the SPS NAL **including its 2-byte NAL header** (so the prefix
//! carries the header through unchanged).

use crate::bitio::{BitBuf, BitReader, BitWriter};
use crate::error::Result;

/// The colour signalling to write into an SPS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VuiColorEdit {
    /// `video_format` (3 bits); 5 = unspecified is the usual choice.
    pub video_format: u8,
    /// `video_full_range_flag`: full-range (`true`) vs limited/TV-range (`false`).
    pub video_full_range: bool,
    /// `colour_primaries`, e.g. 9 = BT.2020.
    pub colour_primaries: u8,
    /// `transfer_characteristics`, e.g. 16 = SMPTE ST 2084 (PQ).
    pub transfer_characteristics: u8,
    /// `matrix_coeffs`, e.g. 9 = BT.2020 non-constant luminance.
    pub matrix_coeffs: u8,
    /// Chroma sample location (`top` and `bottom` set equal). `None` leaves chroma loc untouched.
    pub chroma_loc: Option<u32>,
}

/// The colour signalling currently present in an SPS VUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExistingColor {
    /// Whether a `video_signal_type` block is present.
    pub video_signal_type_present: bool,
    /// `video_format`.
    pub video_format: u8,
    /// `video_full_range_flag`.
    pub video_full_range: bool,
    /// Whether a colour description (`primaries`/`transfer`/`matrix`) is present.
    pub colour_description_present: bool,
    /// `colour_primaries` (meaningful only if `colour_description_present`).
    pub colour_primaries: u8,
    /// `transfer_characteristics`.
    pub transfer_characteristics: u8,
    /// `matrix_coeffs`.
    pub matrix_coeffs: u8,
    /// Whether chroma sample location info is present.
    pub chroma_loc_present: bool,
    /// `chroma_sample_loc_type_top_field`.
    pub chroma_loc_top: u32,
    /// `chroma_sample_loc_type_bottom_field`.
    pub chroma_loc_bottom: u32,
}

/// Result of traversing an SPS to the VUI colour region.
#[derive(Debug, Clone)]
struct Layout {
    /// `true` if `vui_parameters_present_flag` was set.
    vui_present: bool,
    /// Bit position of `vui_parameters_present_flag` itself.
    vui_flag_pos: usize,
    /// (VUI present only) bit position where the `video_signal_type` block starts.
    vst_start: usize,
    /// (VUI present only) bit position where the `video_signal_type` block ends.
    vst_end: usize,
    /// (VUI present only) bit position where the chroma-loc block ends (== start of tail).
    chroma_end: usize,
    /// Total bit length of the RBSP.
    total_bits: usize,
    /// The colour values currently encoded (None if there is no VUI at all).
    existing: Option<ExistingColor>,
}

/// Reads the colour signalling currently present in an SPS NAL's RBSP.
///
/// `rbsp` is the de-emulated SPS NAL (2-byte header included). Returns `None` if the SPS has no
/// VUI at all.
pub fn read_color(rbsp: &[u8]) -> Result<Option<ExistingColor>> {
    Ok(analyze(rbsp)?.existing)
}

/// Rewrites an SPS NAL's RBSP, injecting/overriding the HDR10 colour signalling.
///
/// `rbsp` is the de-emulated SPS NAL (2-byte header included); the returned `Vec` is the new
/// de-emulated NAL. Callers re-insert emulation-prevention bytes before writing to the wire.
pub fn edit_color(rbsp: &[u8], edit: &VuiColorEdit) -> Result<Vec<u8>> {
    let layout = analyze(rbsp)?;
    let mut w = BitWriter::new();

    if layout.vui_present {
        // prefix: everything up to the video_signal_type block (header, SPS body, aspect/overscan).
        w.append_bitbuf(&slice_bits(rbsp, 0, layout.vst_start));
        write_video_signal_type(&mut w, edit);
        match edit.chroma_loc {
            Some(loc) => write_chroma_loc(&mut w, Some(loc)),
            // Preserve the original chroma-loc bits verbatim.
            None => w.append_bitbuf(&slice_bits(rbsp, layout.vst_end, layout.chroma_end)),
        }
        // tail: rest of the VUI, SPS extensions and rbsp_trailing_bits.
        let tail = slice_bits(rbsp, layout.chroma_end, layout.total_bits);
        w.append_bitbuf(&trim_trailing_zero_bits(&tail));
    } else {
        // No VUI present: synthesise a minimal one and flip vui_parameters_present_flag to 1.
        w.append_bitbuf(&slice_bits(rbsp, 0, layout.vui_flag_pos));
        w.write_flag(true); // vui_parameters_present_flag = 1
        write_synthetic_vui(&mut w, edit);
        // tail: everything the original had after the (zero) flag — SPS extensions + trailing.
        let tail = slice_bits(rbsp, layout.vui_flag_pos + 1, layout.total_bits);
        w.append_bitbuf(&trim_trailing_zero_bits(&tail));
    }

    // Byte-align: the trimmed tail ends with rbsp_stop_one_bit, so padding zeros here reconstructs
    // rbsp_trailing_bits at the new alignment.
    Ok(w.into_bytes())
}

fn write_video_signal_type(w: &mut BitWriter, edit: &VuiColorEdit) {
    w.write_flag(true); // video_signal_type_present_flag
    w.write_bits(u64::from(edit.video_format), 3);
    w.write_flag(edit.video_full_range);
    w.write_flag(true); // colour_description_present_flag
    w.write_bits(u64::from(edit.colour_primaries), 8);
    w.write_bits(u64::from(edit.transfer_characteristics), 8);
    w.write_bits(u64::from(edit.matrix_coeffs), 8);
}

fn write_chroma_loc(w: &mut BitWriter, loc: Option<u32>) {
    match loc {
        Some(v) => {
            w.write_flag(true); // chroma_loc_info_present_flag
            w.write_ue(u64::from(v));
            w.write_ue(u64::from(v));
        }
        None => w.write_flag(false),
    }
}

/// Writes a complete minimal `vui_parameters()` carrying just the colour description.
fn write_synthetic_vui(w: &mut BitWriter, edit: &VuiColorEdit) {
    w.write_flag(false); // aspect_ratio_info_present_flag
    w.write_flag(false); // overscan_info_present_flag
    write_video_signal_type(w, edit);
    write_chroma_loc(w, edit.chroma_loc);
    w.write_flag(false); // neutral_chroma_indication_flag
    w.write_flag(false); // field_seq_flag
    w.write_flag(false); // frame_field_info_present_flag
    w.write_flag(false); // default_display_window_flag
    w.write_flag(false); // vui_timing_info_present_flag
    w.write_flag(false); // bitstream_restriction_flag
}

/// Slices `[start, end)` bits out of `rbsp` into an owned [`BitBuf`].
fn slice_bits(rbsp: &[u8], start: usize, end: usize) -> BitBuf {
    let mut r = BitReader::new(rbsp);
    r.seek(start);
    r.read_bitbuf(end - start).expect("slice within bounds")
}

/// Drops trailing zero bits down to (and including) the final one bit (`rbsp_stop_one_bit`).
fn trim_trailing_zero_bits(buf: &BitBuf) -> BitBuf {
    let mut len = buf.bit_len;
    while len > 0 {
        let idx = len - 1;
        let bit = (buf.bytes[idx / 8] >> (7 - idx % 8)) & 1;
        if bit == 1 {
            break;
        }
        len -= 1;
    }
    BitReader::new(&buf.bytes)
        .read_bitbuf(len)
        .expect("trim within bounds")
}

/// Reads `n` bits without retaining them.
fn skip(r: &mut BitReader, mut n: usize) -> Result<()> {
    while n > 0 {
        let take = n.min(64) as u32;
        r.read_bits(take)?;
        n -= take as usize;
    }
    Ok(())
}

/// `profile_tier_level(1, maxNumSubLayersMinus1)` — profile is always present in an SPS.
fn skip_profile_tier_level(r: &mut BitReader, max_sub_layers_minus1: u8) -> Result<()> {
    // general: profile_space(2)+tier(1)+idc(5) + 32 compat + 4 source flags + 44 reserved = 88 bits.
    skip(r, 88)?;
    skip(r, 8)?; // general_level_idc
    let n = max_sub_layers_minus1 as usize;
    let mut profile_present = [false; 8];
    let mut level_present = [false; 8];
    for i in 0..n {
        profile_present[i] = r.read_flag()?;
        level_present[i] = r.read_flag()?;
    }
    if n > 0 {
        for _ in n..8 {
            skip(r, 2)?; // reserved_zero_2bits
        }
    }
    for i in 0..n {
        if profile_present[i] {
            skip(r, 88)?; // sub_layer profile block (same shape as general, no level idc)
        }
        if level_present[i] {
            skip(r, 8)?; // sub_layer_level_idc
        }
    }
    Ok(())
}

/// `scaling_list_data()` (H.265 §7.3.4).
fn skip_scaling_list_data(r: &mut BitReader) -> Result<()> {
    for size_id in 0..4usize {
        let step = if size_id == 3 { 3 } else { 1 };
        let mut matrix_id = 0usize;
        while matrix_id < 6 {
            let pred_mode_flag = r.read_flag()?;
            if !pred_mode_flag {
                r.read_ue()?; // scaling_list_pred_matrix_id_delta
            } else {
                let coef_num = core::cmp::min(64usize, 1usize << (4 + (size_id << 1)));
                if size_id > 1 {
                    r.read_se()?; // scaling_list_dc_coef_minus8
                }
                for _ in 0..coef_num {
                    r.read_se()?; // scaling_list_delta_coef
                }
            }
            matrix_id += step;
        }
    }
    Ok(())
}

/// All `st_ref_pic_set(i)` in the SPS loop, tracking `NumDeltaPocs` for inter-predicted sets
/// (H.265 §7.3.7 / §7.4.8). In SPS context `delta_idx_minus1` is never present.
fn skip_st_ref_pic_sets(r: &mut BitReader, num_sets: u32) -> Result<()> {
    let mut num_delta_pocs = vec![0u32; num_sets as usize];
    for idx in 0..num_sets as usize {
        let inter_pred = if idx != 0 { r.read_flag()? } else { false };
        if inter_pred {
            let ref_idx = idx - 1; // RefRpsIdx = stRpsIdx - (delta_idx_minus1 + 1), delta_idx_minus1 == 0
            r.read_flag()?; // delta_rps_sign
            r.read_ue()?; // abs_delta_rps_minus1
            let ref_count = num_delta_pocs[ref_idx];
            let mut derived = 0u32;
            for _ in 0..=ref_count {
                let used_by_curr = r.read_flag()?;
                // use_delta_flag is inferred 1 when used_by_curr_pic_flag is 1.
                let use_delta = if used_by_curr { true } else { r.read_flag()? };
                if use_delta {
                    derived += 1;
                }
            }
            num_delta_pocs[idx] = derived;
        } else {
            let num_negative = r.read_ue32()?;
            let num_positive = r.read_ue32()?;
            for _ in 0..num_negative {
                r.read_ue()?; // delta_poc_s0_minus1
                r.read_flag()?; // used_by_curr_pic_s0_flag
            }
            for _ in 0..num_positive {
                r.read_ue()?; // delta_poc_s1_minus1
                r.read_flag()?; // used_by_curr_pic_s1_flag
            }
            num_delta_pocs[idx] = num_negative + num_positive;
        }
    }
    Ok(())
}

/// Traverses an SPS NAL RBSP up to and across the VUI colour region.
fn analyze(rbsp: &[u8]) -> Result<Layout> {
    let mut r = BitReader::new(rbsp);
    let total_bits = r.len_bits();

    // NAL unit header: forbidden(1) + type(6) + layer_id(6) + temporal_id_plus1(3).
    skip(&mut r, 16)?;

    skip(&mut r, 4)?; // sps_video_parameter_set_id
    let max_sub_layers_minus1 = r.read_bits(3)? as u8;
    skip(&mut r, 1)?; // sps_temporal_id_nesting_flag
    skip_profile_tier_level(&mut r, max_sub_layers_minus1)?;
    r.read_ue()?; // sps_seq_parameter_set_id
    let chroma_format_idc = r.read_ue()?;
    if chroma_format_idc == 3 {
        skip(&mut r, 1)?; // separate_colour_plane_flag
    }
    r.read_ue()?; // pic_width_in_luma_samples
    r.read_ue()?; // pic_height_in_luma_samples
    if r.read_flag()? {
        // conformance_window_flag
        r.read_ue()?;
        r.read_ue()?;
        r.read_ue()?;
        r.read_ue()?;
    }
    r.read_ue()?; // bit_depth_luma_minus8
    r.read_ue()?; // bit_depth_chroma_minus8
    let log2_max_poc_lsb_minus4 = r.read_ue32()?;
    let sub_layer_ordering_info_present = r.read_flag()?;
    let first = if sub_layer_ordering_info_present {
        0
    } else {
        u32::from(max_sub_layers_minus1)
    };
    for _ in first..=u32::from(max_sub_layers_minus1) {
        r.read_ue()?; // sps_max_dec_pic_buffering_minus1
        r.read_ue()?; // sps_max_num_reorder_pics
        r.read_ue()?; // sps_max_latency_increase_plus1
    }
    r.read_ue()?; // log2_min_luma_coding_block_size_minus3
    r.read_ue()?; // log2_diff_max_min_luma_coding_block_size
    r.read_ue()?; // log2_min_luma_transform_block_size_minus2
    r.read_ue()?; // log2_diff_max_min_luma_transform_block_size
    r.read_ue()?; // max_transform_hierarchy_depth_inter
    r.read_ue()?; // max_transform_hierarchy_depth_intra
    if r.read_flag()? {
        // scaling_list_enabled_flag
        if r.read_flag()? {
            // sps_scaling_list_data_present_flag
            skip_scaling_list_data(&mut r)?;
        }
    }
    skip(&mut r, 1)?; // amp_enabled_flag
    skip(&mut r, 1)?; // sample_adaptive_offset_enabled_flag
    if r.read_flag()? {
        // pcm_enabled_flag
        skip(&mut r, 8)?; // pcm bit-depth luma/chroma minus1 (4 + 4)
        r.read_ue()?; // log2_min_pcm_luma_coding_block_size_minus3
        r.read_ue()?; // log2_diff_max_min_pcm_luma_coding_block_size
        skip(&mut r, 1)?; // pcm_loop_filter_disabled_flag
    }
    let num_st_rps = r.read_ue32()?;
    skip_st_ref_pic_sets(&mut r, num_st_rps)?;
    if r.read_flag()? {
        // long_term_ref_pics_present_flag
        let num_lt = r.read_ue32()?;
        let poc_bits = (log2_max_poc_lsb_minus4 + 4) as usize;
        for _ in 0..num_lt {
            skip(&mut r, poc_bits)?; // lt_ref_pic_poc_lsb_sps
            skip(&mut r, 1)?; // used_by_curr_pic_lt_sps_flag
        }
    }
    skip(&mut r, 1)?; // sps_temporal_mvp_enabled_flag
    skip(&mut r, 1)?; // strong_intra_smoothing_enabled_flag

    let vui_flag_pos = r.position();
    let vui_present = r.read_flag()?;

    if !vui_present {
        return Ok(Layout {
            vui_present: false,
            vui_flag_pos,
            vst_start: 0,
            vst_end: 0,
            chroma_end: 0,
            total_bits,
            existing: None,
        });
    }

    // vui_parameters(): aspect_ratio and overscan precede the colour region (kept in the prefix).
    if r.read_flag()? {
        // aspect_ratio_info_present_flag
        let idc = r.read_bits(8)?;
        if idc == 255 {
            skip(&mut r, 32)?; // sar_width(16) + sar_height(16)
        }
    }
    if r.read_flag()? {
        // overscan_info_present_flag
        skip(&mut r, 1)?; // overscan_appropriate_flag
    }

    let vst_start = r.position();
    let mut existing = ExistingColor {
        video_signal_type_present: false,
        video_format: 5,
        video_full_range: false,
        colour_description_present: false,
        colour_primaries: 2,
        transfer_characteristics: 2,
        matrix_coeffs: 2,
        chroma_loc_present: false,
        chroma_loc_top: 0,
        chroma_loc_bottom: 0,
    };
    if r.read_flag()? {
        // video_signal_type_present_flag
        existing.video_signal_type_present = true;
        existing.video_format = r.read_bits(3)? as u8;
        existing.video_full_range = r.read_flag()?;
        if r.read_flag()? {
            // colour_description_present_flag
            existing.colour_description_present = true;
            existing.colour_primaries = r.read_bits(8)? as u8;
            existing.transfer_characteristics = r.read_bits(8)? as u8;
            existing.matrix_coeffs = r.read_bits(8)? as u8;
        }
    }
    let vst_end = r.position();
    if r.read_flag()? {
        // chroma_loc_info_present_flag
        existing.chroma_loc_present = true;
        existing.chroma_loc_top = r.read_ue32()?;
        existing.chroma_loc_bottom = r.read_ue32()?;
    }
    let chroma_end = r.position();

    Ok(Layout {
        vui_present: true,
        vui_flag_pos,
        vst_start,
        vst_end,
        chroma_end,
        total_bits,
        existing: Some(existing),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hevc::nal::{add_emulation_prevention, find_nals, remove_emulation_prevention};

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // Real SPS NAL units (with emulation-prevention bytes) extracted from x265 output.
    const SPS_PLAIN: &str =
        "42010101600000030090000003000003003ca00a080f165959a4932bc05a020000030002000003003010";
    const SPS_HDR10: &str = "42010102200000030090000003000003003ca00a080f1365959a4932bc05a84880482000000300200000030301";
    const SPS_FEATURES: &str =
        "42010101600000030090000003000003003ca00a080f165959a49395e02d010000030001000003001808";

    const ALL: &[(&str, &str)] = &[
        ("plain", SPS_PLAIN),
        ("hdr10", SPS_HDR10),
        ("features", SPS_FEATURES),
    ];

    fn pq_edit() -> VuiColorEdit {
        VuiColorEdit {
            video_format: 5,
            video_full_range: false,
            colour_primaries: 9,          // BT.2020
            transfer_characteristics: 16, // SMPTE ST 2084 (PQ)
            matrix_coeffs: 9,             // BT.2020 NCL
            chroma_loc: Some(2),
        }
    }

    #[test]
    fn analyze_succeeds_on_all_samples() {
        for (name, h) in ALL {
            let rbsp = remove_emulation_prevention(&hex(h));
            let layout = analyze(&rbsp).unwrap_or_else(|e| panic!("analyze {name} failed: {e}"));
            assert!(layout.vui_present, "{name} should have a VUI");
        }
    }

    #[test]
    fn inject_then_read_back_roundtrips_values() {
        let edit = pq_edit();
        for (name, h) in ALL {
            let rbsp = remove_emulation_prevention(&hex(h));
            let out = edit_color(&rbsp, &edit).unwrap_or_else(|e| panic!("edit {name}: {e}"));

            let color = read_color(&out)
                .unwrap_or_else(|e| panic!("read_color {name}: {e}"))
                .unwrap_or_else(|| panic!("{name} lost its VUI"));
            assert!(color.colour_description_present, "{name}");
            assert_eq!(color.colour_primaries, 9, "{name} primaries");
            assert_eq!(color.transfer_characteristics, 16, "{name} transfer");
            assert_eq!(color.matrix_coeffs, 9, "{name} matrix");
            assert_eq!(color.video_format, 5, "{name} video_format");
            assert!(color.chroma_loc_present, "{name} chroma present");
            assert_eq!(color.chroma_loc_top, 2, "{name} chroma top");
            assert_eq!(color.chroma_loc_bottom, 2, "{name} chroma bottom");

            // The edited NAL must still survive an emulation-prevention round trip and re-parse.
            let wire = add_emulation_prevention(&out);
            let nals = find_nals(&{
                let mut v = vec![0u8, 0, 0, 1];
                v.extend_from_slice(&wire);
                v
            });
            assert_eq!(nals.len(), 1, "{name} re-split");
            assert_eq!(nals[0].nal_type, 33, "{name} still an SPS");
        }
    }

    #[test]
    fn prefix_is_preserved_byte_for_byte() {
        // Everything before the colour region must be identical after an edit.
        let edit = pq_edit();
        for (name, h) in ALL {
            let rbsp = remove_emulation_prevention(&hex(h));
            let layout = analyze(&rbsp).unwrap();
            let out = edit_color(&rbsp, &edit).unwrap();
            let before = slice_bits(&rbsp, 0, layout.vst_start);
            let after = slice_bits(&out, 0, layout.vst_start);
            assert_eq!(before, after, "{name} prefix changed");
        }
    }

    #[test]
    fn identity_when_reapplying_existing_color() {
        // For a sample that already carries a full colour description, re-applying exactly those
        // values (and preserving chroma loc) must reproduce the RBSP byte-for-byte. This is the
        // strong test that the traversal located the colour region precisely.
        for (name, h) in ALL {
            let rbsp = remove_emulation_prevention(&hex(h));
            let Some(existing) = read_color(&rbsp).unwrap() else {
                continue;
            };
            if !existing.colour_description_present {
                continue;
            }
            let edit = VuiColorEdit {
                video_format: existing.video_format,
                video_full_range: existing.video_full_range,
                colour_primaries: existing.colour_primaries,
                transfer_characteristics: existing.transfer_characteristics,
                matrix_coeffs: existing.matrix_coeffs,
                chroma_loc: None, // preserve original chroma-loc bits verbatim
            };
            let out = edit_color(&rbsp, &edit).unwrap();
            assert_eq!(out, rbsp, "{name} should round-trip to identical bytes");
        }
    }
}
