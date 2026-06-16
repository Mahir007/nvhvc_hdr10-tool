//! Annex B NAL framing and emulation-prevention handling.
//!
//! An Annex B stream is a sequence of NAL units each preceded by a start code (`00 00 01`, often
//! with an extra leading `00`). Within a NAL, the byte sequence `00 00 00/01/02/03` cannot appear
//! literally, so encoders insert an *emulation-prevention three byte* (`03`) to break it up:
//! `00 00 03 00/01/02/03`. Decoding RBSP means removing those `03` bytes; encoding RBSP back to
//! the wire format means re-inserting them.
//!
//! The original Python tool used a blanket `00 00 -> 00 00 03` replacement, which inserts spurious
//! bytes. The functions here implement the spec-correct rule and round-trip exactly.

/// `nal_unit_type` for a Video Parameter Set.
pub const NAL_VPS: u8 = 32;
/// `nal_unit_type` for a Sequence Parameter Set.
pub const NAL_SPS: u8 = 33;
/// `nal_unit_type` for a Picture Parameter Set.
pub const NAL_PPS: u8 = 34;
/// `nal_unit_type` for a prefix SEI (precedes the picture it describes).
pub const NAL_PREFIX_SEI: u8 = 39;
/// `nal_unit_type` for a suffix SEI.
pub const NAL_SUFFIX_SEI: u8 = 40;

/// A NAL unit located within an Annex B buffer. All offsets index the original buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nal {
    /// Offset of the first byte of this unit's start code.
    pub start_code: usize,
    /// Offset of the NAL header (first byte after the start code).
    pub header: usize,
    /// Exclusive end of this unit (the next unit's `start_code`, or the buffer length).
    pub end: usize,
    /// `nal_unit_type` extracted from the header (bits 1..=6 of the first header byte).
    pub nal_type: u8,
}

impl Nal {
    /// Video Coding Layer NALs (slice data) have types 0..=31.
    pub fn is_vcl(&self) -> bool {
        self.nal_type <= 31
    }

    /// The on-wire bytes of this NAL: the 2-byte header plus payload, excluding the start code.
    pub fn raw<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        &buf[self.header..self.end]
    }
}

/// Locates every NAL unit in an Annex B buffer, in order.
///
/// Start codes are detected as `00 00 01`; an immediately preceding `00` is folded into the start
/// code so the reported `end` of the previous NAL excludes it.
pub fn find_nals(buf: &[u8]) -> Vec<Nal> {
    // (start_code_offset, header_offset) for each start code.
    let mut codes: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i + 3 <= buf.len() {
        if buf[i] == 0 && buf[i + 1] == 0 && buf[i + 2] == 1 {
            let mut sc = i;
            if sc > 0 && buf[sc - 1] == 0 {
                sc -= 1; // four-byte start code 00 00 00 01
            }
            codes.push((sc, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }

    let mut nals = Vec::with_capacity(codes.len());
    for (idx, &(start_code, header)) in codes.iter().enumerate() {
        let end = codes
            .get(idx + 1)
            .map_or(buf.len(), |&(next_sc, _)| next_sc);
        let nal_type = buf.get(header).map_or(0, |b| (b >> 1) & 0x3F);
        nals.push(Nal {
            start_code,
            header,
            end,
            nal_type,
        });
    }
    nals
}

/// Removes emulation-prevention bytes, turning on-wire NAL bytes into RBSP.
///
/// A `03` is dropped when it directly follows `00 00` and is itself followed by a byte `<= 03`
/// (or sits at the end of the unit). This is the exact inverse of [`add_emulation_prevention`].
pub fn remove_emulation_prevention(nal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nal.len());
    let mut zeros = 0u32; // consecutive 0x00 already pushed to `out`
    let mut i = 0usize;
    while i < nal.len() {
        let b = nal[i];
        if zeros >= 2 && b == 0x03 && nal.get(i + 1).is_none_or(|&n| n <= 0x03) {
            // Emulation-prevention byte: skip it and reset the run.
            zeros = 0;
            i += 1;
            continue;
        }
        out.push(b);
        if b == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
        i += 1;
    }
    out
}

/// Inserts emulation-prevention bytes, turning RBSP into on-wire NAL bytes.
///
/// After any `00 00`, if the next byte is `<= 03`, a `03` is inserted first. This guarantees the
/// forbidden sequences `00 00 00/01/02/03` never appear in the output.
pub fn add_emulation_prevention(rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len() + rbsp.len() / 64 + 1);
    let mut zeros = 0u32; // consecutive trailing 0x00 in `out`
    for &b in rbsp {
        if zeros == 2 && b <= 0x03 {
            out.push(0x03);
            zeros = 0;
        }
        out.push(b);
        if b == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_three_and_four_byte_start_codes() {
        // VPS (4-byte sc) | SPS (3-byte sc) | a VCL slice (4-byte sc).
        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x40, 0x01, 0xAA]); // type 32 (VPS)
        buf.extend_from_slice(&[0x00, 0x00, 0x01, 0x42, 0x01, 0xBB]); // type 33 (SPS)
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x26, 0x01, 0xCC]); // type 19 (IDR, VCL)

        let nals = find_nals(&buf);
        assert_eq!(nals.len(), 3);
        assert_eq!(nals[0].nal_type, NAL_VPS);
        assert_eq!(nals[1].nal_type, NAL_SPS);
        assert!(nals[2].is_vcl());
        // The SPS payload is header(2) + 0xBB.
        assert_eq!(nals[1].raw(&buf), &[0x42, 0x01, 0xBB]);
    }

    #[test]
    fn emulation_prevention_roundtrip() {
        let rbsp = vec![
            0x00, 0x00, 0x00, // -> 00 00 03 00
            0x00, 0x00, 0x01, // -> 00 00 03 01
            0x00, 0x00, 0x02, // -> 00 00 03 02
            0x00, 0x00, 0x03, // -> 00 00 03 03
            0xFF, 0x00, 0x00, 0xAB, // no insertion (AB > 03)
            0x00, 0x00, // trailing zeros, no following byte
        ];
        let wire = add_emulation_prevention(&rbsp);
        // Each of the first four triples gains a 03.
        assert!(wire.windows(4).any(|w| w == [0x00, 0x00, 0x03, 0x00]));
        assert_eq!(remove_emulation_prevention(&wire), rbsp);
    }

    #[test]
    fn add_does_not_touch_safe_sequences() {
        let rbsp = vec![0x12, 0x34, 0x00, 0x56, 0x00, 0x78];
        assert_eq!(add_emulation_prevention(&rbsp), rbsp);
    }

    #[test]
    fn remove_handles_consecutive_emulation_bytes() {
        // 00 00 03 03 03 -> after removing the first EPB: 00 00 03 03.
        let wire = vec![0x00, 0x00, 0x03, 0x03, 0x03];
        let rbsp = remove_emulation_prevention(&wire);
        // Re-encoding must reproduce the wire form exactly.
        assert_eq!(add_emulation_prevention(&rbsp), wire);
    }
}
