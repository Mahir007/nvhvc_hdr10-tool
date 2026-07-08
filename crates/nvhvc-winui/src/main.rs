//! WinUI3 GUI (windows-reactor) for the nvhvc HDR10 HEVC patcher.
//!
//! A three-stage wizard styled after the Fluent design mockup: pick the HDR10 **source**
//! (with editable detected metadata), pick the **encode** to patch and the output, run the
//! pipeline with staged progress, and review the **result**. Mica backdrop, `TitleBar` with a
//! themed app menu, and a Preferences overlay for custom tool paths.
#![windows_subsystem = "windows"]

mod model;
mod views;

use windows_reactor::*;

use nvhvc_core::config::Settings;
use nvhvc_core::pipeline::{self, PipelineOptions, Stage as PipeStage};
use nvhvc_core::probe;
use nvhvc_core::tools::{Tool, Toolset};

use model::*;

/// Everything the view functions need: current state plus the setters/triggers to change it.
pub struct Vm {
    pub ui: Ui,
    pub upd: Updater<Ui>,
    pub source: String,
    pub set_source: AsyncSetState<String>,
    pub encode: String,
    pub set_encode: AsyncSetState<String>,
    pub output: String,
    pub set_output: AsyncSetState<String>,
    pub src_probe: MutationState<FileProbe>,
    pub src_trigger: MutationTrigger<FileProbe>,
    pub enc_probe: MutationState<FileProbe>,
    pub enc_trigger: MutationTrigger<FileProbe>,
    pub run: MutationState<RunSummary>,
    pub run_trigger: MutationTrigger<RunSummary>,
    pub progress: usize,
    pub set_progress: AsyncSetState<usize>,
    /// A file is being dragged over the current drop zone (drives its accent tint). One flag
    /// serves both zones — only one is ever on screen.
    pub drag_hover: bool,
    pub set_drag_hover: AsyncSetState<bool>,
}

