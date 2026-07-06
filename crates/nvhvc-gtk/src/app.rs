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
    /// The live processing view, kept across progress updates so its spinner
    /// animation isn't restarted by every stage change.
    pub processing_ui: RefCell<Option<stages::ProcessingUi>>,
}

/// Builds the application and runs the GTK main loop.
pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| {
        load_css();
        install_bundled_icons();
    });
    app.connect_activate(build_ui);
    app.run()
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(css::STYLE);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Icons the stock theme lacks, written to the cache dir and added as an (unthemed)
/// icon search path. Monochrome `-symbolic` SVGs recolor with the theme like any other.
fn install_bundled_icons() {
    // Stage-2 encode glyph: the GNOME icon-library "arrow-into-box" (an arrow pointing into a
    // rounded box). Bundled as a plain file on the unthemed search path; the library ships it
    // via gresource, but we don't need that packaging here.
    const ARROW_INTO_BOX_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" height="16px" viewBox="0 0 16 16" width="16px"><g fill="#222222"><path d="m 5 -0.0078125 c -1.644531 0 -3 1.3554685 -3 3.0000005 v 2 h 2 v -2 c 0 -0.570313 0.429688 -1 1 -1 h 8 c 0.570312 0 1 0.429687 1 1 v 10 c 0 0.570312 -0.429688 1 -1 1 h -8 c -0.570312 0 -1 -0.429688 -1 -1 v -2 h -2 v 2 c 0 1.644531 1.355469 3 3 3 h 8 c 1.644531 0 3 -1.355469 3 -3 v -10 c 0 -1.644532 -1.355469 -3.0000005 -3 -3.0000005 z m 0 0"/><path d="m 6 3.992188 v 1 c 0 0.257812 0.128906 0.527343 0.3125 0.71875 l 1.28125 1.28125 h -7.59375 v 2 h 7.59375 l -1.28125 1.28125 c -0.183594 0.191406 -0.3125 0.410156 -0.3125 0.71875 v 1 h 1 c 0.308594 0 0.550781 -0.089844 0.75 -0.28125 l 3.65625 -3.71875 l -3.65625 -3.71875 c -0.199219 -0.191407 -0.441406 -0.28125 -0.75 -0.28125 z m 0 0"/></g></svg>"##;

    // Stage-1 source glyph: the GNOME icon-library "video-reel" (a film box with a play
    // button). Bundled as a plain file on the same unthemed search path as the upload glyph —
    // the library ships it via gresource, but we don't need that packaging here.
    const REEL_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" height="16px" viewBox="0 0 16 16" width="16px"><g fill="#222222"><path d="m 0 0 v 15 h 2 v -1 h 12 v 1 h 2 v -15 h -2 v 1 h -12 v -1 z m 2 3 h 12 v 9 h -12 z m 0 0"/><path d="m 13 11 v -1 h -1 v 1 z m -9 0 v -1 h -1 v 1 z m 9 -2 v -1 h -1 v 1 z m -9 0 v -1 h -1 v 1 z m 9 -2 v -1 h -1 v 1 z m -9 0 v -1 h -1 v 1 z m 9 -2 v -1 h -1 v 1 z m -9 0 v -1 h -1 v 1 z m 0 0"/><path d="m 7 4.5 c 0.195312 0 0.386719 0.058594 0.554688 0.167969 l 3 2 c 0.589843 0.394531 0.589843 1.269531 0 1.664062 l -3 2 c -0.167969 0.109375 -0.359376 0.167969 -0.554688 0.167969 c 0 0 -1.003906 0 -1 -1 v -4 c -0.003906 -1 1 -1 1 -1 z m 0 0"/></g></svg>"##;

    let dir = glib::user_cache_dir().join("nvhvc-icons");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::write(dir.join("arrow-into-box-symbolic.svg"), ARROW_INTO_BOX_SVG);
    let _ = std::fs::write(dir.join("video-reel-symbolic.svg"), REEL_SVG);
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_search_path(&dir);
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
        processing_ui: RefCell::new(None),
    });

    install_actions(app, &ctx);
    app.set_menubar(Some(&build_menubar()));
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
///
/// Exception: while the pipeline runs, the processing view is built once and then only
/// *updated* per stage message, so its spinner animation runs uninterrupted.
pub fn render(ctx: &Rc<Ctx>) {
    let (stage, processing) = {
        let s = ctx.state.borrow();
        (s.stage, s.processing)
    };

    ctx.title.set_subtitle(&format!("Step {stage} of 3"));
    ctx.back_btn.set_visible(stage > 1 && processing.is_none());
    for (i, dot) in ctx.dots.iter().enumerate() {
        if i + 1 == stage as usize {
            dot.add_css_class("active");
        } else {
            dot.remove_css_class("active");
        }
    }

    let page = &ctx.pages[stage as usize - 1];

    if let Some(pipe_stage) = processing {
        let mut slot = ctx.processing_ui.borrow_mut();
        match slot.as_ref() {
            Some(ui) => ui.update(pipe_stage),
            None => {
                while let Some(child) = page.first_child() {
                    page.remove(&child);
                }
                let ui = stages::build_processing(pipe_stage);
                page.append(&ui.root);
                *slot = Some(ui);
            }
        }
        ctx.stack.set_visible_child_name(&format!("stage{stage}"));
        while let Some(child) = ctx.footer.first_child() {
            ctx.footer.remove(&child);
        }
        ctx.footer.set_visible(false);
        return;
    }
    ctx.processing_ui.replace(None);

    // Rebuild the visible page.
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
    ctx.footer.set_visible(true);
    stages::build_footer(ctx, &ctx.footer);

    // "Open Encode…" only makes sense once a source is picked.
    if let Some(app) = ctx.window.application()
        && let Some(action) = app.lookup_action("open-encode")
        && let Some(action) = action.downcast_ref::<gio::SimpleAction>()
    {
        action.set_enabled(ctx.state.borrow().source.is_some() && processing.is_none());
    }
}

