//! Construction of the HDR10 SEI NAL unit.
//!
//! HDR10 carries two static-metadata SEI messages (H.265 Annex D):
//!
//! * **Mastering display colour volume** (`payloadType` 137, 24 bytes) — the display primaries,
//!   white point and luminance range of the grading display.
//! * **Content light level information** (`payloadType` 144, 4 bytes) — MaxCLL and MaxFALL.
//!
//! Both are packed into a single prefix-SEI NAL (`nal_unit_type` 39). The returned bytes are the
//! NAL **RBSP** (2-byte header included, emulation-prevention *not* yet applied) so the caller can
//! treat it exactly like an edited SPS: run [`add_emulation_prevention`] then prepend a start code.
//!
//! [`add_emulation_prevention`]: crate::hevc::nal::add_emulation_prevention

/// `payloadType` for mastering display colour volume.
pub const SEI_MASTERING_DISPLAY: u8 = 137;
/// `payloadType` for content light level information.
pub const SEI_CONTENT_LIGHT_LEVEL: u8 = 144;

/// Mastering display colour volume metadata, in the SEI's own integer units.
///
/// Chromaticity coordinates are in units of 0.00002; luminance is in units of 0.0001 cd/m². The
/// primaries are ordered green, blue, red (i.e. index 0 = green) as the SEI requires — which is the
/// same order as the familiar `G(..)B(..)R(..)` mastering-display string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasteringDisplay {
    /// `display_primaries_x[c]` for c = green, blue, red.
    pub primaries_x: [u16; 3],
    /// `display_primaries_y[c]` for c = green, blue, red.
    pub primaries_y: [u16; 3],
    /// `white_point_x`.
    pub white_point_x: u16,
    /// `white_point_y`.
    pub white_point_y: u16,
    /// `max_display_mastering_luminance` (0.0001 cd/m² units).
    pub max_luminance: u32,
    /// `min_display_mastering_luminance` (0.0001 cd/m² units).
    pub min_luminance: u32,
}

/// Content light level information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentLightLevel {
    /// MaxCLL — `max_content_light_level` (cd/m²).
    pub max_cll: u16,
    /// MaxFALL — `max_pic_average_light_level` (cd/m²).
    pub max_fall: u16,
}

/// Builds the HDR10 prefix-SEI NAL RBSP from whichever messages are present.
///
/// Returns `None` if neither message is supplied (so the caller can skip insertion entirely).
pub fn build_hdr10_sei_rbsp(
    mastering: Option<&MasteringDisplay>,
    light_level: Option<&ContentLightLevel>,
) -> Option<Vec<u8>> {
    if mastering.is_none() && light_level.is_none() {
        return None;
    }

    // Prefix-SEI NAL header: forbidden_zero_bit=0, nal_unit_type=39, nuh_layer_id=0,
    // nuh_temporal_id_plus1=1  ->  0x4E 0x01.
    let mut nal = vec![0x4E, 0x01];

    if let Some(md) = mastering {
        let mut payload = Vec::with_capacity(24);
        for c in 0..3 {
            payload.extend_from_slice(&md.primaries_x[c].to_be_bytes());
            payload.extend_from_slice(&md.primaries_y[c].to_be_bytes());
        }
        payload.extend_from_slice(&md.white_point_x.to_be_bytes());
        payload.extend_from_slice(&md.white_point_y.to_be_bytes());
        payload.extend_from_slice(&md.max_luminance.to_be_bytes());
        payload.extend_from_slice(&md.min_luminance.to_be_bytes());
        debug_assert_eq!(payload.len(), 24);
        write_sei_message(&mut nal, SEI_MASTERING_DISPLAY, &payload);
    }

    if let Some(cll) = light_level {
        let mut payload = Vec::with_capacity(4);
        payload.extend_from_slice(&cll.max_cll.to_be_bytes());
        payload.extend_from_slice(&cll.max_fall.to_be_bytes());
        write_sei_message(&mut nal, SEI_CONTENT_LIGHT_LEVEL, &payload);
    }

    // rbsp_trailing_bits(): rbsp_stop_one_bit then zero padding. SEI payloads are byte-aligned, so
    // this is a single 0x80 byte. (The original Python tool wrote 0x00 here, which is invalid.)
    nal.push(0x80);
    Some(nal)
}

/// Whether a prefix-SEI NAL's RBSP already carries an SEI message of `payload_type`. Used to
/// avoid inserting a duplicate at an access unit that already signals HDR10 — which keeps the
/// patch idempotent (re-running it inserts nothing new). Callers pass whichever payload type
/// they're about to write ([`SEI_MASTERING_DISPLAY`], or [`SEI_CONTENT_LIGHT_LEVEL`] for a
/// content-light-only configuration).
///
/// `rbsp` is the emulation-prevention-removed NAL, including its 2-byte header. Parses the
/// `sei_message()` loop of H.265 §7.3.5, stopping at `rbsp_trailing_bits`.
pub fn rbsp_has_sei_payload(rbsp: &[u8], payload_type: u8) -> bool {
    let mut i = 2usize; // skip the 2-byte NAL header
    loop {
        // more_rbsp_data() is false once only rbsp_trailing_bits remain (a `0x80` byte-aligned
        // stop bit followed by zero padding).
        match rbsp.get(i) {
            None => break,
            Some(&0x80) if rbsp[i + 1..].iter().all(|&b| b == 0) => break,
            _ => {}
        }
        // payload_type and payload_size are each coded as 0xFF-extended byte runs.
        let Some(this_type) = read_ff_extended(rbsp, &mut i) else {
            break;
        };
        let Some(payload_size) = read_ff_extended(rbsp, &mut i) else {
            break;
        };
        if this_type == u32::from(payload_type) {
            return true;
        }
        i = i.saturating_add(payload_size as usize);
        if i > rbsp.len() {
            break;
        }
    }
    false
}

