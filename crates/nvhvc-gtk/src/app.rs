//! The libadwaita application window and its wiring to the core pipeline.
//!
//! Layout (a single `AdwPreferencesPage`):
//! * **Files** — source / encode / output pickers.
//! * **Detected HDR10 metadata** — populated (with friendly names) after a source is chosen.
//! * **External tools** — one row per tool with a coloured status dot (green = on PATH / custom,
//!   blue = found at a fallback install dir, red = missing).
//! * **Run** — a button, a progress bar and a status line. The pipeline runs on a worker thread and
//!   reports [`Stage`]s back to the UI over an async channel.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use nvhvc_core::config::Settings;
use nvhvc_core::naming;
use nvhvc_core::pipeline::{self, PipelineOptions, Stage};
use nvhvc_core::probe::{self, ProbedHdr10};
use nvhvc_core::tools::{Resolution, Tool, Toolset};

const APP_ID: &str = "io.github.Mahir007.NvhvcHdr10Tool";

// GNOME palette dots for the tool-status indicator.
const DOT_GREEN: &str = "#33d17a";
const DOT_BLUE: &str = "#62a0ea";
const DOT_RED: &str = "#e01b24";

/// The three files the user picks.
#[derive(Default)]
struct Selection {
    source: Option<PathBuf>,
    encoded: Option<PathBuf>,
    output: Option<PathBuf>,
}

/// Messages from the worker thread to the UI.
enum Msg {
    Progress(Stage),
    Done(Result<PathBuf, String>),
}

