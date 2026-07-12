//! Developer utility: inject a fixed HDR10 metadata set into a raw HEVC file.
//!
//! Usage: `cargo run -p nvhvc-core --example patch_file -- in.hevc out.hevc`

use nvhvc_core::hevc::patch::{Hdr10Metadata, patch_stream};
use nvhvc_core::hevc::sei::{ContentLightLevel, MasteringDisplay};
use nvhvc_core::hevc::sps::VuiColorEdit;

fn main() {
    let mut args = std::env::args().skip(1);
    let input_path = args.next().expect("usage: patch_file <in.hevc> <out.hevc>");
    let output_path = args.next().expect("usage: patch_file <in.hevc> <out.hevc>");

    let input = std::fs::read(&input_path).expect("read input");
    let meta = Hdr10Metadata {
        color: VuiColorEdit {
            video_format: 5,
            video_full_range: false,
            colour_primaries: 9,          // BT.2020
            transfer_characteristics: 16, // SMPTE ST 2084 (PQ)
            matrix_coeffs: 9,             // BT.2020 NCL
            chroma_loc: Some(2),
        },
        mastering_display: Some(MasteringDisplay {
            primaries_x: [13250, 7500, 34000],
            primaries_y: [34500, 3000, 16000],
            white_point_x: 15635,
            white_point_y: 16450,
            max_luminance: 10_000_000,
            min_luminance: 1,
        }),
        content_light_level: Some(ContentLightLevel {
            max_cll: 1000,
            max_fall: 400,
        }),
    };

    let (out, report) = patch_stream(&input, &meta).expect("patch stream");
    std::fs::write(&output_path, &out).expect("write output");
    eprintln!(
        "patched {} SPS, sei_insertions={}, {} -> {} bytes",
        report.sps_patched,
        report.sei_insertions,
        input.len(),
        out.len()
    );
}
