//! Developer utility: probe a source file's HDR10 metadata via ffprobe.
//!
//! Usage: `cargo run -p nvhvc-core --example probe -- path/to/source.mkv`

use nvhvc_core::probe::probe_hdr10;
use nvhvc_core::tools::{Overrides, Tool, Toolset};

fn main() {
    let source = std::env::args().nth(1).expect("usage: probe <source>");
    let tools = Toolset::discover(&Overrides::default());
    let ffprobe = tools.require(Tool::Ffprobe).expect("ffprobe not found");
    let probed = probe_hdr10(ffprobe, std::path::Path::new(&source)).expect("probe failed");
    println!("{probed:#?}");
}
