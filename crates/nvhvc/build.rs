//! On Windows, nvhvc.exe is also the app. This embeds `nvhvc.manifest` and sets up the Windows App
//! SDK runtime the app needs. By default it stages a self-contained runtime next to the exe, so a
//! local build runs without a system-wide runtime install. `NVHVC_WINUI_RUNTIME=framework` builds
//! the exe the releases ship instead, which uses the Windows App Runtime installed on the system.
//! On other platforms this does nothing.
use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=NVHVC_WINUI_RUNTIME");
    println!("cargo:rerun-if-changed=nvhvc.manifest");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("nvhvc.manifest");
    // The linker merges this with the manifest windows-reactor-setup adds.
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );
    match env::var("NVHVC_WINUI_RUNTIME").as_deref() {
        Ok("framework") => {}
        Ok("self-contained") | Err(_) => windows_reactor_setup::as_self_contained(),
        Ok(other) => panic!("NVHVC_WINUI_RUNTIME must be framework or self-contained, not {other}"),
    }
}
