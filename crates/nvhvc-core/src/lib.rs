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

pub use error::{Error, Result};

/// The crate version, surfaced so front-ends can report a single source of truth.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
