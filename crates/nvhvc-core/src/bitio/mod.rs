//! MSB-first bit I/O with Exp-Golomb coding.
//!
//! HEVC RBSP data is a big-endian bit stream: the first bit of the stream is the most significant
//! bit of the first byte. [`BitReader`] and [`BitWriter`] operate on that convention. They work on
//! *RBSP* bytes — i.e. after emulation-prevention bytes have been removed (see
//! [`crate::hevc::nal`]).
//!
//! A [`BitBuf`] is an owned run of bits of an exact (not necessarily byte-aligned) length. It lets
//! us lift an untouched region of a parameter set out as raw bits and splice it back in at a
//! different bit offset, which is the heart of the surgical SPS edit.

mod reader;
mod writer;

pub use reader::BitReader;
pub use writer::BitWriter;

/// An owned sequence of bits with an exact length.
///
/// Bits are packed MSB-first into `bytes`; only the first `bit_len` bits are meaningful and any
/// padding in the final byte is zero.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitBuf {
    /// Backing bytes, MSB-first. `bytes.len() == bit_len.div_ceil(8)`.
    pub bytes: Vec<u8>,
    /// Number of meaningful bits.
    pub bit_len: usize,
}

impl BitBuf {
    /// Returns `true` if there are no bits.
    pub fn is_empty(&self) -> bool {
        self.bit_len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_fixed_bits_roundtrip() {
        let mut w = BitWriter::new();
        w.write_bits(0b101, 3);
        w.write_bits(0xABCD, 16);
        w.write_bit(1);
        w.write_flag(false);
        let buf = w.into_bitbuf();
        assert_eq!(buf.bit_len, 3 + 16 + 1 + 1);

        let mut r = BitReader::new(&buf.bytes);
        assert_eq!(r.read_bits(3).unwrap(), 0b101);
        assert_eq!(r.read_bits(16).unwrap(), 0xABCD);
        assert_eq!(r.read_bit().unwrap(), 1);
        assert!(!r.read_flag().unwrap());
    }

    #[test]
    fn ue_roundtrip_small_values() {
        for v in 0u64..=4096 {
            let mut w = BitWriter::new();
            w.write_ue(v);
            let buf = w.into_bitbuf();
            let mut r = BitReader::new(&buf.bytes);
            assert_eq!(r.read_ue().unwrap(), v, "ue roundtrip failed for {v}");
        }
    }

    #[test]
    fn ue_known_codewords() {
        // From the H.264/HEVC spec: 0 -> "1", 1 -> "010", 2 -> "011", 3 -> "00100".
        let cases: &[(u64, &[u8], usize)] = &[
            (0, &[0b1000_0000], 1),
            (1, &[0b0100_0000], 3),
            (2, &[0b0110_0000], 3),
            (3, &[0b0010_0000], 5),
            (4, &[0b0010_1000], 5),
        ];
        for &(value, bytes, bit_len) in cases {
            let mut w = BitWriter::new();
            w.write_ue(value);
            let buf = w.into_bitbuf();
            assert_eq!(buf.bit_len, bit_len, "wrong length for ue({value})");
            assert_eq!(buf.bytes, bytes, "wrong codeword for ue({value})");
        }
    }

    #[test]
    fn se_roundtrip() {
        for v in -2048i64..=2048 {
            let mut w = BitWriter::new();
            w.write_se(v);
            let buf = w.into_bitbuf();
            let mut r = BitReader::new(&buf.bytes);
            assert_eq!(r.read_se().unwrap(), v, "se roundtrip failed for {v}");
        }
    }

    #[test]
    fn se_mapping_matches_spec() {
        // se(v) maps code_num 0,1,2,3,4 -> 0, 1, -1, 2, -2.
        let expect: &[(i64, u64)] = &[(0, 0), (1, 1), (-1, 2), (2, 3), (-2, 4)];
        for &(value, code_num) in expect {
            let mut w = BitWriter::new();
            w.write_se(value);
            let se_buf = w.into_bitbuf();

            let mut w2 = BitWriter::new();
            w2.write_ue(code_num);
            let ue_buf = w2.into_bitbuf();

            assert_eq!(se_buf, ue_buf, "se({value}) should equal ue({code_num})");
        }
    }

    #[test]
    fn bitbuf_copy_roundtrip_across_alignment() {
        // Build a payload, slice out a non-byte-aligned middle region, then reassemble it at a
        // different offset and confirm the region's bits are preserved.
        let mut w = BitWriter::new();
        w.write_bits(0b11010, 5); // prefix
        w.write_bits(0xDEAD_BEEF, 32); // region of interest
        w.write_bits(0b1, 1); // suffix marker
        let original = w.into_bitbuf();

        let mut r = BitReader::new(&original.bytes);
        let _prefix = r.read_bitbuf(5).unwrap();
        let region = r.read_bitbuf(32).unwrap();
        assert_eq!(region.bit_len, 32);

        // Reassemble with a *different* prefix length to force a different alignment.
        let mut w2 = BitWriter::new();
        w2.write_bits(0b1, 1); // shorter prefix -> region now starts at bit 1
        w2.append_bitbuf(&region);
        let reassembled = w2.into_bitbuf();

        let mut r2 = BitReader::new(&reassembled.bytes);
        assert_eq!(r2.read_bit().unwrap(), 1);
        assert_eq!(r2.read_bits(32).unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn eof_is_reported() {
        let data = [0xFFu8];
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_bits(8).unwrap(), 0xFF);
        assert!(matches!(
            r.read_bit(),
            Err(crate::error::Error::UnexpectedEof { .. })
        ));
    }
}
