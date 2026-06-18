//! Stages a self-contained Windows App SDK Runtime next to the exe so the app runs without a
//! system-wide runtime install.
//!
//! Why not framework-dependent (which would be far slimmer)? windows-reactor is hard-pinned to
//! WinApp SDK **2.1.3**, and its bootstrap binds to the exact 2.1 framework family. Machines with a
//! different 2.x runtime (e.g. 2.2.x — the 2.x family is single/auto-updating) can't satisfy it, so
//! framework-dependent fails to launch with `0x80040154`. The proper slim build belongs to the
//! eventual MSIX packaging, where the package declares the 2.1 dependency and Windows installs the
//! exact matching runtime automatically. Until then, self-contained "just works" everywhere.
fn main() {
    windows_reactor_setup::as_self_contained();
}
