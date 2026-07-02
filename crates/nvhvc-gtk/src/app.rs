//! The libadwaita application: window chrome (header bar, page dots), the three-stage
//! wizard in a `GtkStack`, and the async glue to the core pipeline.
//!
//! Rendering model: state lives in [`Ctx`]; after every change a handler calls
//! [`render`], which rebuilds the visible stage's content and footer from scratch.
//! That keeps the UI a pure function of state (no partial-update bugs) and is cheap
//! at this widget count.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use nvhvc_core::config::Settings;
use nvhvc_core::pipeline::{self, PipelineOptions, Stage as PipeStage};
use nvhvc_core::probe;
use nvhvc_core::tools::{Tool, Toolset};

use crate::state::{AppState, FileInfo, RunOutcome};
use crate::{css, prefs, stages};

const APP_ID: &str = "io.github.Mahir007.NvhvcHdr10Tool";

/// Widgets that live for the whole session plus the mutable wizard state.
pub struct Ctx {
    pub state: RefCell<AppState>,
    pub window: adw::ApplicationWindow,
    pub title: adw::WindowTitle,
    pub back_btn: gtk::Button,
    pub dots: [gtk::Box; 3],
    pub stack: gtk::Stack,
    pub pages: [gtk::Box; 3],
    pub footer: gtk::Box,
}

/// Builds the application and runs the GTK main loop.
pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| load_css());
    app.connect_activate(build_ui);
    app.run()
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    #[allow(deprecated)] // load_from_string needs the v4_12 feature; we target 4.10.
    provider.load_from_data(css::STYLE);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn build_ui(app: &adw::Application) {
    // Restore the persisted theme before the window shows.
    apply_theme_setting(Settings::load().ui_theme.as_deref().unwrap_or("system"));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("HDR10 Patcher")
        .default_width(768)
        .default_height(716)
        .build();

    // --- Header bar ---
    let title = adw::WindowTitle::new("HDR10 Patcher", "Step 1 of 3");
    let back_btn = gtk::Button::from_icon_name("go-previous-symbolic");
    back_btn.set_tooltip_text(Some("Back"));
    back_btn.set_visible(false);
    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main menu")
        .build();

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));
    header.pack_start(&back_btn);
    header.pack_end(&menu_btn);

    // --- Page dots ---
    let dots_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(7)
        .halign(gtk::Align::Center)
        .build();
    let dots: [gtk::Box; 3] = std::array::from_fn(|_| {
        let d = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        d.add_css_class("dot");
        dots_row.append(&d);
        d
    });
    let dots_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dots_wrap.add_css_class("dots-row");
    dots_wrap.set_halign(gtk::Align::Fill);
    let dots_center = gtk::CenterBox::new();
    dots_center.set_center_widget(Some(&dots_row));
    dots_center.set_hexpand(true);
    dots_wrap.append(&dots_center);

    // --- Stage pages in a stack ---
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::SlideLeftRight)
        .transition_duration(220)
        .vexpand(true)
        .build();
    let pages: [gtk::Box; 3] = std::array::from_fn(|i| {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.set_vexpand(true);
        stack.add_named(&page, Some(&format!("stage{}", i + 1)));
        page
    });

    // --- Footer bar ---
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    footer.add_css_class("footer-bar");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&dots_wrap);
    content.append(&stack);
    content.append(&footer);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    window.set_content(Some(&toolbar));

    let ctx = Rc::new(Ctx {
        state: RefCell::new(AppState::new()),
        window: window.clone(),
        title,
        back_btn: back_btn.clone(),
        dots,
        stack,
        pages,
        footer,
    });

    menu_btn.set_popover(Some(&build_app_menu(&ctx)));

    {
        let ctx = ctx.clone();
        back_btn.connect_clicked(move |_| {
            {
                let mut s = ctx.state.borrow_mut();
                if s.processing.is_some() {
                    return;
                }
                if s.stage == 3 {
                    s.result = None;
                    s.stage = 2;
                } else if s.stage > 1 {
                    s.stage -= 1;
                }
            }
            render(&ctx);
        });
    }

    render(&ctx);
    window.present();
}

/// Rebuilds everything that depends on state: chrome, the visible page, the footer.
pub fn render(ctx: &Rc<Ctx>) {
    let (stage, processing) = {
        let s = ctx.state.borrow();
        (s.stage, s.processing.is_some())
    };

    ctx.title.set_subtitle(&format!("Step {stage} of 3"));
    ctx.back_btn.set_visible(stage > 1 && !processing);
    for (i, dot) in ctx.dots.iter().enumerate() {
        if i + 1 == stage as usize {
            dot.add_css_class("active");
        } else {
            dot.remove_css_class("active");
        }
    }

    // Rebuild the visible page.
    let page = &ctx.pages[stage as usize - 1];
    while let Some(child) = page.first_child() {
        page.remove(&child);
    }
    let body: gtk::Widget = match stage {
        1 => stages::stage_source(ctx),
        2 => stages::stage_encode(ctx),
        _ => stages::stage_result(ctx),
    };
    page.append(&body);
    ctx.stack.set_visible_child_name(&format!("stage{stage}"));

    // Rebuild the footer.
    while let Some(child) = ctx.footer.first_child() {
        ctx.footer.remove(&child);
    }
    ctx.footer.set_visible(!processing);
    if !processing {
        stages::build_footer(ctx, &ctx.footer);
    }
}

