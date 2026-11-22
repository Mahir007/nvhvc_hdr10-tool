//! By default this stages a self-contained Windows App SDK runtime next to the exe, so the app
//! runs without a system-wide runtime install. `NVHVC_WINUI_RUNTIME=framework` builds the exe for
//! the MSIX package instead: the package declares the runtime framework it needs, and Windows
//! provides it.
fn main() {
    println!("cargo:rerun-if-env-changed=NVHVC_WINUI_RUNTIME");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    match std::env::var("NVHVC_WINUI_RUNTIME").as_deref() {
        Ok("framework") => {}
        Ok("self-contained") | Err(_) => windows_reactor_setup::as_self_contained(),
        Ok(other) => panic!("NVHVC_WINUI_RUNTIME must be framework or self-contained, not {other}"),
    }
}
