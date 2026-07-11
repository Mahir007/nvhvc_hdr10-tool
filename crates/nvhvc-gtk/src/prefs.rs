//! The Preferences dialog: a native `AdwPreferencesDialog` listing the external-tool paths and
//! output defaults as `AdwExpanderRow`s (status-dot prefix, name, "status · path" subtitle).
//! Expanding a row reveals its inline `AdwEntryRow` editor — mirroring the WinUI inline
//! editors while staying on the HIG's progressive-disclosure pattern. Applying (the ✓ button
//! or Enter) saves and collapses the row; an empty value resets to auto-detect / the default.

use adw::prelude::*;

use nvhvc_core::config::Settings;
use nvhvc_core::pipeline::Container;
use nvhvc_core::tools::{Overrides, Resolution, Tool, Toolset};

/// Shows the Preferences dialog as a sheet inside `parent`. `on_default_container` fires when
/// the default-container combo changes (the wizard applies it to the current run too, unless
/// the user already picked an output file explicitly).
pub fn show(parent: &adw::ApplicationWindow, on_default_container: impl Fn(Container) + 'static) {
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
    output_group.add(&container_row(&settings, on_default_container));
    output_group.add(&folder_row(parent, &settings));
    page.add(&output_group);

    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Preferences");
    dialog.add(&page);
    dialog.present(Some(parent));
}

/// One external tool as an expander row whose inline entry edits the custom path.
fn tool_row(
    _window: &adw::ApplicationWindow,
    tool: Tool,
    resolution: &Resolution,
) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder()
        .title(tool.label())
        .subtitle(status_subtitle(tool, resolution))
        .build();

    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("tool-dot");
    dot.set_valign(gtk::Align::Center);
    apply_dot(&dot, resolution);
    row.add_prefix(&dot);

    // Prefer the saved custom override so a broken path stays visible/editable; otherwise show
    // where the tool currently resolves (PATH / install dir).
    let settings = Settings::load();
    let current = tool_override(&settings.tool_overrides, tool)
        .map(|p| p.display().to_string())
        .or_else(|| resolution.path().map(|p| p.display().to_string()))
        .unwrap_or_default();
    let entry = adw::EntryRow::builder()
        .title("Custom executable path (empty = auto-detect)")
        .text(&current)
        .show_apply_button(true)
        .build();
    {
        let row = row.clone();
        let dot = dot.clone();
        entry.connect_apply(move |entry| {
            let value = entry.text().trim().to_string();
            let mut settings = Settings::load();
            set_tool_override(&mut settings.tool_overrides, tool, &value);
            let _ = settings.save();
            let toolset = Toolset::discover(&settings.tool_overrides);
            let res = toolset.resolution(tool);
            row.set_subtitle(&status_subtitle(tool, res));
            apply_dot(&dot, res);
            row.set_expanded(false);
        });
    }
    row.add_row(&entry);
    row
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

/// The "Default output container" combo row: persisted immediately on change and reported to
/// the wizard via the callback. Per-file switching happens in the Save as dialog instead.
fn container_row(settings: &Settings, on_change: impl Fn(Container) + 'static) -> adw::ComboRow {
    let labels: Vec<&str> = Container::ALL.iter().map(|c| c.label()).collect();
    let row = adw::ComboRow::builder()
        .title("Default output container")
        .subtitle("Save as can still switch it per file (.mkv / .mp4)")
        .model(&gtk::StringList::new(&labels))
        .build();
    let current = Container::from_setting(settings.default_container.as_deref());
    row.set_selected(
        Container::ALL
            .iter()
            .position(|&c| c == current)
            .unwrap_or(0) as u32,
    );
    row.connect_selected_notify(move |row| {
        let Some(&container) = Container::ALL.get(row.selected() as usize) else {
            return;
        };
        let mut settings = Settings::load();
        settings.default_container = Some(container.as_setting().to_owned());
        let _ = settings.save();
        on_change(container);
    });
    row
}

/// Subtitle for the default-output-folder row.
fn folder_subtitle(settings: &Settings) -> String {
    settings
        .default_output_dir
        .as_ref()
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "Same folder as the encode".to_owned())
}

/// The "Default output folder" expander row: inline entry (clear it to go back to "same
/// folder as the encode") plus a native folder-picker suffix button.
fn folder_row(window: &adw::ApplicationWindow, settings: &Settings) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder()
        .title("Default output folder")
        .subtitle(folder_subtitle(settings))
        .build();

    let current = settings
        .default_output_dir
        .as_ref()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let entry = adw::EntryRow::builder()
        .title("Folder (empty = same folder as the encode)")
        .text(&current)
        .show_apply_button(true)
        .build();
    let browse = gtk::Button::from_icon_name("folder-open-symbolic");
    browse.add_css_class("flat");
    browse.set_valign(gtk::Align::Center);
    browse.set_tooltip_text(Some("Browse…"));
    entry.add_suffix(&browse);
    {
        let entry = entry.clone();
        let window = window.clone();
        browse.connect_clicked(move |_| {
            let entry = entry.clone();
            gtk::FileDialog::builder()
                .title("Choose the default output folder")
                .build()
                .select_folder(Some(&window), gtk::gio::Cancellable::NONE, move |result| {
                    if let Some(path) = result.ok().and_then(|f| f.path()) {
                        entry.set_text(&path.display().to_string());
                    }
                });
        });
    }
    {
        let row = row.clone();
        entry.connect_apply(move |entry| {
            let value = entry.text().trim().to_string();
            let mut settings = Settings::load();
            settings.default_output_dir = if value.is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(value))
            };
            let _ = settings.save();
            row.set_subtitle(&folder_subtitle(&settings));
            row.set_expanded(false);
        });
    }
    row.add_row(&entry);
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
