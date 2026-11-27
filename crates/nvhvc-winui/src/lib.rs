//! WinUI 3 app (windows-reactor) for the nvhvc HDR10 HEVC patcher.
//!
//! A three-stage wizard styled after the Fluent design mockup: pick the HDR10 **source**
//! (with editable detected metadata), pick the **encode** to patch and the output, run the
//! pipeline with staged progress, and review the **result**. Mica backdrop, `TitleBar` with a
//! themed app menu, and a Preferences overlay for custom tool paths.
//!
//! `nvhvc.exe` (the `nvhvc` crate) opens it with `nvhvc gui`, or when it starts outside a
//! terminal. On other platforms this crate is empty.
#![cfg(windows)]

mod app;
mod model;
mod palette;
mod platform;
mod runtime;
mod views;

pub use platform::started_outside_terminal;
pub use runtime::runtime_installed;

/// Opens the app's window and returns when it closes.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    windows_reactor::App::run_component::<app::Patcher>(())?;
    Ok(())
}