fn app(cx: &mut RenderCx) -> Element {
    let (ui, upd) = cx.use_reducer(initial_ui());
    let (source, set_source) = cx.use_async_state(String::new());
    let (encode, set_encode) = cx.use_async_state(String::new());
    let (output, set_output) = cx.use_async_state(String::new());
    let (src_probe, src_trigger) = cx.use_mutation::<FileProbe>();
    let (enc_probe, enc_trigger) = cx.use_mutation::<FileProbe>();
    let (run, run_trigger) = cx.use_mutation::<RunSummary>();
    let (progress, set_progress) = cx.use_async_state(0usize);
    let (drag_hover, set_drag_hover) = cx.use_async_state(false);

    // Mica occasionally fails to attach at window creation (the host swallows the error), and
    // the persisted theme can't be applied before the window exists — one post-first-render
    // effect settles both.
    cx.use_effect((), || {
        set_backdrop(Some(Backdrop::Mica));
        views::apply_startup_theme();
    });

    let vm = Vm {
        ui,
        upd,
        source,
        set_source,
        encode,
        set_encode,
        output,
        set_output,
        src_probe,
        src_trigger,
        enc_probe,
        enc_trigger,
        run,
        run_trigger,
        progress,
        set_progress,
        drag_hover,
        set_drag_hover,
    };

    // The stage to display: a fresh successful run shows the result screen.
    let current_summary = match &vm.run {
        MutationState::Success(s) if s.token == vm.ui.run_token && s.token > vm.ui.ack_token => {
            Some(s.clone())
        }
        _ => None,
    };
    let processing = vm.run.is_loading();
    let shown = if vm.ui.stage == Stage::Encode && current_summary.is_some() {
        Stage::Result
    } else {
        vm.ui.stage
    };

    let (content, footer): (Element, Element) = match shown {
        Stage::Source => views::stage_source(&vm),
        Stage::Encode => {
            let (c, f) = views::stage_encode(&vm, processing);
            (c, f.unwrap_or_else(|| text_block("").into()))
        }
        Stage::Result => {
            let summary = current_summary.expect("result stage implies a current summary");
            views::stage_result(&vm, &summary)
        }
    };

    // Stage content and footer are keyed per stage so switching stages swaps the whole
    // subtree instead of morphing one stage's controls into the other's (the reconciler
    // matches keyed children by identity, not position).
    let stage_key = match (shown, processing) {
        (Stage::Encode, true) => "processing",
        (Stage::Source, _) => "source",
        (Stage::Encode, _) => "encode",
        (Stage::Result, _) => "result",
    };
    let main = grid((
        views::title_bar(&vm, shown, processing)
            .grid_row(0)
            .with_key("titlebar"),
        views::breadcrumb(shown).grid_row(1).with_key("crumbs"),
        content.grid_row(2).with_key(format!("content-{stage_key}")),
        footer.grid_row(3).with_key(format!("footer-{stage_key}")),
    ))
    .rows([
        GridLength::Auto,
        GridLength::Auto,
        GridLength::Star(1.0),
        GridLength::Auto,
    ])
    .columns([GridLength::Star(1.0)]);

    // Overlay layers stack in the same grid cell (later children render on top). The
    // always-mounted About ContentDialog is pushed BEFORE any conditional Border overlay:
    // a ContentDialog isn't a visual child, and removing a keyed Border that sits *before*
    // it left the Border stuck on screen (the reconciler's child mirror desynced). Keeping
    // every conditional visual overlay strictly last makes its removal a clean truncation.
    let mut layers: Vec<Element> = vec![main.with_key("app").into()];
    {
        let upd = vm.upd.clone();
        layers.push(
            ContentDialog::new("About nvhvc")
                .with_key("about")
                // Manual line breaks: the dialog's content TextBlock doesn't wrap (and the
                // reactor ContentDialog only takes a string), so long lines get clipped.
                .content(format!(
                    "nvhvc HDR10 patcher {}\n\nInjects HDR10 metadata into HEVC streams\n\
                     without re-encoding — bit-exact SPS/VUI\nediting with the encode's frame \
                     timing\npreserved exactly.\n\nGPL-3.0",
                    nvhvc_core::VERSION
                ))
                .close_button_text("OK")
                .is_open(vm.ui.about_open)
                .on_closed(move |_| {
                    upd.call(|mut s| {
                        s.about_open = false;
                        s
                    });
                })
                .into(),
        );
    }
    if vm.ui.prefs_open {
        layers.push(views::prefs_overlay(&vm).with_key("prefs"));
    }
    // The overwrite confirmation is a custom smoke overlay (not a ContentDialog) so the
    // destructive Replace button can be red — reactor's ContentDialog exposes no button
    // styling. A conditional overlay layer is also crash-safe (unlike unmounting a
    // ContentDialog from its own Closed callback, which aborts the WinUI backend).
    if vm.ui.confirm_overwrite {
        layers.push(views::overwrite_overlay(&vm, launch_run(&vm)).with_key("overwrite"));
    }
    // Topmost so it draws over everything, and last so its unmount is a clean truncation.
    if vm.ui.menu_open {
        layers.push(views::menu_overlay(&vm).with_key("menu"));
    }
    grid(layers).into()
}

/// Initial UI state (theme restored from settings).
fn initial_ui() -> Ui {
    Ui {
        theme: ThemeMode::from_setting(Settings::load().ui_theme.as_deref()),
        ..Ui::default()
    }
}

/// Fires an ffprobe of `path` on a worker thread.
pub fn fire_probe(trigger: &MutationTrigger<FileProbe>, path: String) {
    trigger.fire(move || {
        let settings = Settings::load();
        let tools = Toolset::discover(&settings.tool_overrides);
        let ffprobe = tools.require(Tool::Ffprobe).map_err(|e| e.to_string())?;
        let (hdr, info) = probe::probe_media(ffprobe, path.as_ref()).map_err(|e| e.to_string())?;
        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Ok(FileProbe {
            path,
            size_bytes,
            info,
            hdr,
        })
    });
}

/// Click handler for picking the source file.
pub fn pick_source_handler(vm: &Vm) -> impl Fn() + Clone + 'static {
    let set_source = vm.set_source.clone();
    let trigger = vm.src_trigger.clone();
    let upd = vm.upd.clone();
    move || {
        if let Some(path) = pick_file(false) {
            set_source.call(path.clone());
            // A new source invalidates any edits made against the old one.
            upd.call(|mut s| {
                s.overrides = None;
                s.editing = None;
                s
            });
            fire_probe(&trigger, path);
        }
    }
}

