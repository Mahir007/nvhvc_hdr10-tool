//! The Preferences window: external-tool paths (with inline editing) and an About section.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::pango;

use nvhvc_core::config::Settings;
use nvhvc_core::tools::{Overrides, Resolution, Tool, Toolset};

/// Shows the modal Preferences window.
pub fn show(parent: &adw::ApplicationWindow) {
    let window = adw::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Preferences")
        .default_width(484)
        .default_height(560)
        .build();

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Preferences", "")));

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_top(22)
        .margin_bottom(24)
        .margin_start(22)
        .margin_end(22)
        .build();

    let tools_title = gtk::Label::builder()
        .label("External tools")
        .halign(gtk::Align::Start)
        .build();
    tools_title.add_css_class("section-title");
    body.append(&tools_title);
    let tools_caption = gtk::Label::builder()
        .label("Point any tool at your own build. A custom path is always used, even when a copy is found on PATH.")
        .halign(gtk::Align::Start)
        .wrap(true)
        .xalign(0.0)
        .margin_bottom(13)
        .build();
    tools_caption.add_css_class("dim-label");
    tools_caption.add_css_class("caption");
    body.append(&tools_caption);

    // The tool list rebuilds itself when an edit starts/finishes.
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");
    body.append(&list);

    // index of the row currently being edited, if any
    let editing: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
    rebuild_tools(&list, &editing);

    let about_title = gtk::Label::builder()
        .label("About")
        .halign(gtk::Align::Start)
        .margin_top(22)
        .margin_bottom(9)
        .build();
    about_title.add_css_class("section-title");
    body.append(&about_title);

    let about = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    about.add_css_class("boxed-list");
    let about_row = |label: &str, value: &str| -> gtk::ListBoxRow {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(16)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(16)
            .margin_end(16)
            .build();
        let l = gtk::Label::new(Some(label));
        l.set_hexpand(true);
        l.set_halign(gtk::Align::Start);
        row.append(&l);
        let v = gtk::Label::new(Some(value));
        v.add_css_class("dim-label");
        row.append(&v);
        gtk::ListBoxRow::builder()
            .activatable(false)
            .child(&row)
            .build()
    };
    about.append(&about_row("nvhvc HDR10 patcher", nvhvc_core::VERSION));
    about.append(&about_row("Default output container", "Matroska (.mkv)"));
    body.append(&about);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));
    window.present();
}

/// Fills the tool list from a fresh discovery; rows swap to an inline editor when edited.
fn rebuild_tools(list: &gtk::ListBox, editing: &Rc<RefCell<Option<usize>>>) {
    while let Some(row) = list.first_child() {
        list.remove(&row);
    }
    let settings = Settings::load();
    let toolset = Toolset::discover(&settings.tool_overrides);

    for (i, (tool, resolution)) in toolset.all().into_iter().enumerate() {
        let is_editing = *editing.borrow() == Some(i);
        let row: gtk::Widget = if is_editing {
            edit_row(list, editing, tool, resolution)
        } else {
            view_row(list, editing, i, tool, resolution)
        };
        list.append(
            &gtk::ListBoxRow::builder()
                .activatable(false)
                .child(&row)
                .build(),
        );
    }
}

fn view_row(
    list: &gtk::ListBox,
    editing: &Rc<RefCell<Option<usize>>>,
    index: usize,
    tool: Tool,
    resolution: &Resolution,
) -> gtk::Widget {
    // green = found on PATH · blue = custom user path · purple = well-known install dir.
    let (dot_class, detail) = match resolution {
        Resolution::OnPath(p) => ("green", format!("On PATH · {}", p.display())),
        Resolution::Override(p) => ("blue", format!("Custom path · {}", p.display())),
        Resolution::Fallback(p) => ("purple", format!("Install dir · {}", p.display())),
        Resolution::NotFound => ("red", format!("Not found · {}", tool.install_hint())),
    };

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(15)
        .margin_end(15)
        .build();

    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("tool-dot");
    dot.add_css_class(dot_class);
    dot.set_valign(gtk::Align::Center);
    row.append(&dot);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);
    let name = gtk::Label::builder()
        .label(tool.label())
        .halign(gtk::Align::Start)
        .build();
    name.add_css_class("heading");
    let det = gtk::Label::builder()
        .label(&detail)
        .halign(gtk::Align::Start)
        .ellipsize(pango::EllipsizeMode::Middle)
        .build();
    det.add_css_class("dim-label");
    det.add_css_class("caption");
    text.append(&name);
    text.append(&det);
    row.append(&text);

    let edit_row_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    edit_row_box.set_halign(gtk::Align::Center);
    edit_row_box.append(&gtk::Image::from_icon_name("document-edit-symbolic"));
    edit_row_box.append(&gtk::Label::new(Some("Edit")));
    let edit = gtk::Button::builder().child(&edit_row_box).build();
    edit.set_valign(gtk::Align::Center);
    {
        let list = list.clone();
        let editing = editing.clone();
        edit.connect_clicked(move |_| {
            *editing.borrow_mut() = Some(index);
            rebuild_tools(&list, &editing);
        });
    }
    row.append(&edit);
    row.upcast()
}

fn edit_row(
    list: &gtk::ListBox,
    editing: &Rc<RefCell<Option<usize>>>,
    tool: Tool,
    resolution: &Resolution,
) -> gtk::Widget {
    let area = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(15)
        .margin_end(15)
        .build();

    let title = gtk::Label::builder()
        .label(format!(
            "{} — custom executable (empty resets to auto-detect)",
            tool.label()
        ))
        .halign(gtk::Align::Start)
        .wrap(true)
        .xalign(0.0)
        .build();
    title.add_css_class("heading");
    area.append(&title);

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let entry = gtk::Entry::builder()
        .text(
            resolution
                .path()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        )
        .placeholder_text("/path/to/executable")
        .hexpand(true)
        .build();
    controls.append(&entry);

    let cancel = gtk::Button::with_label("Cancel");
    {
        let list = list.clone();
        let editing = editing.clone();
        cancel.connect_clicked(move |_| {
            *editing.borrow_mut() = None;
            rebuild_tools(&list, &editing);
        });
    }
    controls.append(&cancel);

    let set = gtk::Button::with_label("Set");
    set.add_css_class("suggested-action");
    {
        let list = list.clone();
        let editing = editing.clone();
        let entry = entry.clone();
        set.connect_clicked(move |_| {
            let value = entry.text().trim().to_string();
            let mut settings = Settings::load();
            set_tool_override(&mut settings.tool_overrides, tool, &value);
            let _ = settings.save();
            *editing.borrow_mut() = None;
            rebuild_tools(&list, &editing);
        });
    }
    controls.append(&set);
    area.append(&controls);
    area.upcast()
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
