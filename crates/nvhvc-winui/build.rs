//! Stages a self-contained Windows App SDK runtime next to the exe so the app runs without a
//! system-wide runtime install. A slim framework-dependent build belongs to the MSIX packaging,
//! where the package declares the runtime dependency and Windows installs it.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_reactor_setup::as_self_contained();
    }
}
