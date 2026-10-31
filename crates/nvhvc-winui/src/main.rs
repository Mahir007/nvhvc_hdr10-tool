//! WinUI 3 GUI (windows-reactor) for the nvhvc HDR10 HEVC patcher.
//!
//! A three-stage wizard styled after the Fluent design mockup: pick the HDR10 **source**
//! (with editable detected metadata), pick the **encode** to patch and the output, run the
//! pipeline with staged progress, and review the **result**. Mica backdrop, `TitleBar` with a
//! themed app menu, and a Preferences overlay for custom tool paths.
//!
//! This is the Windows front-end. On other platforms it builds as a small stub (use the `nvhvc`
//! CLI, or the GTK GUI on Linux, there instead) so the workspace still compiles everywhere.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod model;
#[cfg(windows)]
mod palette;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
mod views;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_reactor::App::run_component::<app::Patcher>(())?;
    Ok(())
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("nvhvc-winui is the Windows GUI; on this platform use the `nvhvc` CLI.");
    std::process::ExitCode::SUCCESS
}
