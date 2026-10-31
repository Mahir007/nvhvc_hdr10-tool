//! WinUI 3 GUI (windows-reactor) for the nvhvc HDR10 HEVC patcher.
//!
//! A three-stage wizard styled after the Fluent design mockup: pick the HDR10 **source**
//! (with editable detected metadata), pick the **encode** to patch and the output, run the
//! pipeline with staged progress, and review the **result**. Mica backdrop, `TitleBar` with a
//! themed app menu, and a Preferences overlay for custom tool paths.
#![windows_subsystem = "windows"]

mod app;
mod model;
mod palette;
mod platform;
mod views;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_reactor::App::run_component::<app::Patcher>(())?;
    Ok(())
}