// ===================== Application actions & exported menubar =====================

/// Registers the application's `GAction`s and their accelerators.
///
/// These back both the in-window menu popover and the exported menubar (global menus).
fn install_actions(app: &adw::Application, ctx: &Rc<Ctx>) {
    let add = |name: &str, accels: &[&str], f: Box<dyn Fn()>| {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(move |_, _| f());
        app.add_action(&action);
        if !accels.is_empty() {
            app.set_accels_for_action(&format!("app.{name}"), accels);
        }
        action
    };

    {
        let ctx = ctx.clone();
        add(
            "open-source",
            &["<Control>o"],
            Box::new(move || {
                pick_file(&ctx, false, "Choose HDR10 source", None, |ctx, path| {
                    ctx.state.borrow_mut().stage = 1;
                    start_probe(ctx, path, true);
                });
            }),
        );
    }
    {
        let ctx = ctx.clone();
        add(
            "open-encode",
            &["<Control><Shift>o"],
            Box::new(move || {
                if ctx.state.borrow().source.is_none() {
                    return;
                }
                pick_file(
                    &ctx,
                    false,
                    "Choose the encode to patch",
                    None,
                    |ctx, path| {
                        ctx.state.borrow_mut().stage = 2;
                        start_probe(ctx, path, false);
                    },
                );
            }),
        );
    }
    {
        let ctx = ctx.clone();
        add(
            "preferences",
            &["<Control>comma"],
            Box::new(move || prefs::show(&ctx.window)),
        );
    }
    {
        let window = ctx.window.clone();
        add(
            "shortcuts",
            &["<Control>question"],
            Box::new(move || show_shortcuts(&window)),
        );
    }
    {
        let window = ctx.window.clone();
        add("help", &["F1"], Box::new(move || show_help(&window)));
    }
    {
        let window = ctx.window.clone();
        add("about", &[], Box::new(move || show_about(&window)));
    }
    {
        let app = app.clone();
        add("quit", &["<Control>q"], Box::new(move || app.quit()));
    }

    // Stateful radio action for the theme (System / Light / Dark).
    let initial = Settings::load().ui_theme.unwrap_or_else(|| "system".into());
    let theme = gio::SimpleAction::new_stateful(
        "theme",
        Some(glib::VariantTy::STRING),
        &initial.to_variant(),
    );
    theme.connect_activate(|action, param| {
        let Some(param) = param else { return };
        let Some(mode) = param.str() else { return };
        action.set_state(param);
        apply_theme_setting(mode);
        let mut settings = Settings::load();
        settings.ui_theme = Some(mode.to_owned());
        let _ = settings.save();
    });
    app.add_action(&theme);
}