/// Click handler for picking the encode file (also derives the default output path).
pub fn pick_encode_handler(vm: &Vm) -> impl Fn() + Clone + 'static {
    let set_encode = vm.set_encode.clone();
    let set_output = vm.set_output.clone();
    let trigger = vm.enc_trigger.clone();
    move || {
        if let Some(path) = pick_file(false) {
            set_encode.call(path.clone());
            set_output.call(default_output(&path));
            fire_probe(&trigger, path);
        }
    }
}

/// Click handler for choosing the output path (save dialog).
pub fn pick_output_handler(vm: &Vm) -> impl Fn() + Clone + 'static {
    let set_output = vm.set_output.clone();
    let upd = vm.upd.clone();
    move || {
        if let Some(path) = pick_file(true) {
            set_output.call(path.clone());
            // The Win32 save dialog already warned about an existing file.
            upd.call(move |mut s| {
                s.output_chosen_for = Some(path);
                s
            });
        }
    }
}

/// Click handler for the Patch button: asks about overwriting an existing output file
/// the user didn't pick themselves, then launches.
pub fn run_handler(vm: &Vm) -> impl Fn() + Clone + 'static {
    let launch = launch_run(vm);
    let output = vm.output.clone();
    let user_chosen = vm.ui.output_chosen_for.as_deref() == Some(vm.output.as_str());
    let upd = vm.upd.clone();
    move || {
        if !user_chosen && std::path::Path::new(&output).exists() {
            upd.call(|mut s| {
                s.confirm_overwrite = true;
                s
            });
        } else {
            launch();
        }
    }
}

/// Launches the pipeline on a worker thread with staged progress.
pub fn launch_run(vm: &Vm) -> impl Fn() + Clone + 'static {
    let (source, encode, output) = (vm.source.clone(), vm.encode.clone(), vm.output.clone());
    let metadata_override = vm.ui.override_for(&vm.source);
    let token = vm.ui.run_token + 1;
    let upd = vm.upd.clone();
    let trigger = vm.run_trigger.clone();
    let set_progress = vm.set_progress.clone();
    move || {
        // Clear the overwrite prompt in the SAME update that starts the run — separate
        // updater calls in one event don't chain, so setting it in the tap handler and the
        // run_token here would clobber each other and leave the overlay stuck.
        upd.call(move |mut s| {
            s.run_token = token;
            s.confirm_overwrite = false;
            s
        });
        set_progress.call(0);
        let (source, encode, output) = (source.clone(), encode.clone(), output.clone());
        let set_progress = set_progress.clone();
        trigger.fire(move || {
            let settings = Settings::load();
            let tools = Toolset::discover(&settings.tool_overrides);
            let opts = PipelineOptions {
                chroma_loc: settings.default_chroma_loc,
                keep_intermediates: false,
                metadata_override,
            };
            let report = pipeline::run_with_progress(
                &tools,
                source.as_ref(),
                encode.as_ref(),
                output.as_ref(),
                &opts,
                &mut |stage| set_progress.call(stage_index(stage)),
            )
            .map_err(|e| e.to_string())?;
            Ok(RunSummary {
                token,
                output: report.output.display().to_string(),
                sps_patched: report.sps_patched,
                sei_inserted: report.sei_inserted,
                frames: report.frames,
                tracks: report.tracks.as_ref().map(nvhvc_core::uimodel::tracks_line),
                used_ffmpeg_fallback: report.used_ffmpeg_fallback,
            })
        });
    }
}

/// Maps a pipeline stage to the 0..=5 progress index shown in the processing view.
fn stage_index(stage: PipeStage) -> usize {
    match stage {
        PipeStage::Probing => 0,
        PipeStage::Extracting => 1,
        PipeStage::Patching => 2,
        PipeStage::CapturingTiming => 3,
        PipeStage::Remuxing => 4,
        PipeStage::Done => 5,
    }
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
pub fn accept_files(ctx: &mut DragContext) -> DragOperation {
    if ctx.has_storage_items {
        ctx.caption = Some("Use this file".into());
        DragOperation::Link
    } else {
        DragOperation::None
    }
}

/// Opens a file with its default handler (e.g. plays the output video).
pub fn open_path(path: &str) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

/// Opens Explorer with the file selected.
pub fn reveal_in_folder(path: &str) {
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn();
}

fn main() -> Result<()> {
    // Restore the persisted theme before the window shows (queued until the root attaches).
    views::apply_startup_theme();
    App::new()
        .title("HDR10 Patcher")
        .inner_size(780.0, 760.0)
        .backdrop(Backdrop::Mica)
        .render(app)
}