// ===================== App menu (theme circles + entries) =====================

fn build_app_menu(ctx: &Rc<Ctx>) -> gtk::Popover {
    let popover = gtk::Popover::new();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.set_width_request(230);

    // Theme selector circles.
    let circles = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(18)
        .halign(gtk::Align::Center)
        .margin_top(9)
        .margin_bottom(9)
        .build();
    let current = Settings::load().ui_theme.unwrap_or_else(|| "system".into());
    let mk_circle = |mode: &'static str, tooltip: &str| -> gtk::Button {
        let b = gtk::Button::new();
        b.add_css_class("theme-circle");
        b.add_css_class(mode);
        b.set_tooltip_text(Some(tooltip));
        if current == mode {
            b.add_css_class("selected");
        }
        b
    };
    let sys = mk_circle("system", "Follow system style");
    let light = mk_circle("light", "Light style");
    let dark = mk_circle("dark", "Dark style");
    for (btn, mode) in [(&sys, "system"), (&light, "light"), (&dark, "dark")] {
        let all = [sys.clone(), light.clone(), dark.clone()];
        btn.connect_clicked(move |b| {
            apply_theme_setting(mode);
            let mut settings = Settings::load();
            settings.ui_theme = Some(mode.to_owned());
            let _ = settings.save();
            for other in &all {
                other.remove_css_class("selected");
            }
            b.add_css_class("selected");
        });
        circles.append(btn);
    }
    root.append(&circles);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let entry = |label: &str| -> gtk::Button {
        let b = gtk::Button::builder()
            .child(
                &gtk::Label::builder()
                    .label(label)
                    .halign(gtk::Align::Start)
                    .build(),
            )
            .build();
        b.add_css_class("flat");
        b
    };
    let prefs_btn = entry("Preferences");
    {
        let ctx = ctx.clone();
        let popover_weak = glib::clone::Downgrade::downgrade(&popover);
        prefs_btn.connect_clicked(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.popdown();
            }
            prefs::show(&ctx.window);
        });
    }
    let about_btn = entry("About nvhvc");
    {
        let window = ctx.window.clone();
        let popover_weak = glib::clone::Downgrade::downgrade(&popover);
        about_btn.connect_clicked(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.popdown();
            }
            show_about(&window);
        });
    }
    root.append(&prefs_btn);
    root.append(&about_btn);

    popover.set_child(Some(&root));
    popover
}

fn apply_theme_setting(mode: &str) {
    let scheme = match mode {
        "light" => adw::ColorScheme::ForceLight,
        "dark" => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    };
    adw::StyleManager::default().set_color_scheme(scheme);
}

fn show_about(window: &adw::ApplicationWindow) {
    let about = adw::AboutWindow::builder()
        .transient_for(window)
        .application_name("HDR10 Patcher")
        .application_icon("video-display")
        .version(nvhvc_core::VERSION)
        .developer_name("nvhvc")
        .license_type(gtk::License::Gpl30)
        .comments(
            "Injects HDR10 metadata into HEVC streams without re-encoding — bit-exact \
             SPS/VUI editing with the encode's frame timing preserved exactly.",
        )
        .build();
    about.present();
}

// ===================== Async: probing & the pipeline =====================

/// Kicks off a background ffprobe of `path` for the source (`is_source`) or encode slot.
pub fn start_probe(ctx: &Rc<Ctx>, path: PathBuf, is_source: bool) {
    let seq = {
        let mut s = ctx.state.borrow_mut();
        s.probe_seq += 1;
        if is_source {
            s.src_probing = true;
            s.src_error = None;
            s.source = None;
            // New source invalidates edits made against the old one.
            s.overrides = None;
            s.editing = None;
        } else {
            s.enc_probing = true;
            s.enc_error = None;
            s.encode = None;
            s.output = Some(PathBuf::from(nvhvc_core::uimodel::default_output(
                &path.display().to_string(),
            )));
        }
        s.probe_seq
    };
    render(ctx);

    let (tx, rx) = async_channel::bounded::<Result<FileInfo, String>>(1);
    {
        let path = path.clone();
        gio::spawn_blocking(move || {
            let result = (|| {
                let settings = Settings::load();
                let tools = Toolset::discover(&settings.tool_overrides);
                let ffprobe = tools.require(Tool::Ffprobe).map_err(|e| e.to_string())?;
                let (hdr, info) = probe::probe_media(ffprobe, &path).map_err(|e| e.to_string())?;
                let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                Ok(FileInfo {
                    path,
                    size_bytes,
                    info,
                    hdr,
                })
            })();
            let _ = tx.send_blocking(result);
        });
    }

    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        let Ok(result) = rx.recv().await else { return };
        {
            let mut s = ctx.state.borrow_mut();
            if s.probe_seq != seq {
                return; // a newer pick superseded this probe
            }
            match (is_source, result) {
                (true, Ok(info)) => {
                    s.source = Some(info);
                    s.src_probing = false;
                }
                (true, Err(e)) => {
                    s.src_error = Some(e);
                    s.src_probing = false;
                }
                (false, Ok(info)) => {
                    s.encode = Some(info);
                    s.enc_probing = false;
                }
                (false, Err(e)) => {
                    s.enc_error = Some(e);
                    s.enc_probing = false;
                }
            }
        }
        render(&ctx);
    });
}