/// The menubar model. `AdwApplicationWindow` never draws it in-window; it exists so
/// `GtkApplication` exports it over D-Bus (plus the `_GTK_MENUBAR_OBJECT_PATH` X11
/// window properties), which desktop global menus — e.g. KDE Plasma's — pick up.
fn build_menubar() -> gio::Menu {
    let file = gio::Menu::new();
    file.append(Some("Open Source…"), Some("app.open-source"));
    file.append(Some("Open Encode…"), Some("app.open-encode"));
    let file_quit = gio::Menu::new();
    file_quit.append(Some("Quit"), Some("app.quit"));
    file.append_section(None, &file_quit);

    let theme = gio::Menu::new();
    theme.append(Some("System"), Some("app.theme::system"));
    theme.append(Some("Light"), Some("app.theme::light"));
    theme.append(Some("Dark"), Some("app.theme::dark"));
    let view = gio::Menu::new();
    view.append_submenu(Some("Theme"), &theme);

    let help = gio::Menu::new();
    help.append(Some("Help"), Some("app.help"));
    help.append(Some("About nvhvc"), Some("app.about"));

    let menubar = gio::Menu::new();
    menubar.append_submenu(Some("File"), &file);
    menubar.append_submenu(Some("View"), &view);
    menubar.append_submenu(Some("Help"), &help);
    menubar
}

/// The keyboard-shortcuts dialog (native `AdwShortcutsDialog`; `GtkShortcutsWindow` is
/// deprecated at GTK 4.18).
fn show_shortcuts(window: &adw::ApplicationWindow) {
    let files = adw::ShortcutsSection::new(Some("Files"));
    files.add(adw::ShortcutsItem::new("Open source", "<Control>o"));
    files.add(adw::ShortcutsItem::new("Open encode", "<Control><Shift>o"));

    let general = adw::ShortcutsSection::new(Some("General"));
    general.add(adw::ShortcutsItem::new("Preferences", "<Control>comma"));
    general.add(adw::ShortcutsItem::new(
        "Keyboard shortcuts",
        "<Control>question",
    ));
    general.add(adw::ShortcutsItem::new("Help", "F1"));
    general.add(adw::ShortcutsItem::new("Quit", "<Control>q"));

    let dialog = adw::ShortcutsDialog::new();
    dialog.add(files);
    dialog.add(general);
    dialog.present(Some(window));
}

/// Opens the project page (the closest thing to online help).
fn show_help(window: &adw::ApplicationWindow) {
    gtk::UriLauncher::new("https://github.com/Mahir007/nvhvc_hdr10-tool").launch(
        Some(window),
        gio::Cancellable::NONE,
        |_| {},
    );
}

// ===================== App menu (theme circles + entries) =====================