/// Builds the application and runs the GTK main loop.
pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    let settings = Settings::load();
    let toolset = Toolset::discover(&settings.tool_overrides);
    let selection = Rc::new(RefCell::new(Selection::default()));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("nvhvc — HDR10 patcher")
        .default_width(720)
        .default_height(660)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());

    let page = adw::PreferencesPage::new();

    // --- Files ---
    let files = adw::PreferencesGroup::builder()
        .title("Files")
        .description("Pick the HDR10 master, the encode to patch, and where to save the result")
        .build();
    let (source_row, source_btn) = file_row("Source", "HDR10 master to copy metadata from");
    let (encoded_row, encoded_btn) = file_row("Encode", "HEVC encode missing HDR10");
    let (output_row, output_btn) = file_row("Output", "Destination .mkv");
    files.add(&source_row);
    files.add(&encoded_row);
    files.add(&output_row);
    page.add(&files);

    // --- Detected metadata ---
    let meta_group = adw::PreferencesGroup::builder()
        .title("Detected HDR10 metadata")
        .build();
    let meta_row = adw::ActionRow::builder()
        .title("—")
        .subtitle("Select a source to read its metadata")
        .build();
    meta_group.add(&meta_row);
    page.add(&meta_group);

    // --- External tools (coloured dots) ---
    let tools_group = adw::PreferencesGroup::builder()
        .title("External tools")
        .build();
    for (tool, resolution) in toolset.all() {
        let (color, detail) = tool_status(tool, resolution);
        let row = adw::ActionRow::builder()
            .title(tool.label())
            .subtitle(&detail)
            .build();
        let dot = gtk::Label::new(None);
        dot.set_markup(&format!(
            "<span foreground=\"{color}\" size=\"large\">●</span>"
        ));
        dot.set_valign(gtk::Align::Center);
        row.add_prefix(&dot);
        tools_group.add(&row);
    }
    page.add(&tools_group);

    // --- Run + progress ---
    let run_group = adw::PreferencesGroup::new();
    let run_button = gtk::Button::builder()
        .label("Patch HDR10")
        .halign(gtk::Align::Center)
        .build();
    run_button.add_css_class("suggested-action");
    run_button.add_css_class("pill");
    let progress = gtk::ProgressBar::builder()
        .show_text(true)
        .visible(false)
        .build();
    let status = gtk::Label::builder().wrap(true).xalign(0.0).build();
    let run_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(6)
        .build();
    run_box.append(&run_button);
    run_box.append(&progress);
    run_box.append(&status);
    run_group.add(&run_box);
    page.add(&run_group);

    toolbar.set_content(Some(&page));
    window.set_content(Some(&toolbar));

    // --- Wiring: file choosers ---
    let ffprobe_path = toolset
        .resolution(Tool::Ffprobe)
        .path()
        .map(Path::to_path_buf);

    // Source (also probes for metadata).
    {
        let window = window.clone();
        let selection = selection.clone();
        let row = source_row.clone();
        let meta_row = meta_row.clone();
        let ffprobe_path = ffprobe_path.clone();
        source_btn.connect_clicked(move |_| {
            let dialog = gtk::FileDialog::builder()
                .title("Choose HDR10 source")
                .build();
            let selection = selection.clone();
            let row = row.clone();
            let meta_row = meta_row.clone();
            let ffprobe_path = ffprobe_path.clone();
            dialog.open(Some(&window), gio::Cancellable::NONE, move |result| {
                if let Some(path) = result.ok().and_then(|f| f.path()) {
                    row.set_subtitle(&path.display().to_string());
                    if let Some(ffprobe) = &ffprobe_path {
                        match probe::probe_hdr10(ffprobe, &path) {
                            Ok(p) => {
                                meta_row.set_title("HDR10 detected");
                                meta_row.set_subtitle(&metadata_summary(&p));
                            }
                            Err(e) => {
                                meta_row.set_title("Probe failed");
                                meta_row.set_subtitle(&e.to_string());
                            }
                        }
                    }
                    selection.borrow_mut().source = Some(path);
                }
            });
        });
    }

    // Encode (open).
    connect_open_into(
        &encoded_btn,
        &window,
        &selection,
        &encoded_row,
        |sel, path| {
            sel.encoded = Some(path);
        },
    );
    // Output (save).
    connect_save_into(&output_btn, &window, &selection, &output_row);

    // --- Wiring: run ---
    {
        let selection = selection.clone();
        let overrides = settings.tool_overrides.clone();
        let progress = progress.clone();
        let status = status.clone();
        let run_button_for_async = run_button.clone();
        run_button.connect_clicked(move |btn| {
            let (source, encoded, output) = {
                let sel = selection.borrow();
                match (sel.source.clone(), sel.encoded.clone(), sel.output.clone()) {
                    (Some(s), Some(e), Some(o)) => (s, e, o),
                    _ => {
                        status.set_text("Choose a source, an encode and an output file first.");
                        return;
                    }
                }
            };

            btn.set_sensitive(false);
            progress.set_visible(true);
            progress.set_fraction(0.0);
            status.set_text("Working…");

            let (tx, rx) = async_channel::unbounded::<Msg>();
            let overrides = overrides.clone();

            // Worker thread: the whole pipeline.
            gio::spawn_blocking(move || {
                let tools = Toolset::discover(&overrides);
                let opts = PipelineOptions::default();
                let result = pipeline::run_with_progress(
                    &tools,
                    &source,
                    &encoded,
                    &output,
                    &opts,
                    &mut |stage| {
                        let _ = tx.send_blocking(Msg::Progress(stage));
                    },
                );
                let outcome = result.map(|r| r.output).map_err(|e| e.to_string());
                let _ = tx.send_blocking(Msg::Done(outcome));
            });

            // UI side: consume progress messages.
            let progress = progress.clone();
            let status = status.clone();
            let button = run_button_for_async.clone();
            glib::spawn_future_local(async move {
                while let Ok(msg) = rx.recv().await {
                    match msg {
                        Msg::Progress(stage) => {
                            progress.set_text(Some(stage.label()));
                            progress.set_fraction(stage_fraction(stage));
                        }
                        Msg::Done(Ok(path)) => {
                            progress.set_fraction(1.0);
                            progress.set_text(Some("Done"));
                            status.set_text(&format!("✓ Wrote {}", path.display()));
                            button.set_sensitive(true);
                        }
                        Msg::Done(Err(e)) => {
                            progress.set_visible(false);
                            status.set_text(&format!("Error: {e}"));
                            button.set_sensitive(true);
                        }
                    }
                }
            });
        });
    }

    window.present();
}

