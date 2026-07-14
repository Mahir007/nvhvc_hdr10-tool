//! Extracting streams from a container via ffmpeg: the raw Annex B HEVC to patch, plus the
//! per-track pieces the MP4 assembly needs (SRT subtitles, DTS cores, a cover image).
//! ffmpeg is used here purely as an extractor/converter — MP4Box does all MP4 muxing.

use std::path::Path;

use crate::error::Result;
use crate::tools::{self, Tool, path_arg};

/// Copies the first video track out of `encoded` into a raw Annex B HEVC file at `out_hevc`.
///
/// Uses stream copy (no re-encode) plus the `hevc_mp4toannexb` bitstream filter, which converts
/// the container's length-prefixed NALs (hvcC) into Annex B start-code framing.
pub fn extract_hevc(ffmpeg: &Path, encoded: &Path, out_hevc: &Path) -> Result<()> {
    tools::run(
        ffmpeg,
        Tool::Ffmpeg,
        &[
            "-y",
            "-loglevel",
            "error",
            "-i",
            path_arg(encoded)?,
            "-map",
            "0:v:0",
            "-c:v",
            "copy",
            "-bsf:v",
            "hevc_mp4toannexb",
            "-f",
            "hevc",
            path_arg(out_hevc)?,
        ],
    )
}

/// Extracts text subtitle stream `index` to an SRT file (cue times are carried exactly; ASS/SSA
/// styling is reduced to text). MP4Box then imports the SRT natively as tx3g.
pub fn extract_srt(ffmpeg: &Path, input: &Path, index: u64, out_srt: &Path) -> Result<()> {
    let map = format!("0:{index}");
    tools::run(
        ffmpeg,
        Tool::Ffmpeg,
        &[
            "-y",
            "-loglevel",
            "error",
            "-i",
            path_arg(input)?,
            "-map",
            &map,
            "-c:s",
            "srt",
            "-f",
            "srt",
            path_arg(out_srt)?,
        ],
    )
}

/// Extracts audio stream `index` through the `dca_core` bitstream filter to a raw DTS file —
/// a lossless passthrough for plain DTS, and the core substream (the only part MP4 can hold)
/// for DTS-HD MA/HRA.
pub fn extract_dts_core(ffmpeg: &Path, input: &Path, index: u64, out_dts: &Path) -> Result<()> {
    let map = format!("0:{index}");
    tools::run(
        ffmpeg,
        Tool::Ffmpeg,
        &[
            "-y",
            "-loglevel",
            "error",
            "-i",
            path_arg(input)?,
            "-map",
            &map,
            "-c:a",
            "copy",
            "-bsf:a",
            "dca_core",
            "-f",
            "dts",
            path_arg(out_dts)?,
        ],
    )
}

/// One piece of a combined extraction pass (see [`extract_many`]).
pub enum ExtractJob<'a> {
    /// [`extract_dts_core`] equivalent.
    DtsCore { index: u64, out: &'a Path },
    /// [`extract_srt`] equivalent.
    Srt { index: u64, out: &'a Path },
}

/// Extracts every job's track in a single ffmpeg invocation, so the input is read once —
/// per-track invocations re-read the whole file once per track, which dominated multi-track
/// MP4 runs. One bad track fails the whole pass: callers fall back to the per-track
/// extractors to isolate the failure.
pub fn extract_many(ffmpeg: &Path, input: &Path, jobs: &[ExtractJob]) -> Result<()> {
    if jobs.is_empty() {
        return Ok(());
    }
    let mut args: Vec<String> = ["-y", "-loglevel", "error", "-i", path_arg(input)?]
        .map(str::to_owned)
        .into();
    for job in jobs {
        match job {
            ExtractJob::DtsCore { index, out } => args.extend([
                "-map".to_owned(),
                format!("0:{index}"),
                "-c:a".to_owned(),
                "copy".to_owned(),
                "-bsf:a".to_owned(),
                "dca_core".to_owned(),
                "-f".to_owned(),
                "dts".to_owned(),
                path_arg(out)?.to_owned(),
            ]),
            ExtractJob::Srt { index, out } => args.extend([
                "-map".to_owned(),
                format!("0:{index}"),
                "-c:s".to_owned(),
                "srt".to_owned(),
                "-f".to_owned(),
                "srt".to_owned(),
                path_arg(out)?.to_owned(),
            ]),
        }
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    tools::run(ffmpeg, Tool::Ffmpeg, &args)
}

/// Extracts a Matroska file's chapters in OGM ("simple") format — exactly what MP4Box's
/// `-chap` option consumes.
pub fn extract_chapters_ogm(mkvextract: &Path, input: &Path, out_txt: &Path) -> Result<()> {
    tools::run(
        mkvextract,
        crate::tools::Tool::Mkvextract,
        &[path_arg(input)?, "chapters", "--simple", path_arg(out_txt)?],
    )
}

/// Extracts an image attachment / attached picture at stream `index` to `out_img` (for MP4
/// cover art). Tries the attachment dump first (Matroska attachments aren't packet streams),
/// then a one-frame stream copy (attached-picture streams). Returns whether a file appeared —
/// cover art is best-effort and never fails the pipeline.
pub fn extract_cover(ffmpeg: &Path, input: &Path, index: u64, out_img: &Path) -> bool {
    let dump = format!("-dump_attachment:{index}");
    // ffmpeg exits non-zero after dumping (no output file was specified) — check the file.
    let _ = tools::run(
        ffmpeg,
        Tool::Ffmpeg,
        &[
            "-y",
            "-loglevel",
            "error",
            &dump,
            &out_img.display().to_string(),
            "-i",
            &input.display().to_string(),
        ],
    );
    if out_img.exists() {
        return true;
    }
    let map = format!("0:{index}");
    tools::run(
        ffmpeg,
        Tool::Ffmpeg,
        &[
            "-y",
            "-loglevel",
            "error",
            "-i",
            &input.display().to_string(),
            "-map",
            &map,
            "-frames:v",
            "1",
            "-c",
            "copy",
            &out_img.display().to_string(),
        ],
    )
    .is_ok()
        && out_img.exists()
}