fn build_app_menu(ctx: &Rc<Ctx>) -> gtk::Popover {
    let popover = gtk::Popover::new();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
    root.set_width_request(230);

    // Theme selector circles with a check badge on the selected one (per the mockup).
    let circles = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(18)
        .halign(gtk::Align::Center)
        .margin_top(9)
        .margin_bottom(11)
        .build();
    struct Circle {
        mode: &'static str,
        button: gtk::Button,
        badge: gtk::Image,
    }
    let mk_circle = |mode: &'static str, tooltip: &str| -> (gtk::Overlay, Circle) {
        let button = gtk::Button::new();
        button.add_css_class("theme-circle");
        button.add_css_class(mode);
        button.set_tooltip_text(Some(tooltip));
        let badge = gtk::Image::from_icon_name("object-select-symbolic");
        badge.add_css_class("badge-check");
        badge.set_pixel_size(9);
        badge.set_halign(gtk::Align::End);
        badge.set_valign(gtk::Align::End);
        badge.set_can_target(false);
        badge.set_visible(false);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&button));
        overlay.add_overlay(&badge);
        (
            overlay,
            Circle {
                mode,
                button,
                badge,
            },
        )
    };
    let (sys_w, sys) = mk_circle("system", "Follow system style");
    let (light_w, light) = mk_circle("light", "Light style");
    let (dark_w, dark) = mk_circle("dark", "Dark style");
    circles.append(&sys_w);
    circles.append(&light_w);
    circles.append(&dark_w);

    let all = Rc::new([sys, light, dark]);
    let sync = {
        let all = all.clone();
        let window = ctx.window.clone();
        move || {
            // Read the current theme from the in-memory `theme` action state (its single source
            // of truth), not by re-reading settings.json off disk on every menu open / click.
            let current = window
                .application()
                .and_then(|a| a.lookup_action("theme"))
                .and_then(|a| a.state())
                .and_then(|v| v.str().map(str::to_owned))
                .unwrap_or_else(|| "system".into());
            for c in all.iter() {
                if c.mode == current {
                    c.button.add_css_class("selected");
                } else {
                    c.button.remove_css_class("selected");
                }
                c.badge.set_visible(c.mode == current);
            }
        }
    };
    sync();
    for c in all.iter() {
        let window = ctx.window.clone();
        let mode = c.mode;
        let sync = sync.clone();
        c.button.connect_clicked(move |_| {
            // Route through the app action so the exported menubar radio stays in sync.
            let _ = gtk::prelude::WidgetExt::activate_action(
                &window,
                "app.theme",
                Some(&mode.to_variant()),
            );
            sync();
        });
    }
    {
        // Theme may also change from the (global) menubar — refresh whenever we open.
        let sync = sync.clone();
        popover.connect_show(move |_| sync());
    }
    root.append(&circles);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // Menu rows: icon + label, activating the shared app actions.
    let entry = |icon: &str, label: &str, action: &'static str| -> gtk::Button {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 11);
        row.set_halign(gtk::Align::Start);
        let image = gtk::Image::from_icon_name(icon);
        image.add_css_class("dim-label");
        row.append(&image);
        row.append(&gtk::Label::new(Some(label)));
        let b = gtk::Button::builder().child(&row).build();
        b.add_css_class("flat");
        b.add_css_class("menu-entry");
        let window = ctx.window.clone();
        let popover_weak = glib::clone::Downgrade::downgrade(&popover);
        b.connect_clicked(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.popdown();
            }
            let _ = gtk::prelude::WidgetExt::activate_action(&window, action, None);
        });
        b
    };
    // No "Keyboard Shortcuts" row — it's too minor to earn a menu slot. The shortcuts
    // themselves (incl. Ctrl+? to open the shortcuts dialog) still work.
    root.append(&entry(
        "emblem-system-symbolic",
        "Preferences",
        "app.preferences",
    ));
    root.append(&entry("help-browser-symbolic", "Help", "app.help"));
    root.append(&entry("help-about-symbolic", "About nvhvc", "app.about"));

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
    let about = adw::AboutDialog::builder()
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
    about.present(Some(window));
}

/// Runs the pipeline, first asking about overwriting an existing output file the user
/// didn't pick themselves (the save dialog warns for manual choices; the auto-derived
/// default name gets this dialog instead).
pub fn confirm_and_run(ctx: &Rc<Ctx>) {
    let pending_overwrite = {
        let s = ctx.state.borrow();
        match &s.output {
            Some(out) if out.exists() && !s.output_user_chosen => Some(
                out.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| out.display().to_string()),
            ),
            Some(_) => None,
            None => return,
        }
    };
    let Some(name) = pending_overwrite else {
        start_run(ctx);
        return;
    };
    let dialog = adw::AlertDialog::builder()
        .heading(format!("Replace “{name}”?"))
        .body("A file with this name already exists. Replacing it will overwrite its contents.")
        .build();
    // Replace is added first so it sits on the LEFT (destructive, red) — deliberately not
    // under the pointer's default resting spot. Cancel stays the default/close response so
    // Enter and Esc both cancel safely.
    dialog.add_responses(&[("replace", "Replace"), ("cancel", "Cancel")]);
    dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    {
        let ctx = ctx.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "replace" {
                start_run(&ctx);
            }
        });
    }
    dialog.present(Some(&ctx.window));
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
            s.output_user_chosen = false;
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
                tracks: r.tracks,
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

/// Picks a source (or encode) file and starts its probe — the dialog title follows `is_source`.
/// The app-level `open-source`/`open-encode` actions stay inline because they also set the stage.
pub fn pick_and_probe(ctx: &Rc<Ctx>, is_source: bool) {
    let title = if is_source {
        "Choose HDR10 source"
    } else {
        "Choose the encode to patch"
    };
    pick_file(ctx, false, title, None, move |ctx, path| {
        start_probe(ctx, path, is_source);
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
