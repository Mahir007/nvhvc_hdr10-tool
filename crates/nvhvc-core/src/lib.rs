//! `nvhvc-core` — the engine for adding HDR10 metadata to raw HEVC streams.
//!
//! The crate is split into small, independently testable layers:
//!
//! * [`bitio`] — MSB-first bit reader/writer with Exp-Golomb (`ue`/`se`) coding. This is the
//!   foundation for bit-exact rewriting of parameter sets.
//! * [`hevc`] — Annex B NAL handling (splitting, emulation-prevention) and, building on it, the
//!   SPS edit and SEI construction used to inject HDR10 signalling.
//!
//! The guiding correctness principle is **bit-exact round-tripping**: parsing then re-serialising
//! an unmodified parameter set must reproduce the original bytes exactly. Only then do we apply a
//! surgical edit. This is the property the original Python tool lacked, which corrupted the
//! bitstream (including its timing information) and produced variable-frame-rate output.

pub mod bitio;
pub mod config;
pub mod error;
pub mod extract;
pub mod hevc;
pub mod naming;
pub mod pipeline;
pub mod probe;
pub mod remux;
pub mod timing;
pub mod tools;
pub mod uimodel;

pub use error::{Error, Result};

/// The version this copy was built as, for every front-end to report. The source says
/// `0.0.0-dev`. CI sets `NVHVC_VERSION` (`0.1.0`, `0.1.0-beta.1+abc1234`, `0.0.0-dev+abc1234`)
/// when it builds.
pub const VERSION: &str = match option_env!("NVHVC_VERSION") {
    Some(version) if !version.is_empty() => version,
    _ => env!("CARGO_PKG_VERSION"),
};
