//! MSB-first bit writer producing RBSP bytes.

use crate::bitio::{BitBuf, BitReader};

/// Accumulates bits most-significant-first into a byte buffer.
#[derive(Debug, Clone, Default)]
pub struct BitWriter {
    bytes: Vec<u8>,
    /// Bits accumulated in `current` so far, in `0..8`.
    nbits: u8,
    /// The partially filled byte, left-justified as it fills.
    current: u8,
}

impl BitWriter {
    /// Creates an empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of bits written so far.
    pub fn bit_len(&self) -> usize {
        self.bytes.len() * 8 + self.nbits as usize
    }

    /// Whether the next bit would start on a byte boundary.
    pub fn byte_aligned(&self) -> bool {
        self.nbits == 0
    }

    /// Appends a single bit (only the least significant bit of `bit` is used).
    pub fn write_bit(&mut self, bit: u8) {
        self.current = (self.current << 1) | (bit & 1);
        self.nbits += 1;
        if self.nbits == 8 {
            self.bytes.push(self.current);
            self.current = 0;
            self.nbits = 0;
        }
    }

    /// Appends the low `n` bits (0..=64) of `value`, MSB-first.
    pub fn write_bits(&mut self, value: u64, n: u32) {
        debug_assert!(n <= 64, "write_bits supports at most 64 bits");
        for i in (0..n).rev() {
            self.write_bit(((value >> i) & 1) as u8);
        }
    }

    /// Appends a flag bit.
    pub fn write_flag(&mut self, flag: bool) {
        self.write_bit(u8::from(flag));
    }

    /// Appends an unsigned Exp-Golomb code, `ue(v)`.
    pub fn write_ue(&mut self, value: u64) {
        // Codeword for `value` is the minimal binary of `value + 1`, prefixed with one fewer
        // zeros than its bit length.
        let v = value + 1;
        let significant_bits = 64 - v.leading_zeros();
        let prefix_zeros = significant_bits - 1;
        for _ in 0..prefix_zeros {
            self.write_bit(0);
        }
        self.write_bits(v, significant_bits);
    }

    /// Appends an unsigned Exp-Golomb code from a `u32`.
    pub fn write_ue32(&mut self, value: u32) {
        self.write_ue(u64::from(value));
    }

    /// Appends a signed Exp-Golomb code, `se(v)`.
    pub fn write_se(&mut self, value: i64) {
        // Inverse of the reader mapping: 0 -> 0, 1 -> 1, -1 -> 2, 2 -> 3, -2 -> 4, ...
        let code_num = if value > 0 {
            (2 * value - 1) as u64
        } else {
            (-2 * value) as u64
        };
        self.write_ue(code_num);
    }

    /// Appends every bit of `buf` (an exact-length run of bits).
    pub fn append_bitbuf(&mut self, buf: &BitBuf) {
        let mut r = BitReader::new(&buf.bytes);
        for _ in 0..buf.bit_len {
            let bit = r.read_bit().expect("bit within bitbuf bounds");
            self.write_bit(bit);
        }
    }

    /// Pads with zero bits until byte-aligned (a no-op if already aligned).
    pub fn align_to_byte(&mut self) {
        while self.nbits != 0 {
            self.write_bit(0);
        }
    }

    /// Finalises into a [`BitBuf`] preserving the exact bit length; any trailing partial byte is
    /// zero-padded on the right.
    pub fn into_bitbuf(self) -> BitBuf {
        let bit_len = self.bit_len();
        let mut bytes = self.bytes;
        if self.nbits > 0 {
            bytes.push(self.current << (8 - self.nbits));
        }
        BitBuf { bytes, bit_len }
    }

    /// Finalises into byte-aligned bytes, zero-padding any trailing partial byte.
    ///
    /// Note: this is raw zero padding, *not* `rbsp_trailing_bits` (which begins with a one bit).
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.align_to_byte();
        self.bytes
    }
}