/// Reads one 0xFF-extended value (`floor(v/255)` bytes of `0xFF` then a final `< 255` byte),
/// advancing `i`. Returns `None` if the buffer ends mid-value. Saturates rather than
/// overflowing on a pathologically long `0xFF` run in corrupt input.
fn read_ff_extended(buf: &[u8], i: &mut usize) -> Option<u32> {
    let mut value = 0u32;
    while buf.get(*i) == Some(&0xFF) {
        value = value.saturating_add(255);
        *i += 1;
    }
    let last = *buf.get(*i)?;
    *i += 1;
    Some(value.saturating_add(u32::from(last)))
}

/// Appends one `sei_message()`: payload type, payload size (both 0xFF-extended), then the payload.
fn write_sei_message(out: &mut Vec<u8>, payload_type: u8, payload: &[u8]) {
    write_ff_extended(out, u32::from(payload_type));
    write_ff_extended(out, payload.len() as u32);
    out.extend_from_slice(payload);
}

/// Writes a value as `floor(v / 255)` bytes of `0xFF` followed by `v % 255` (H.265 §7.3.5).
fn write_ff_extended(out: &mut Vec<u8>, mut value: u32) {
    while value >= 255 {
        out.push(0xFF);
        value -= 255;
    }
    out.push(value as u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hevc::nal::{add_emulation_prevention, remove_emulation_prevention};

    fn sample_md() -> MasteringDisplay {
        // x265: G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1)
        MasteringDisplay {
            primaries_x: [13250, 7500, 34000],
            primaries_y: [34500, 3000, 16000],
            white_point_x: 15635,
            white_point_y: 16450,
            max_luminance: 10_000_000,
            min_luminance: 1,
        }
    }

    #[test]
    fn none_when_empty() {
        assert!(build_hdr10_sei_rbsp(None, None).is_none());
    }

    #[test]
    fn cll_only_layout() {
        let cll = ContentLightLevel {
            max_cll: 1000,
            max_fall: 400,
        };
        let nal = build_hdr10_sei_rbsp(None, Some(&cll)).unwrap();
        // header(2) + type(0x90) + size(0x04) + 4 payload + trailing(0x80) = 9 bytes.
        assert_eq!(
            nal,
            vec![0x4E, 0x01, 0x90, 0x04, 0x03, 0xE8, 0x01, 0x90, 0x80]
        );
    }

    #[test]
    fn md_message_has_type_137_size_24() {
        let nal = build_hdr10_sei_rbsp(Some(&sample_md()), None).unwrap();
        assert_eq!(&nal[0..2], &[0x4E, 0x01]);
        assert_eq!(nal[2], 137); // payload type
        assert_eq!(nal[3], 24); // payload size
        assert_eq!(*nal.last().unwrap(), 0x80); // rbsp_trailing_bits
        assert_eq!(nal.len(), 2 + 1 + 1 + 24 + 1);
    }

    #[test]
    fn both_messages_in_order() {
        let cll = ContentLightLevel {
            max_cll: 1000,
            max_fall: 400,
        };
        let nal = build_hdr10_sei_rbsp(Some(&sample_md()), Some(&cll)).unwrap();
        // header(2) + md(1+1+24) + cll(1+1+4) + trailing(1).
        assert_eq!(nal.len(), 2 + 26 + 6 + 1);
        assert_eq!(nal[2], 137);
        assert_eq!(nal[2 + 26], 144);
    }

    #[test]
    fn survives_emulation_prevention_roundtrip() {
        // min_luminance = 1 puts a literal 00 00 00 01 in the payload, which must be protected.
        let nal = build_hdr10_sei_rbsp(Some(&sample_md()), None).unwrap();
        let wire = add_emulation_prevention(&nal);
        // No forbidden start-code emulation remains.
        assert!(!wire.windows(3).any(|w| w == [0x00, 0x00, 0x01]));
        assert_eq!(remove_emulation_prevention(&wire), nal);
    }

    #[test]
    fn detects_sei_payload_type() {
        let cll = ContentLightLevel {
            max_cll: 1000,
            max_fall: 400,
        };
        let md_only = build_hdr10_sei_rbsp(Some(&sample_md()), None).unwrap();
        let both = build_hdr10_sei_rbsp(Some(&sample_md()), Some(&cll)).unwrap();
        let cll_only = build_hdr10_sei_rbsp(None, Some(&cll)).unwrap();

        // Mastering-display (137) present alone and alongside CLL; absent in the CLL-only NAL.
        assert!(rbsp_has_sei_payload(&md_only, SEI_MASTERING_DISPLAY));
        assert!(rbsp_has_sei_payload(&both, SEI_MASTERING_DISPLAY));
        assert!(!rbsp_has_sei_payload(&cll_only, SEI_MASTERING_DISPLAY));
        // Content-light-level (144) — the second message — is found in both and cll-only.
        assert!(rbsp_has_sei_payload(&both, SEI_CONTENT_LIGHT_LEVEL));
        assert!(rbsp_has_sei_payload(&cll_only, SEI_CONTENT_LIGHT_LEVEL));
        assert!(!rbsp_has_sei_payload(&md_only, SEI_CONTENT_LIGHT_LEVEL));
        // A bare prefix-SEI header with only trailing bits has no messages.
        assert!(!rbsp_has_sei_payload(&[0x4E, 0x01, 0x80], SEI_MASTERING_DISPLAY));
    }
}
