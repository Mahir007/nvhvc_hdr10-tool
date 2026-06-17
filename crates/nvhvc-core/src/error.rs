//! Crate-wide error type.

use thiserror::Error;

/// Convenience alias for results returned throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// All failures the core engine can produce.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// A bit read ran past the end of the available data.
    #[error("unexpected end of bitstream: needed {needed} more bit(s) at bit position {pos}")]
    UnexpectedEof {
        /// Absolute bit position at which the read failed.
        pos: usize,
        /// How many additional bits were requested beyond what remained.
        needed: u32,
    },

    /// An Exp-Golomb code (or a fixed read) decoded to a value too large to represent.
    #[error("exp-golomb value {value} out of range")]
    ExpGolombOverflow {
        /// The offending decoded value (or a sentinel when the prefix itself overflowed).
        value: u64,
    },

    /// More than 64 bits were requested in a single fixed-width read/write.
    #[error("requested {0} bits in a single operation (maximum is 64)")]
    TooManyBits(u32),

    /// The buffer contained no Annex B start code, so it is not a NAL stream.
    #[error("no NAL start code (00 00 01) found in stream")]
    NoStartCode,

    /// The stream did not contain a Sequence Parameter Set NAL (type 33).
    #[error("stream contains no SPS NAL unit")]
    NoSps,

    /// The SPS was structurally invalid (parsing reached an impossible state).
    #[error("malformed SPS: {0}")]
    MalformedSps(&'static str),

    /// A required external tool could not be located.
    #[error("could not find {tool}: {hint}")]
    ToolNotFound {
        /// The tool that was being looked for (e.g. "ffprobe").
        tool: &'static str,
        /// How to fix it (install / set a custom path).
        hint: &'static str,
    },

    /// An external tool exited with a non-zero status.
    #[error("{tool} failed ({status}): {stderr}")]
    CommandFailed {
        /// The tool that failed.
        tool: &'static str,
        /// The exit status, rendered.
        status: String,
        /// Captured stderr (trimmed).
        stderr: String,
    },

    /// ffprobe output could not be understood.
    #[error("could not parse {what}: {detail}")]
    Probe {
        /// What was being parsed (e.g. "ffprobe JSON").
        what: &'static str,
        /// The specific problem.
        detail: String,
    },

    /// Underlying I/O failure (spawning a process, reading/writing files).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON deserialisation failure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
