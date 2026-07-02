//! GTK4 + libadwaita GUI for the nvhvc HDR10 HEVC patcher.
//!
//! This is the Linux front-end. On non-Linux platforms it builds as a small stub (use the `nvhvc`
//! CLI, or the upcoming Windows GUI, there instead) so the workspace still compiles everywhere.

#[cfg(target_os = "linux")]
mod app;
#[cfg(target_os = "linux")]
mod css;
#[cfg(target_os = "linux")]
mod prefs;
#[cfg(target_os = "linux")]
mod stages;
#[cfg(target_os = "linux")]
mod state;

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    let _ = app::run();
    std::process::ExitCode::SUCCESS
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("nvhvc-gtk is the Linux GUI; on this platform use the `nvhvc` CLI.");
    std::process::ExitCode::SUCCESS
}