/// Builds an `ActionRow` with a trailing "choose file" button.
fn file_row(title: &str, subtitle: &str) -> (adw::ActionRow, gtk::Button) {
    let button = gtk::Button::builder()
        .icon_name("document-open-symbolic")
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("flat");
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    (row, button)
}

/// Wires an "open file" chooser that stores the chosen path into `selection`.
fn connect_open_into(
    button: &gtk::Button,
    window: &adw::ApplicationWindow,
    selection: &Rc<RefCell<Selection>>,
    row: &adw::ActionRow,
    set: fn(&mut Selection, PathBuf),
) {
    let window = window.clone();
    let selection = selection.clone();
    let row = row.clone();
    button.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::builder().title("Choose a file").build();
        let selection = selection.clone();
        let row = row.clone();
        dialog.open(Some(&window), gio::Cancellable::NONE, move |result| {
            if let Some(path) = result.ok().and_then(|f| f.path()) {
                row.set_subtitle(&path.display().to_string());
                set(&mut selection.borrow_mut(), path);
            }
        });
    });
}

/// Wires a "save file" chooser that stores the chosen path as the output.
fn connect_save_into(
    button: &gtk::Button,
    window: &adw::ApplicationWindow,
    selection: &Rc<RefCell<Selection>>,
    row: &adw::ActionRow,
) {
    let window = window.clone();
    let selection = selection.clone();
    let row = row.clone();
    button.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::builder()
            .title("Choose output file")
            .initial_name("out.mkv")
            .build();
        let selection = selection.clone();
        let row = row.clone();
        dialog.save(Some(&window), gio::Cancellable::NONE, move |result| {
            if let Some(path) = result.ok().and_then(|f| f.path()) {
                row.set_subtitle(&path.display().to_string());
                selection.borrow_mut().output = Some(path);
            }
        });
    });
}

/// Colour and description for a tool's resolution.
fn tool_status(tool: Tool, resolution: &Resolution) -> (&'static str, String) {
    match resolution {
        Resolution::OnPath(p) => (DOT_GREEN, format!("On PATH — {}", p.display())),
        Resolution::Override(p) => (DOT_GREEN, format!("Custom path — {}", p.display())),
        Resolution::Fallback(p) => (DOT_BLUE, format!("Install dir — {}", p.display())),
        Resolution::NotFound => (DOT_RED, format!("Not found — {}", tool.install_hint())),
    }
}

/// A one-line, friendly summary of probed metadata for the UI.
fn metadata_summary(p: &ProbedHdr10) -> String {
    let c = &p.color;
    let mut parts = vec![
        format!(
            "{} / {} / {}",
            naming::primaries_name(c.colour_primaries),
            naming::transfer_name(c.transfer_characteristics),
            naming::matrix_name(c.matrix_coeffs),
        ),
        naming::range_name(c.video_full_range).to_string(),
    ];
    if let Some(md) = &p.mastering_display {
        let gamut = naming::gamut_name(md).unwrap_or("Custom");
        parts.push(format!(
            "{gamut}, {:.0} nits",
            md.max_luminance as f64 / 10_000.0
        ));
    }
    if let Some(cll) = &p.content_light_level {
        parts.push(format!("MaxCLL {}", cll.max_cll));
    }
    parts.join(" · ")
}

/// Rough progress fraction for each pipeline stage.
fn stage_fraction(stage: Stage) -> f64 {
    match stage {
        Stage::Probing => 0.1,
        Stage::Extracting => 0.3,
        Stage::Patching => 0.55,
        Stage::CapturingTiming => 0.75,
        Stage::Remuxing => 0.9,
        Stage::Done => 1.0,
    }
}
