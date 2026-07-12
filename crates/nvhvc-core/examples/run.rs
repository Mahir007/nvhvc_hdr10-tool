//! Developer utility: run the whole pipeline (probe → extract → patch → remux).
//!
//! Usage: `cargo run -p nvhvc-core --example run -- source.mkv encode.mkv out.mkv [chroma_loc]`

use nvhvc_core::pipeline::{self, PipelineOptions};
use nvhvc_core::tools::{Overrides, Toolset};

fn main() {
    let mut args = std::env::args().skip(1);
    let source = args
        .next()
        .expect("usage: run <source> <encoded> <output> [chroma_loc]");
    let encoded = args.next().expect("missing <encoded>");
    let output = args.next().expect("missing <output>");
    let chroma_loc = args.next().and_then(|s| s.parse::<u32>().ok());

    let tools = Toolset::discover(&Overrides::default());
    let report = pipeline::run(
        &tools,
        source.as_ref(),
        encoded.as_ref(),
        output.as_ref(),
        &PipelineOptions {
            chroma_loc,
            keep_intermediates: false,
            metadata_override: None,
        },
    )
    .expect("pipeline failed");

    println!(
        "OK -> {}\n  SPS patched: {}\n  SEI insertions: {}\n  primaries/transfer/matrix: {}/{}/{}\n  mastering_display: {}\n  content_light_level: {}",
        report.output.display(),
        report.sps_patched,
        report.sei_insertions,
        report.probed.color.colour_primaries,
        report.probed.color.transfer_characteristics,
        report.probed.color.matrix_coeffs,
        report.probed.mastering_display.is_some(),
        report.probed.content_light_level.is_some(),
    );
}
