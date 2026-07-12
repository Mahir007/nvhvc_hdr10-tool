//! Command-line front-end for the nvhvc HDR10 HEVC patcher.
//!
//! Subcommands:
//! * `tools` — show which external tools were found and where.
//! * `probe` — read and print a source's HDR10 metadata.
//! * `run`   — inject the source's HDR10 metadata into an encode (probe → extract → patch → remux).
//!
//! Custom tool paths can be given per-invocation (`--mkvmerge PATH`, …); these override both
//! auto-detection and any persisted settings.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use nvhvc_core::config::Settings;
use nvhvc_core::naming;
use nvhvc_core::pipeline::{self, PipelineOptions};
use nvhvc_core::probe::{ProbedHdr10, probe_hdr10};
use nvhvc_core::tools::{Overrides, Resolution, Tool, Toolset};

/// Inject HDR10 metadata (SPS VUI + SEI) into an HEVC encode without re-encoding.
#[derive(Parser)]
#[command(name = "nvhvc", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Custom path to ffmpeg (overrides auto-detection and settings).
    #[arg(long, value_name = "PATH", global = true)]
    ffmpeg: Option<PathBuf>,
    /// Custom path to ffprobe.
    #[arg(long, value_name = "PATH", global = true)]
    ffprobe: Option<PathBuf>,
    /// Custom path to mkvmerge.
    #[arg(long, value_name = "PATH", global = true)]
    mkvmerge: Option<PathBuf>,
    /// Custom path to mkvextract.
    #[arg(long, value_name = "PATH", global = true)]
    mkvextract: Option<PathBuf>,
    /// Custom path to MP4Box.
    #[arg(long, value_name = "PATH", global = true)]
    mp4box: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Show which external tools were found and where.
    Tools,
    /// Probe a source file and print its HDR10 metadata.
    Probe {
        /// The HDR10 source file.
        source: PathBuf,
    },
    /// Inject the source's HDR10 metadata into an encode.
    Run {
        /// HDR10 source to read metadata from.
        source: PathBuf,
        /// Encoded file (missing HDR10) to patch.
        encoded: PathBuf,
        /// Output file; the container is chosen from its extension (.mkv or .mp4).
        output: PathBuf,
        /// Force a chroma sample location (0..=5) instead of preserving the encode's.
        #[arg(long, value_name = "0-5")]
        chroma_loc: Option<u32>,
        /// Keep the intermediate extracted/patched/timecode files.
        #[arg(long)]
        keep_intermediates: bool,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let settings = Settings::load();
    let tools = Toolset::discover(&merge_overrides(&cli, &settings));

    match dispatch(&cli, &settings, &tools) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Per-invocation `--tool` flags take precedence over persisted settings.
fn merge_overrides(cli: &Cli, settings: &Settings) -> Overrides {
    let s = &settings.tool_overrides;
    Overrides {
        ffmpeg: cli.ffmpeg.clone().or_else(|| s.ffmpeg.clone()),
        ffprobe: cli.ffprobe.clone().or_else(|| s.ffprobe.clone()),
        mkvmerge: cli.mkvmerge.clone().or_else(|| s.mkvmerge.clone()),
        mkvextract: cli.mkvextract.clone().or_else(|| s.mkvextract.clone()),
        mp4box: cli.mp4box.clone().or_else(|| s.mp4box.clone()),
    }
}

fn dispatch(cli: &Cli, settings: &Settings, tools: &Toolset) -> nvhvc_core::Result<()> {
    match &cli.command {
        Command::Tools => {
            print_tools(tools);
            Ok(())
        }
        Command::Probe { source } => {
            let ffprobe = tools.require(Tool::Ffprobe)?;
            let probed = probe_hdr10(ffprobe, source)?;
            print_metadata(&probed);
            Ok(())
        }
        Command::Run {
            source,
            encoded,
            output,
            chroma_loc,
            keep_intermediates,
        } => {
            let opts = PipelineOptions {
                chroma_loc: chroma_loc.or(settings.default_chroma_loc),
                keep_intermediates: *keep_intermediates,
                metadata_override: None,
            };
            let report =
                pipeline::run_with_progress(tools, source, encoded, output, &opts, &mut |stage| {
                    println!("  - {}", stage.label())
                })?;
            println!();
            println!("wrote {}", report.output.display());
            println!("  SPS patched : {}", report.sps_patched);
            println!(
                "  SEI inserted: {} (at {} random-access point{})",
                report.sei_insertions > 0,
                report.sei_insertions,
                if report.sei_insertions == 1 { "" } else { "s" }
            );
            if !report.dropped_tracks.is_empty() {
                println!(
                    "  note        : not carried into MP4 — {}",
                    report.dropped_tracks.join(", ")
                );
            }
            print_metadata(&report.probed);
            Ok(())
        }
    }
}

fn print_tools(tools: &Toolset) {
    println!("External tools:");
    for (tool, resolution) in tools.all() {
        let (status, detail) = match resolution {
            Resolution::OnPath(p) => ("found (PATH)", p.display().to_string()),
            Resolution::Override(p) => ("found (custom)", p.display().to_string()),
            Resolution::Fallback(p) => ("found (install dir)", p.display().to_string()),
            Resolution::OverrideMissing(p) => ("MISSING (custom)", p.display().to_string()),
            Resolution::NotFound => ("MISSING", tool.install_hint().to_string()),
        };
        println!("  {:<11} {:<20} {detail}", tool.label(), status);
    }
    if let Some(path) = Settings::config_path() {
        println!("\nsettings: {}", path.display());
    }
}

fn print_metadata(p: &ProbedHdr10) {
    let c = &p.color;
    println!("HDR10 metadata:");
    println!(
        "  primaries : {} ({})",
        c.colour_primaries,
        naming::primaries_name(c.colour_primaries)
    );
    println!(
        "  transfer  : {} ({})",
        c.transfer_characteristics,
        naming::transfer_name(c.transfer_characteristics)
    );
    println!(
        "  matrix    : {} ({})",
        c.matrix_coeffs,
        naming::matrix_name(c.matrix_coeffs)
    );
    println!("  range     : {}", naming::range_name(c.video_full_range));
    if let Some(loc) = p.source_chroma_loc {
        println!("  chroma    : {}", naming::chroma_loc_name(loc));
    }
    match &p.mastering_display {
        Some(md) => {
            let gamut = naming::gamut_name(md).unwrap_or("Custom");
            println!(
                "  mastering : {gamut} — max {:.0} nits, min {:.4} nits",
                md.max_luminance as f64 / 10_000.0,
                md.min_luminance as f64 / 10_000.0,
            );
        }
        None => println!("  mastering : (none)"),
    }
    match &p.content_light_level {
        Some(cll) => println!(
            "  light lvl : MaxCLL {} / MaxFALL {}",
            cll.max_cll, cll.max_fall
        ),
        None => println!("  light lvl : (none in source)"),
    }
}
