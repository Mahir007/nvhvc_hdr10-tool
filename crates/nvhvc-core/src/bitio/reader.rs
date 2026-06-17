//! MSB-first bit reader over a byte slice.

use crate::bitio::{BitBuf, BitWriter};
use crate::error::{Error, Result};

/// Reads bits most-significant-first from a borrowed byte slice.
///
/// The slice is expected to be RBSP (emulation-prevention bytes already removed).
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Absolute bit position from the start of `data`.
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    /// Creates a reader positioned at the first bit of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    /// Current absolute bit position.
    pub fn position(&self) -> usize {
        self.bit_pos
    }

    /// Total number of bits in the underlying buffer.
    pub fn len_bits(&self) -> usize {
        self.data.len() * 8
    }

    /// Number of bits between the cursor and the end of the buffer.
    pub fn bits_remaining(&self) -> usize {
        self.len_bits().saturating_sub(self.bit_pos)
    }

    /// Whether the cursor sits on a byte boundary.
    pub fn byte_aligned(&self) -> bool {
        self.bit_pos.is_multiple_of(8)
    }

    /// Moves the cursor to an absolute bit position.
    pub fn seek(&mut self, bit_pos: usize) {
        self.bit_pos = bit_pos;
    }

    /// Reads a single bit as 0 or 1.
    pub fn read_bit(&mut self) -> Result<u8> {
        if self.bit_pos >= self.len_bits() {
            return Err(Error::UnexpectedEof {
                pos: self.bit_pos,
                needed: 1,
            });
        }
        let byte = self.data[self.bit_pos / 8];
        let shift = 7 - (self.bit_pos % 8);
        self.bit_pos += 1;
        Ok((byte >> shift) & 1)
    }

    /// Reads `n` bits (0..=64) MSB-first into the low bits of a `u64`.
    pub fn read_bits(&mut self, n: u32) -> Result<u64> {
        if n > 64 {
            return Err(Error::TooManyBits(n));
        }
        if n == 0 {
            return Ok(0);
        }
        if self.bits_remaining() < n as usize {
            return Err(Error::UnexpectedEof {
                pos: self.bit_pos,
                needed: n - self.bits_remaining() as u32,
            });
        }
        let mut value: u64 = 0;
        for _ in 0..n {
            let byte = self.data[self.bit_pos / 8];
            let shift = 7 - (self.bit_pos % 8);
            value = (value << 1) | u64::from((byte >> shift) & 1);
            self.bit_pos += 1;
        }
        Ok(value)
    }

    /// Reads a single flag bit as a `bool`.
    pub fn read_flag(&mut self) -> Result<bool> {
        Ok(self.read_bit()? != 0)
    }

    /// Reads an unsigned Exp-Golomb code, `ue(v)`.
    pub fn read_ue(&mut self) -> Result<u64> {
        let mut leading_zeros: u32 = 0;
        loop {
            if self.read_bit()? == 1 {
                break;
            }
            leading_zeros += 1;
            if leading_zeros >= 64 {
                return Err(Error::ExpGolombOverflow { value: u64::MAX });
            }
        }
        if leading_zeros == 0 {
            return Ok(0);
        }
        let suffix = self.read_bits(leading_zeros)?;
        Ok(((1u64 << leading_zeros) - 1) + suffix)
    }

    /// Reads an unsigned Exp-Golomb code that must fit in a `u32`.
    pub fn read_ue32(&mut self) -> Result<u32> {
        let value = self.read_ue()?;
        u32::try_from(value).map_err(|_| Error::ExpGolombOverflow { value })
    }

    /// Reads a signed Exp-Golomb code, `se(v)`.
    pub fn read_se(&mut self) -> Result<i64> {
        let code_num = self.read_ue()?;
        // 0 -> 0, 1 -> 1, 2 -> -1, 3 -> 2, 4 -> -2, ...  (magnitude == ceil(code_num / 2))
        let magnitude = code_num.div_ceil(2) as i64;
        if code_num % 2 == 1 {
            Ok(magnitude)
        } else {
            Ok(-magnitude)
        }
    }

    /// Reads exactly `n` bits and returns them as an owned [`BitBuf`].
    pub fn read_bitbuf(&mut self, n: usize) -> Result<BitBuf> {
        let mut w = BitWriter::new();
        for _ in 0..n {
            w.write_bit(self.read_bit()?);
        }
        Ok(w.into_bitbuf())
    }

    /// Reads everything from the cursor to the end of the buffer as an owned [`BitBuf`].
    pub fn read_remaining(&mut self) -> BitBuf {
        let mut w = BitWriter::new();
        while self.bit_pos < self.len_bits() {
            // Safe: bounded by len_bits().
            let bit = self.read_bit().expect("bit within bounds");
            w.write_bit(bit);
        }
        w.into_bitbuf()
    }
}
