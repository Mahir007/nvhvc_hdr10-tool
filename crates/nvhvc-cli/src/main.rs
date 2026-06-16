//! Command-line front-end for the nvhvc HDR10 HEVC patcher.
//!
//! This is a placeholder entry point; the full `probe`/`patch`/`run` subcommands are wired up once
//! the core pipeline lands (milestone M4).

fn main() {
    println!(
        "nvhvc {} (core {}) — HEVC HDR10 stream patcher",
        env!("CARGO_PKG_VERSION"),
        nvhvc_core::VERSION,
    );
}
