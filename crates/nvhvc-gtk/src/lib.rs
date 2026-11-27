//! GTK4 + libadwaita app for the nvhvc HDR10 HEVC patcher.
//!
//! On Linux, the `nvhvc` binary opens it with `nvhvc gui`. On other platforms this crate is empty,
//! so the workspace still compiles everywhere.
#![cfg(target_os = "linux")]

mod app;
mod css;
mod prefs;
mod stages;
mod state;

/// Opens the app's window and returns when it closes.
pub fn run() -> std::process::ExitCode {
    app::run().into()
}
