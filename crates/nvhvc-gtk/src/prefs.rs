//! The Preferences dialog: a native `AdwPreferencesDialog` listing the external-tool paths as
//! `AdwActionRow`s (status-dot prefix, name, "status · path" subtitle, pencil Edit suffix) and an
//! About group. Editing opens a small native `AdwAlertDialog` with an entry — so a row is never
//! "stuck" in an editable state: you click the pencil, set or cancel, and the row reverts.

use adw::prelude::*;

use nvhvc_core::config::Settings;
use nvhvc_core::pipeline::Container;
use nvhvc_core::tools::{Overrides, Resolution, Tool, Toolset};

/// Shows the Preferences dialog as a sheet inside `parent`.
pub fn show(parent: &adw::ApplicationWindow) {
    let page = adw::PreferencesPage::new();

    // --- External tools ---
    let tools_group = adw::PreferencesGroup::builder()
        .title("External tools")
        .description(
            "Point any tool at your own build. A custom path is always used, even when a copy is \
             found on PATH.",
        )
        .build();
    let settings = Settings::load();
    let toolset = Toolset::discover(&settings.tool_overrides);
    for (tool, resolution) in toolset.all() {
        tools_group.add(&tool_row(parent, tool, resolution));
    }
    page.add(&tools_group);

    // --- Output ---
    let output_group = adw::PreferencesGroup::builder().title("Output").build();
    output_group.add(&container_row(&settings));
    page.add(&output_group);

    // --- About ---
    let about_group = adw::PreferencesGroup::builder().title("About").build();
    about_group.add(&about_row("nvhvc HDR10 patcher", nvhvc_core::VERSION));
    page.add(&about_group);

    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Preferences");
    dialog.add(&page);
    dialog.present(Some(parent));
}

/// One external tool as a read-only row with a pencil that opens the editor.
fn tool_row(
    window: &adw::ApplicationWindow,
    tool: Tool,
    resolution: &Resolution,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(tool.label())
        .subtitle(status_subtitle(tool, resolution))
        .build();

    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("tool-dot");
    dot.set_valign(gtk::Align::Center);
    apply_dot(&dot, resolution);
    row.add_prefix(&dot);

    let edit = gtk::Button::from_icon_name("document-edit-symbolic");
    edit.add_css_class("flat");
    edit.set_valign(gtk::Align::Center);
    edit.set_tooltip_text(Some("Edit path"));
    row.add_suffix(&edit);
    // Activating the row (click anywhere / Enter) opens the editor too.
    row.set_activatable_widget(Some(&edit));

    {
        let window = window.clone();
        let row = row.clone();
        let dot = dot.clone();
        edit.connect_clicked(move |_| open_editor(&window, tool, &row, &dot));
    }
    row
}

/// Opens a modal editor for a tool's custom path; on "Set" saves and refreshes the row in place.
fn open_editor(window: &adw::ApplicationWindow, tool: Tool, row: &adw::ActionRow, dot: &gtk::Box) {
    let settings = Settings::load();
    let toolset = Toolset::discover(&settings.tool_overrides);
    // Prefer the saved custom override so a broken path stays visible/editable; otherwise show
    // where the tool currently resolves (PATH / install dir).
    let current = tool_override(&settings.tool_overrides, tool)
        .map(|p| p.display().to_string())
        .or_else(|| {
            toolset
                .resolution(tool)
                .path()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_default();

    let entry = gtk::Entry::builder()
        .text(&current)
        .placeholder_text("/path/to/executable")
        .hexpand(true)
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::new(
        Some(&format!("{} — custom executable", tool.label())),
        Some("Point at your own build, or leave empty to auto-detect."),
    );
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "Cancel"), ("set", "Set")]);
    dialog.set_response_appearance("set", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("set"));
    dialog.set_close_response("cancel");

    {
        let row = row.clone();
        let dot = dot.clone();
        let entry = entry.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "set" {
                return;
            }
            let value = entry.text().trim().to_string();
            let mut settings = Settings::load();
            set_tool_override(&mut settings.tool_overrides, tool, &value);
            let _ = settings.save();
            let toolset = Toolset::discover(&settings.tool_overrides);
            let res = toolset.resolution(tool);
            row.set_subtitle(&status_subtitle(tool, res));
            apply_dot(&dot, res);
        });
    }
    dialog.present(Some(window));
    entry.grab_focus();
}