/// Runs the pipeline on a worker thread with staged progress.
pub fn start_run(ctx: &Rc<Ctx>) {
    let (source, encoded, output, metadata_override) = {
        let s = ctx.state.borrow();
        let (Some(src), Some(enc), Some(out)) = (&s.source, &s.encode, &s.output) else {
            return;
        };
        (src.path.clone(), enc.path.clone(), out.clone(), s.overrides)
    };
    {
        let mut s = ctx.state.borrow_mut();
        s.processing = Some(PipeStage::Probing);
        s.run_error = None;
    }
    render(ctx);

    enum Msg {
        Progress(PipeStage),
        Done(Result<RunOutcome, String>),
    }

    let (tx, rx) = async_channel::unbounded::<Msg>();
    gio::spawn_blocking(move || {
        let settings = Settings::load();
        let tools = Toolset::discover(&settings.tool_overrides);
        let opts = PipelineOptions {
            chroma_loc: settings.default_chroma_loc,
            keep_intermediates: false,
            metadata_override,
        };
        let tx_progress = tx.clone();
        let result =
            pipeline::run_with_progress(&tools, &source, &encoded, &output, &opts, &mut |stage| {
                let _ = tx_progress.send_blocking(Msg::Progress(stage));
            })
            .map(|r| RunOutcome {
                output: r.output,
                sps_patched: r.sps_patched,
                sei_inserted: r.sei_inserted,
                frames: r.frames,
            })
            .map_err(|e| e.to_string());
        let _ = tx.send_blocking(Msg::Done(result));
    });

    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        while let Ok(msg) = rx.recv().await {
            let done = {
                let mut s = ctx.state.borrow_mut();
                match msg {
                    Msg::Progress(stage) => {
                        s.processing = Some(stage);
                        false
                    }
                    Msg::Done(Ok(outcome)) => {
                        s.processing = None;
                        s.result = Some(outcome);
                        s.stage = 3;
                        true
                    }
                    Msg::Done(Err(e)) => {
                        s.processing = None;
                        s.run_error = Some(e);
                        true
                    }
                }
            };
            render(&ctx);
            if done {
                break;
            }
        }
    });
}

/// Opens a native file dialog; `on_pick` receives the chosen path.
pub fn pick_file(
    ctx: &Rc<Ctx>,
    save: bool,
    title: &str,
    initial_name: Option<&str>,
    on_pick: impl Fn(&Rc<Ctx>, PathBuf) + 'static,
) {
    let mut builder = gtk::FileDialog::builder().title(title);
    if let Some(name) = initial_name {
        builder = builder.initial_name(name);
    }
    let dialog = builder.build();
    let ctx = ctx.clone();
    if save {
        dialog.save(
            Some(&ctx.window.clone()),
            gio::Cancellable::NONE,
            move |result| {
                if let Some(path) = result.ok().and_then(|f| f.path()) {
                    on_pick(&ctx, path);
                }
            },
        );
    } else {
        dialog.open(
            Some(&ctx.window.clone()),
            gio::Cancellable::NONE,
            move |result| {
                if let Some(path) = result.ok().and_then(|f| f.path()) {
                    on_pick(&ctx, path);
                }
            },
        );
    }
}

/// Attaches a file drag-and-drop target to `widget`.
pub fn attach_file_drop(
    ctx: &Rc<Ctx>,
    widget: &impl IsA<gtk::Widget>,
    on_file: impl Fn(&Rc<Ctx>, PathBuf) + 'static,
) {
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let ctx = ctx.clone();
    drop.connect_drop(move |_, value, _, _| {
        if let Ok(list) = value.get::<gdk::FileList>()
            && let Some(path) = list.files().into_iter().next().and_then(|f| f.path())
        {
            on_file(&ctx, path);
            return true;
        }
        false
    });
    widget.add_controller(drop);
}

/// Opens a file with the default handler (plays the output).
pub fn launch_file(window: &adw::ApplicationWindow, path: &std::path::Path) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
    launcher.launch(Some(window), gio::Cancellable::NONE, |_| {});
}

/// Reveals a file in the file manager.
pub fn open_containing_folder(window: &adw::ApplicationWindow, path: &std::path::Path) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
    launcher.open_containing_folder(Some(window), gio::Cancellable::NONE, |_| {});
}
