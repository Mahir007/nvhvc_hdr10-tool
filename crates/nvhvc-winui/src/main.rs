//! WinUI3 GUI (windows-reactor) for the nvhvc HDR10 HEVC patcher.
//!
//! Mirrors the CLI/GTK flow: pick a source, an encode and an output, then patch. The pipeline runs
//! on a background thread via `use_mutation`, so the UI stays responsive.

use windows_reactor::*;

use nvhvc_core::pipeline::{self, PipelineOptions};
use nvhvc_core::tools::{Overrides, Resolution, Toolset};

fn app(cx: &mut RenderCx) -> Element {
    let (source, set_source) = cx.use_state(String::new());
    let (encoded, set_encoded) = cx.use_state(String::new());
    let (output, set_output) = cx.use_state(String::new());
    let (patch_state, patch_trigger) = cx.use_mutation::<String>();

    let on_source = move |v: String| set_source.call(v);
    let on_encoded = move |v: String| set_encoded.call(v);
    let on_output = move |v: String| set_output.call(v);

    let on_patch = {
        let trigger = patch_trigger;
        let s = source.clone();
        let e = encoded.clone();
        let o = output.clone();
        move || {
            let (s, e, o) = (s.clone(), e.clone(), o.clone());
            trigger.fire(move || run_patch(&s, &e, &o));
        }
    };

    let status: Element = match &patch_state {
        MutationState::Idle => text_block("Ready.").into(),
        MutationState::Loading => ProgressRing::indeterminate().into(),
        MutationState::Success(path) => text_block(format!("✓ Wrote {path}")).into(),
        MutationState::Error(e) => text_block(format!("Error: {e}")).into(),
    };

    vstack((
        text_block("nvhvc — HDR10 patcher").bold().font_size(24.0),
        text_box(source)
            .header("Source (HDR10 master)")
            .on_text_changed(on_source),
        text_box(encoded)
            .header("Encode to patch")
            .on_text_changed(on_encoded),
        text_box(output)
            .header("Output (.mkv)")
            .on_text_changed(on_output),
        tool_status(),
        button("Patch HDR10")
            .enabled(!patch_state.is_loading())
            .on_click(on_patch),
        status,
    ))
    .spacing(12.0)
    .into()
}

/// A small group of rows showing each external tool with a coloured status dot:
/// green = on PATH / custom, blue = found at a fallback install dir, red = missing.
fn tool_status() -> Element {
    let tools = Toolset::discover(&Overrides::default());
    let rows: Vec<Element> = tools
        .all()
        .into_iter()
        .map(|(tool, resolution)| {
            let (dot, detail) = match resolution {
                Resolution::OnPath(p) => (
                    Color::rgb(51, 209, 122),
                    format!("on PATH — {}", p.display()),
                ),
                Resolution::Override(p) => (
                    Color::rgb(51, 209, 122),
                    format!("custom — {}", p.display()),
                ),
                Resolution::Fallback(p) => (
                    Color::rgb(98, 160, 234),
                    format!("install dir — {}", p.display()),
                ),
                Resolution::NotFound => (
                    Color::rgb(224, 27, 36),
                    format!("not found — {}", tool.install_hint()),
                ),
            };
            hstack((
                text_block("●").foreground(dot),
                text_block(format!("{} · {}", tool.label(), detail)),
            ))
            .spacing(8.0)
            .into()
        })
        .collect();
    vstack(rows).spacing(4.0).into()
}

/// Runs the full pipeline; returns the output path on success or an error message.
fn run_patch(source: &str, encoded: &str, output: &str) -> std::result::Result<String, String> {
    if source.is_empty() || encoded.is_empty() || output.is_empty() {
        return Err("Fill in source, encode and output paths.".into());
    }
    let tools = Toolset::discover(&Overrides::default());
    let report = pipeline::run(
        &tools,
        source.as_ref(),
        encoded.as_ref(),
        output.as_ref(),
        &PipelineOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    Ok(report.output.display().to_string())
}

fn main() -> Result<()> {
    App::new().title("nvhvc — HDR10 patcher").render(app)
}
