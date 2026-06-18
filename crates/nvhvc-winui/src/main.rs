//! WinUI3 GUI (windows-reactor) for the nvhvc HDR10 HEVC patcher.
//!
//! Mirrors the CLI/GTK flow: pick a source, an encode and an output, then patch. The pipeline runs
//! on a background thread via `use_mutation`, so the UI stays responsive. Paths can be typed,
//! dragged-and-dropped onto a field, or chosen with the Browse buttons.

use windows_reactor::*;

use nvhvc_core::pipeline::{self, PipelineOptions};
use nvhvc_core::tools::{Overrides, Resolution, Toolset};

fn app(cx: &mut RenderCx) -> Element {
    let (source, set_source) = cx.use_async_state(String::new());
    let (encoded, set_encoded) = cx.use_async_state(String::new());
    let (output, set_output) = cx.use_async_state(String::new());
    let (patch_state, patch_trigger) = cx.use_mutation::<String>();

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
        text_block("Drag a file onto a field, type a path, or use Browse…").font_size(12.0),
        file_field("Source (HDR10 master)", source.clone(), set_source, false),
        file_field("Encode to patch", encoded.clone(), set_encoded, false),
        file_field("Output (.mkv)", output.clone(), set_output, true),
        tool_status(),
        button("Patch HDR10")
            .enabled(!patch_state.is_loading())
            .on_click(on_patch),
        status,
    ))
    .spacing(12.0)
    .into()
}

/// A path text box that accepts a typed path, a dragged-and-dropped file, or a Browse… dialog.
fn file_field(label: &str, value: String, set: AsyncSetState<String>, save: bool) -> Element {
    let field = text_box(value)
        .header(label)
        .on_text_changed({
            let set = set.clone();
            move |v: String| set.call(v)
        })
        .allow_drop(true)
        .drag_over(accept_files)
        .drag_drop({
            let set = set.clone();
            move |ctx| match ctx.get_storage_items().first() {
                Some(item) => {
                    set.call(item.path.clone());
                    DragOperation::Link
                }
                None => DragOperation::None,
            }
        });
    let browse = button("Browse…")
        .vertical_alignment(VerticalAlignment::Bottom)
        .on_click(move || {
            if let Some(path) = pick_file(save) {
                set.call(path);
            }
        });
    hstack((field, browse)).spacing(8.0).into()
}

/// Shows a native Open (or Save) file dialog; returns the chosen path, or `None` if cancelled.
fn pick_file(save: bool) -> Option<String> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::{
        FileOpenDialog, FileSaveDialog, IFileDialog, SIGDN_FILESYSPATH,
    };

    unsafe {
        let dialog: IFileDialog = if save {
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?
        } else {
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?
        };
        // `Show` returns an error when the user cancels.
        dialog.Show(None).ok()?;
        let item = dialog.GetResult().ok()?;
        let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = pwstr.to_string().ok()?;
        CoTaskMemFree(Some(pwstr.0 as *const _));
        Some(path)
    }
}

/// Accept a drag only when it carries file(s).
fn accept_files(ctx: &mut DragContext) -> DragOperation {
    if ctx.has_storage_items {
        ctx.caption = Some("Use this file".into());
        DragOperation::Link
    } else {
        DragOperation::None
    }
}

/// A small group of rows showing each external tool with a coloured status dot.
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
