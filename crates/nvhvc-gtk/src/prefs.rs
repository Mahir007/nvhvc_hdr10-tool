//! The Preferences dialog: a native `AdwPreferencesDialog` with the external-tool paths as
//! editable `AdwEntryRow`s (status dot prefix, status word suffix, apply-on-edit) and an About
//! group. All the old hand-built ListBox / view-edit state machine is gone — each tool row is
//! simply always-editable, applying (Enter or the ✓ button) saves the override, and clearing it
//! resets to auto-detect.

use adw::prelude::*;

use nvhvc_core::config::Settings;
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
        tools_group.add(&tool_row(tool, resolution));
    }
    page.add(&tools_group);

    // --- About ---
    let about_group = adw::PreferencesGroup::builder().title("About").build();
    about_group.add(&about_row("nvhvc HDR10 patcher", nvhvc_core::VERSION));
    about_group.add(&about_row("Default output container", "Matroska (.mkv)"));
    page.add(&about_group);

    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Preferences");
    dialog.add(&page);
    dialog.present(Some(parent));
}

/// One external tool as an editable path row: `[● dot] name  [/path/to/exe……]  [On PATH]`.
/// Applying saves the override; an empty value resets to auto-detection.
fn tool_row(tool: Tool, resolution: &Resolution) -> adw::EntryRow {
    let row = adw::EntryRow::builder()
        .title(tool.label())
        .text(path_text(resolution))
        .show_apply_button(true)
        .build();

    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("tool-dot");
    dot.set_valign(gtk::Align::Center);
    row.add_prefix(&dot);

    let status = gtk::Label::new(None);
    status.add_css_class("dim-label");
    status.add_css_class("caption");
    status.set_valign(gtk::Align::Center);
    row.add_suffix(&status);

    apply_status(&dot, &status, tool, resolution);

    let dot = dot.clone();
    let status = status.clone();
    row.connect_apply(move |row| {
        let value = row.text().trim().to_string();
        let mut settings = Settings::load();
        set_tool_override(&mut settings.tool_overrides, tool, &value);
        let _ = settings.save();
        // Re-resolve so the dot/status (and the auto-detected path, if cleared) refresh in place.
        let toolset = Toolset::discover(&settings.tool_overrides);
        let res = toolset.resolution(tool);
        row.set_text(&path_text(res));
        apply_status(&dot, &status, tool, res);
    });
    row
}

/// The path (or empty) a resolution points at, for the entry text.
fn path_text(resolution: &Resolution) -> String {
    resolution
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// Recolours the status dot and sets the status word + tooltip from a resolution.
/// green = on PATH · blue = custom path · purple = install-dir fallback · red = not found.
fn apply_status(dot: &gtk::Box, status: &gtk::Label, tool: Tool, resolution: &Resolution) {
    for c in ["green", "blue", "purple", "red"] {
        dot.remove_css_class(c);
    }
    let (class, word, detail) = match resolution {
        Resolution::OnPath(p) => ("green", "On PATH", p.display().to_string()),
        Resolution::Override(p) => ("blue", "Custom path", p.display().to_string()),
        Resolution::Fallback(p) => ("purple", "Install dir", p.display().to_string()),
        Resolution::NotFound => ("red", "Not found", tool.install_hint().to_string()),
    };
    dot.add_css_class(class);
    status.set_text(word);
    dot.set_tooltip_text(Some(&format!("{word} · {detail}")));
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
