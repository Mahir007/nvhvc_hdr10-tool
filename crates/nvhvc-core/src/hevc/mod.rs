//! HEVC (H.265) bitstream handling.
//!
//! [`nal`] provides Annex B framing: locating NAL units within a byte buffer and converting
//! between the on-wire form (with emulation-prevention bytes) and RBSP.

pub mod nal;
pub mod patch;
pub mod sei;
pub mod sps;
