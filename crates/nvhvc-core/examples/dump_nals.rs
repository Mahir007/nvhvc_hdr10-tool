//! Developer utility: list the NAL units in an Annex B HEVC file and print SPS bytes as hex.
//!
//! Usage: `cargo run -p nvhvc-core --example dump_nals -- path/to/stream.hevc`

use nvhvc_core::hevc::nal::{self, NAL_SPS};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: dump_nals <file.hevc>");
    let buf = std::fs::read(&path).expect("read input file");
    let nals = nal::find_nals(&buf);
    println!("{}: {} NAL unit(s)", path, nals.len());
    for (i, n) in nals.iter().enumerate() {
        let raw = n.raw(&buf);
        println!(
            "  [{i:3}] type={:2} vcl={} off={} len={}",
            n.nal_type,
            n.is_vcl() as u8,
            n.header,
            raw.len()
        );
        if n.nal_type == NAL_SPS {
            let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
            let rbsp = nal::remove_emulation_prevention(raw);
            let rbsp_hex: String = rbsp.iter().map(|b| format!("{b:02x}")).collect();
            println!("        SPS  NAL hex ({} B): {hex}", raw.len());
            println!("        SPS RBSP hex ({} B): {rbsp_hex}", rbsp.len());
        }
    }
}