/// `"On PATH · /usr/bin/ffmpeg"`-style subtitle for a resolution.
fn status_subtitle(tool: Tool, resolution: &Resolution) -> String {
    match resolution {
        Resolution::OnPath(p) => format!("On PATH · {}", p.display()),
        Resolution::Override(p) => format!("Custom path · {}", p.display()),
        Resolution::Fallback(p) => format!("Install dir · {}", p.display()),
        Resolution::OverrideMissing(p) => format!("Custom path not found · {}", p.display()),
        Resolution::NotFound => format!("Not found · {}", tool.install_hint()),
    }
}

/// Recolours a status dot from a resolution. green = on PATH · blue = custom · purple = install
/// dir · red = missing (including a custom path that doesn't resolve).
fn apply_dot(dot: &gtk::Box, resolution: &Resolution) {
    for c in ["green", "blue", "purple", "red"] {
        dot.remove_css_class(c);
    }
    let class = match resolution {
        Resolution::OnPath(_) => "green",
        Resolution::Override(_) => "blue",
        Resolution::Fallback(_) => "purple",
        Resolution::OverrideMissing(_) | Resolution::NotFound => "red",
    };
    dot.add_css_class(class);
}

/// The "Default output container" combo row: persisted immediately on change, picked up by
/// the wizard at its next start ("Patch another" or app launch). The current wizard's own
/// container choice lives on stage 2.
fn container_row(settings: &Settings) -> adw::ComboRow {
    let labels: Vec<&str> = Container::ALL.iter().map(|c| c.label()).collect();
    let row = adw::ComboRow::builder()
        .title("Default output container")
        .subtitle("Used for new patches; stage 2 can override per file")
        .model(&gtk::StringList::new(&labels))
        .build();
    let current = Container::from_setting(settings.default_container.as_deref());
    row.set_selected(
        Container::ALL
            .iter()
            .position(|&c| c == current)
            .unwrap_or(0) as u32,
    );
    row.connect_selected_notify(|row| {
        let Some(&container) = Container::ALL.get(row.selected() as usize) else {
            return;
        };
        let mut settings = Settings::load();
        settings.default_container = Some(container.as_setting().to_owned());
        let _ = settings.save();
    });
    row
}

/// An "About" row: title on the left, a dim value on the right.
fn about_row(title: &str, value: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    let v = gtk::Label::new(Some(value));
    v.add_css_class("dim-label");
    v.set_valign(gtk::Align::Center);
    row.add_suffix(&v);
    row
}

/// The saved custom override for a tool, if any.
fn tool_override(overrides: &Overrides, tool: Tool) -> Option<std::path::PathBuf> {
    match tool {
        Tool::Ffmpeg => overrides.ffmpeg.clone(),
        Tool::Ffprobe => overrides.ffprobe.clone(),
        Tool::Mkvmerge => overrides.mkvmerge.clone(),
        Tool::Mkvextract => overrides.mkvextract.clone(),
        Tool::Mp4box => overrides.mp4box.clone(),
    }
}

/// Writes (or clears, when `value` is empty) a tool's custom path.
fn set_tool_override(overrides: &mut Overrides, tool: Tool, value: &str) {
    let path = if value.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(value))
    };
    match tool {
        Tool::Ffmpeg => overrides.ffmpeg = path,
        Tool::Ffprobe => overrides.ffprobe = path,
        Tool::Mkvmerge => overrides.mkvmerge = path,
        Tool::Mkvextract => overrides.mkvextract = path,
        Tool::Mp4box => overrides.mp4box = path,
    }
}
