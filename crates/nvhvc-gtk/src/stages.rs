//! The three wizard stages and the footer bar, rebuilt from state on every render.

use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{glib, pango};

use nvhvc_core::hevc::patch::Hdr10Metadata;
use nvhvc_core::naming;
use nvhvc_core::pipeline::{Container, Stage as PipeStage};
use nvhvc_core::uimodel::{
    self, Draft, MATRIX_OPTS, META_ROWS, PRIMARIES_OPTS, TRANSFER_OPTS, code_label,
};

use crate::app::{self, Ctx};

// ===================== small builders =====================

/// Scrollable, 560px-clamped stage body.
fn page(child: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let clamp = adw::Clamp::builder()
        .maximum_size(560)
        .tightening_threshold(560)
        .margin_top(26)
        .margin_bottom(26)
        .margin_start(30)
        .margin_end(30)
        .build();
    clamp.set_child(Some(child));
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build()
        .upcast()
}

fn section_label(text: &str) -> gtk::Label {
    let l = gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .margin_start(2)
        .margin_end(2)
        .build();
    l.add_css_class("section-title");
    l
}

fn dim_label(text: &str) -> gtk::Label {
    let l = gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .build();
    l.add_css_class("dim-label");
    l
}

/// A `boxed-list` card.
fn card() -> gtk::ListBox {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");
    list
}

fn card_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    gtk::ListBoxRow::builder()
        .activatable(false)
        .child(child)
        .build()
}

/// Tinted rounded banner with a bold title and the body alongside it — matching the WinUI
/// `InfoBar`s (title and message share a line; a multi-line or long body drops below the
/// title, mirroring the InfoBar's overflow behaviour).
fn banner_titled(kind: &str, icon: &str, title: &str, body: &str) -> gtk::Box {
    let stacked = body.contains('\n') || title.chars().count() + body.chars().count() > 64;
    let sep = if stacked { "\n" } else { "  " };
    banner_markup(
        kind,
        icon,
        &format!(
            "<b>{}</b>{sep}{}",
            gtk::glib::markup_escape_text(title),
            gtk::glib::markup_escape_text(body)
        ),
    )
}

/// Like [`banner`], but the text is Pango markup (caller escapes dynamic pieces).
fn banner_markup(kind: &str, icon: &str, markup: &str) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .margin_top(14)
        .build();
    b.add_css_class("banner");
    b.add_css_class(kind);
    let image = gtk::Image::from_icon_name(icon);
    image.set_valign(gtk::Align::Start);
    image.set_margin_top(2);
    b.append(&image);
    let label = gtk::Label::builder()
        .use_markup(true)
        .label(markup)
        .halign(gtk::Align::Start)
        .wrap(true)
        .xalign(0.0)
        .hexpand(true)
        .build();
    b.append(&label);
    b
}

/// A list-item factory rendering an ellipsizing string label (for `GtkDropDown` buttons:
/// small minimum + capped natural width so three dropdowns fit the 560px clamp).
fn ellipsized_button_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .ellipsize(pango::EllipsizeMode::End)
            .width_chars(6)
            .max_width_chars(12)
            .hexpand(true)
            .build();
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let (Some(label), Some(text)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<gtk::StringObject>(),
        ) {
            label.set_label(&text.string());
        }
    });
    factory
}

/// A list-item factory for a `GtkDropDown`'s popup: full-width label plus the checkmark
/// on the selected item (a custom factory loses the built-in selection indicator).
fn checked_list_factory(dd: &gtk::DropDown) -> gtk::SignalListItemFactory {
    use std::cell::RefCell;
    use std::collections::HashMap;

    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build();
        let check = gtk::Image::from_icon_name("object-select-symbolic");
        check.set_opacity(0.0);
        row.append(&label);
        row.append(&check);
        item.set_child(Some(&row));
    });

    // Each bound row listens for selection changes; handlers are dropped on unbind
    // (list items get recycled, so bind/unbind bookkeeping is required). The factory is
    // owned by the dropdown, so it only holds weak references back to it.
    let handlers: Rc<RefCell<HashMap<usize, gtk::glib::SignalHandlerId>>> =
        Rc::new(RefCell::new(HashMap::new()));
    {
        let dd = glib::clone::Downgrade::downgrade(dd);
        let handlers = handlers.clone();
        factory.connect_bind(move |_, item| {
            let Some(dd) = dd.upgrade() else { return };
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(row) = item.child().and_downcast::<gtk::Box>() else {
                return;
            };
            let Some(label) = row.first_child().and_downcast::<gtk::Label>() else {
                return;
            };
            let Some(check) = row.last_child().and_downcast::<gtk::Image>() else {
                return;
            };
            let Some(obj) = item.item() else { return };
            if let Some(text) = obj.downcast_ref::<gtk::StringObject>() {
                label.set_label(&text.string());
            }
            let selected = dd.selected_item().is_some_and(|sel| sel == obj);
            check.set_opacity(if selected { 1.0 } else { 0.0 });
            let id = dd.connect_selected_item_notify({
                let check = check.clone();
                move |dd| {
                    let selected = dd.selected_item().is_some_and(|sel| sel == obj);
                    check.set_opacity(if selected { 1.0 } else { 0.0 });
                }
            });
            handlers.borrow_mut().insert(item.as_ptr() as usize, id);
        });
    }
    {
        let dd = glib::clone::Downgrade::downgrade(dd);
        let handlers = handlers.clone();
        factory.connect_unbind(move |_, item| {
            let Some(dd) = dd.upgrade() else { return };
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            if let Some(id) = handlers.borrow_mut().remove(&(item.as_ptr() as usize)) {
                dd.disconnect(id);
            }
        });
    }
    factory
}

/// A button whose content is an icon + label. Leading-icon buttons use the native
/// `adw::ButtonContent`; the trailing-icon case (the mockup's "Continue →") is hand-built
/// because `AdwButtonContent` only ever puts the icon before the label.
fn icon_button(label: &str, icon: &str, icon_after: bool) -> gtk::Button {
    if icon_after {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        row.set_halign(gtk::Align::Center);
        row.append(&gtk::Label::new(Some(label)));
        row.append(&gtk::Image::from_icon_name(icon));
        gtk::Button::builder().child(&row).build()
    } else {
        let content = adw::ButtonContent::builder()
            .icon_name(icon)
            .label(label)
            .build();
        gtk::Button::builder().child(&content).build()
    }
}

/// The big dashed drop zone with an inner accent pill.
fn drop_zone(
    ctx: &Rc<Ctx>,
    icon: &str,
    heading: &str,
    caption: &str,
    is_source: bool,
) -> gtk::Widget {
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();

    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("drop-icon");
    image.set_pixel_size(32);
    image.set_halign(gtk::Align::Center);
    image.set_margin_bottom(4);
    inner.append(&image);

    let title = gtk::Label::new(Some(heading));
    title.add_css_class("title-2");
    inner.append(&title);

    let cap = gtk::Label::builder()
        .label(caption)
        .wrap(true)
        .justify(gtk::Justification::Center)
        .max_width_chars(46)
        .build();
    cap.add_css_class("dim-label");
    inner.append(&cap);

    // Only this button opens the picker — the zone itself is inert except for drag-and-drop.
    let pill = icon_button("Open File…", "document-open-symbolic", false);
    pill.add_css_class("open-pill");
    pill.set_halign(gtk::Align::Center);
    pill.set_margin_top(12);
    {
        let ctx = ctx.clone();
        pill.connect_clicked(move |_| app::pick_and_probe(&ctx, is_source));
    }
    inner.append(&pill);

    let zone = gtk::Box::new(gtk::Orientation::Vertical, 0);
    zone.add_css_class("dropzone");
    zone.append(&inner);
    // Hug the content instead of stretching to fill the page.
    zone.set_valign(gtk::Align::Start);

    app::attach_file_drop(ctx, &zone, move |ctx, path| {
        app::start_probe(ctx, path, is_source);
    });
    zone.upcast()
}

/// A file card row: icon block, name + detail, optional pill badge, trailing button.
fn file_row(
    icon: &str,
    icon_class: &str,
    name: &str,
    detail: &str,
    badge: Option<(&str, &str)>,
    action: &gtk::Button,
) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(13)
        .margin_top(13)
        .margin_bottom(13)
        .margin_start(15)
        .margin_end(15)
        .build();

    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("file-icon");
    if !icon_class.is_empty() {
        image.add_css_class(icon_class);
    }
    image.set_pixel_size(21);
    image.set_valign(gtk::Align::Center);
    row.append(&image);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);
    let name_l = gtk::Label::builder()
        .label(name)
        .halign(gtk::Align::Start)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    name_l.add_css_class("heading");
    let detail_l = gtk::Label::builder()
        .label(detail)
        .halign(gtk::Align::Start)
        .ellipsize(pango::EllipsizeMode::Middle)
        .build();
    detail_l.add_css_class("dim-label");
    detail_l.add_css_class("caption");
    text.append(&name_l);
    text.append(&detail_l);
    row.append(&text);

    if let Some((text, class)) = badge {
        let pill = gtk::Label::new(Some(text));
        pill.add_css_class("pill");
        pill.add_css_class(class);
        pill.set_valign(gtk::Align::Center);
        row.append(&pill);
    }

    action.set_valign(gtk::Align::Center);
    row.append(action);
    row
}

// ===================== Stage 1: Source =====================

/// The spinner + dim caption shown while a file is being probed.
fn probing_row(text: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.append(&adw::Spinner::new());
    row.append(&dim_label(text));
    row
}

pub fn stage_source(ctx: &Rc<Ctx>) -> gtk::Widget {
    let s = ctx.state.borrow();

    if s.source.is_none() && !s.src_probing && s.src_error.is_none() {
        drop(s);
        return page(&drop_zone(
            ctx,
            "video-reel-symbolic",
            "Select your HDR10 source",
            "The master file to copy HDR10 metadata from. Drag a video here, or browse.",
            true,
        ));
    }

    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // Source file card (or probing/error placeholders).
    let header = section_label("Source file");
    header.set_margin_bottom(9);
    col.append(&header);

    if s.src_probing {
        col.append(&probing_row("Reading HDR10 metadata…"));
        drop(s);
        return page(&col);
    }

    if let Some(err) = &s.src_error {
        col.append(&banner_titled(
            "red",
            "dialog-error-symbolic",
            "Probe failed",
            err,
        ));
        let retry = gtk::Button::with_label("Choose another file");
        retry.set_halign(gtk::Align::Start);
        retry.set_margin_top(12);
        {
            let ctx = ctx.clone();
            retry.connect_clicked(move |_| app::pick_and_probe(&ctx, true));
        }
        col.append(&retry);
        drop(s);
        return page(&col);
    }

    let info = s.source.as_ref().expect("source probed");
    let detected = info.hdr.to_metadata();
    let effective = s.effective().unwrap_or(detected);
    let any_override = effective != detected;
    let name = uimodel::file_name(&info.path.display().to_string());

    let change = gtk::Button::with_label("Change");
    {
        let ctx = ctx.clone();
        change.connect_clicked(move |_| app::pick_and_probe(&ctx, true));
    }
    let src_card = card();
    src_card.append(&card_row(&file_row(
        "video-reel-symbolic",
        "",
        &name,
        &info.detail(),
        None,
        &change,
    )));
    app::attach_file_drop(ctx, &src_card, |ctx, path| {
        app::start_probe(ctx, path, true)
    });
    col.append(&src_card);

    // "HDR10 metadata" header with optional Reset all.
    let meta_header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    meta_header.set_margin_top(22);
    meta_header.set_margin_bottom(9);
    let meta_title = section_label("HDR10 metadata");
    meta_title.set_hexpand(true);
    meta_header.append(&meta_title);
    if any_override {
        let reset = gtk::Button::with_label("Reset all");
        reset.add_css_class("flat");
        {
            let ctx = ctx.clone();
            reset.connect_clicked(move |_| {
                {
                    let mut st = ctx.state.borrow_mut();
                    st.overrides = None;
                    st.editing = None;
                }
                app::render(&ctx);
            });
        }
        meta_header.append(&reset);
    }
    col.append(&meta_header);

    // The five editable rows.
    let rows = card();
    for row in 0..META_ROWS.len() {
        rows.append(&card_row(&meta_row(ctx, row, &detected, &effective)));
    }
    col.append(&rows);

    // Status banner.
    let b = if any_override {
        banner_titled(
            "amber",
            "dialog-warning-symbolic",
            "Manual overrides active",
            "These values will be written instead of the detected ones.",
        )
    } else if info.hdr.has_hdr10() {
        banner_titled(
            "green",
            "object-select-symbolic",
            "Valid HDR10 found",
            "Ready to copy into your encode.",
        )
    } else {
        banner_titled(
            "amber",
            "dialog-warning-symbolic",
            "No HDR10 signalling detected",
            "This file doesn't look like an HDR10 master; the detected values can still be edited above.",
        )
    };
    col.append(&b);

    drop(s);
    page(&col)
}

/// One metadata row: label · (edited pill) · value · edit button, plus the open editor.
fn meta_row(
    ctx: &Rc<Ctx>,
    row: usize,
    detected: &Hdr10Metadata,
    effective: &Hdr10Metadata,
) -> gtk::Box {
    let s = ctx.state.borrow();
    let overridden = uimodel::row_overridden(row, effective, detected);
    let editing = s
        .editing
        .as_ref()
        .filter(|(r, _)| *r == row)
        .map(|(_, d)| *d);
    drop(s);

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(9)
        .margin_bottom(9)
        .margin_start(16)
        .margin_end(9)
        .build();

    header.append(&gtk::Label::new(Some(META_ROWS[row])));
    if overridden {
        let pill = gtk::Label::new(Some("EDITED"));
        pill.add_css_class("edited-pill");
        pill.set_valign(gtk::Align::Center);
        header.append(&pill);
    }
    let value = gtk::Label::builder()
        .label(uimodel::row_value(row, effective))
        .halign(gtk::Align::End)
        .hexpand(true)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    value.add_css_class(if overridden {
        "value-accent"
    } else {
        "dim-label"
    });
    header.append(&value);

    if editing.is_none() {
        let edit = gtk::Button::from_icon_name("document-edit-symbolic");
        edit.set_tooltip_text(Some("Edit raw value"));
        edit.set_valign(gtk::Align::Center);
        {
            let ctx = ctx.clone();
            let eff = *effective;
            edit.connect_clicked(move |_| {
                ctx.state.borrow_mut().editing = Some((row, Draft::from_meta(&eff)));
                app::render(&ctx);
            });
        }
        header.append(&edit);
    }
    outer.append(&header);

    if let Some(draft) = editing {
        outer.append(&row_editor(ctx, row, draft, detected, overridden));
    }
    outer
}

/// The expanded "Raw HEVC values" editor under a row.
fn row_editor(
    ctx: &Rc<Ctx>,
    row: usize,
    draft: Draft,
    detected: &Hdr10Metadata,
    overridden: bool,
) -> gtk::Box {
    let area = gtk::Box::new(gtk::Orientation::Vertical, 9);
    area.add_css_class("editor-area");

    let caption = gtk::Label::builder()
        .label("RAW HEVC VALUES")
        .halign(gtk::Align::Start)
        .build();
    caption.add_css_class("caption-upper");
    area.append(&caption);

    // A grid of labelled fields.
    let fields = gtk::Grid::builder()
        .column_spacing(10)
        .row_spacing(10)
        .column_homogeneous(true)
        .build();

    // Helpers to mutate the open draft without re-rendering (focus must survive typing).
    type FieldSetter = Box<dyn Fn(&mut Draft, u32)>;
    let set_field = |ctx: &Rc<Ctx>, f: FieldSetter| {
        let ctx = ctx.clone();
        move |v: u32| {
            if let Some((_, d)) = ctx.state.borrow_mut().editing.as_mut() {
                f(d, v);
            }
        }
    };
    fn labelled(label: &str, w: &impl IsA<gtk::Widget>) -> gtk::Box {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let l = gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .build();
        l.add_css_class("caption");
        l.add_css_class("dim-label");
        b.append(&l);
        b.append(w);
        b
    }
    let number_entry = |initial: u32, on_change: Box<dyn Fn(u32)>| -> gtk::Entry {
        // Keep the minimum width small so a 4-across grid of these still fits the
        // 560px content clamp (a default GtkEntry would force the card much wider).
        let e = gtk::Entry::builder()
            .text(initial.to_string())
            .input_purpose(gtk::InputPurpose::Digits)
            .width_chars(5)
            .hexpand(true)
            .build();
        e.connect_changed(move |e| {
            if let Ok(v) = e.text().trim().parse::<u32>() {
                on_change(v);
            }
        });
        e
    };
    let enum_dropdown = |opts: &'static [u8],
                         current: u8,
                         namer: fn(u8) -> &'static str,
                         on_change: Box<dyn Fn(u32)>|
     -> gtk::DropDown {
        let labels: Vec<String> = opts.iter().map(|&c| code_label(c, namer(c))).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let dd = gtk::DropDown::from_strings(&refs);
        // The closed button ellipsizes long names ("16 · PQ (SMPTE ST 2084)") so three
        // dropdowns fit side by side; the opened list shows full text plus the checkmark.
        dd.set_factory(Some(&ellipsized_button_factory()));
        dd.set_list_factory(Some(&checked_list_factory(&dd)));
        dd.set_hexpand(true);
        dd.set_selected(opts.iter().position(|&c| c == current).unwrap_or(0) as u32);
        dd.connect_selected_notify(move |dd| {
            if let Some(&code) = opts.get(dd.selected() as usize) {
                on_change(code as u32);
            }
        });
        dd
    };

    match row {
        0 => {
            let p = enum_dropdown(
                &PRIMARIES_OPTS,
                draft.primaries,
                naming::primaries_name,
                Box::new(set_field(ctx, Box::new(|d, v| d.primaries = v as u8))),
            );
            let t = enum_dropdown(
                &TRANSFER_OPTS,
                draft.transfer,
                naming::transfer_name,
                Box::new(set_field(ctx, Box::new(|d, v| d.transfer = v as u8))),
            );
            let m = enum_dropdown(
                &MATRIX_OPTS,
                draft.matrix,
                naming::matrix_name,
                Box::new(set_field(ctx, Box::new(|d, v| d.matrix = v as u8))),
            );
            fields.attach(&labelled("Colour primaries", &p), 0, 0, 1, 1);
            fields.attach(&labelled("Transfer (EOTF)", &t), 1, 0, 1, 1);
            fields.attach(&labelled("Matrix coefficients", &m), 2, 0, 1, 1);
        }
        1 => {
            let dd = gtk::DropDown::from_strings(&["0 · Limited (TV)", "1 · Full (PC)"]);
            dd.set_selected(if draft.full_range { 1 } else { 0 });
            {
                let ctx = ctx.clone();
                dd.connect_selected_notify(move |dd| {
                    if let Some((_, d)) = ctx.state.borrow_mut().editing.as_mut() {
                        d.full_range = dd.selected() == 1;
                    }
                });
            }
            fields.attach(&labelled("video_full_range_flag", &dd), 0, 0, 1, 1);
        }
        2 => {
            type CellDef = (&'static str, u16, fn(&mut Draft, u32));
            let cells: [CellDef; 8] = [
                ("Green x", draft.gx, |d, v| d.gx = v as u16),
                ("Green y", draft.gy, |d, v| d.gy = v as u16),
                ("Blue x", draft.bx, |d, v| d.bx = v as u16),
                ("Blue y", draft.by, |d, v| d.by = v as u16),
                ("Red x", draft.rx, |d, v| d.rx = v as u16),
                ("Red y", draft.ry, |d, v| d.ry = v as u16),
                ("White x", draft.wx, |d, v| d.wx = v as u16),
                ("White y", draft.wy, |d, v| d.wy = v as u16),
            ];
            for (i, (label, value, setter)) in cells.into_iter().enumerate() {
                let e = number_entry(value as u32, Box::new(set_field(ctx, Box::new(setter))));
                fields.attach(&labelled(label, &e), (i % 4) as i32, (i / 4) as i32, 1, 1);
            }
        }
        3 => {
            let max = number_entry(
                draft.max_lum,
                Box::new(set_field(ctx, Box::new(|d, v| d.max_lum = v))),
            );
            let min = number_entry(
                draft.min_lum,
                Box::new(set_field(ctx, Box::new(|d, v| d.min_lum = v))),
            );
            fields.attach(&labelled("max_luminance (0.0001 cd/m²)", &max), 0, 0, 1, 1);
            fields.attach(&labelled("min_luminance (0.0001 cd/m²)", &min), 1, 0, 1, 1);
        }
        _ => {
            let cll = number_entry(
                draft.max_cll as u32,
                Box::new(set_field(ctx, Box::new(|d, v| d.max_cll = v as u16))),
            );
            let fall = number_entry(
                draft.max_fall as u32,
                Box::new(set_field(ctx, Box::new(|d, v| d.max_fall = v as u16))),
            );
            fields.attach(&labelled("MaxCLL (cd/m²)", &cll), 0, 0, 1, 1);
            fields.attach(&labelled("MaxFALL (cd/m²)", &fall), 1, 0, 1, 1);
        }
    }
    area.append(&fields);

    // Reset-to-detected · Cancel · Apply.
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    buttons.set_margin_top(4);
    if overridden {
        let reset = gtk::Button::with_label("Reset to detected");
        reset.add_css_class("flat");
        reset.set_hexpand(true);
        reset.set_halign(gtk::Align::Start);
        {
            let ctx = ctx.clone();
            let det = *detected;
            reset.connect_clicked(move |_| {
                {
                    let mut st = ctx.state.borrow_mut();
                    if let Some(current) = st.overrides {
                        st.overrides = Some(Draft::from_meta(&det).apply_row(row, current));
                    }
                    st.editing = None;
                }
                app::render(&ctx);
            });
        }
        buttons.append(&reset);
    } else {
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        buttons.append(&spacer);
    }
    let cancel = gtk::Button::with_label("Cancel");
    {
        let ctx = ctx.clone();
        cancel.connect_clicked(move |_| {
            ctx.state.borrow_mut().editing = None;
            app::render(&ctx);
        });
    }
    buttons.append(&cancel);
    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    {
        let ctx = ctx.clone();
        apply.connect_clicked(move |_| {
            {
                let mut st = ctx.state.borrow_mut();
                if let Some((r, d)) = st.editing.take() {
                    let base = st
                        .overrides
                        .or_else(|| st.source.as_ref().map(|f| f.hdr.to_metadata()));
                    if let Some(base) = base {
                        st.overrides = Some(d.apply_row(r, base));
                    }
                }
            }
            app::render(&ctx);
        });
    }
    buttons.append(&apply);
    area.append(&buttons);
    area
}

// ===================== Stage 2: Encode =====================

pub fn stage_encode(ctx: &Rc<Ctx>) -> gtk::Widget {
    // While processing, `render` shows the persistent [`ProcessingUi`] instead.
    let s = ctx.state.borrow();
    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // "Copying from …" banner: bold title with the colour summary alongside, mirroring the
    // WinUI InfoBar (title/message) — no separator dot between the file name and the metadata.
    if let (Some(src), Some(meta)) = (&s.source, s.effective()) {
        let markup = format!(
            "<b>Copying from {}</b>  {}",
            gtk::glib::markup_escape_text(&uimodel::file_name(&src.path.display().to_string())),
            gtk::glib::markup_escape_text(&uimodel::colour_summary(&meta)),
        );
        let b = banner_markup("blue", "video-reel-symbolic", &markup);
        b.set_margin_top(0);
        b.set_margin_bottom(22);
        col.append(&b);
    }

    if s.encode.is_none() && !s.enc_probing && s.enc_error.is_none() {
        drop(s);
        // Our bundled "arrow-into-box" glyph (registered in app.rs) — an arrow pointing into a
        // box, i.e. dropping the encode in to be patched.
        col.append(&drop_zone(
            ctx,
            "arrow-into-box-symbolic",
            "Select the encode to patch",
            "The HEVC encode that's missing its HDR10 signalling. Drag a video here, or browse.",
            false,
        ));
        return page(&col);
    }

    if s.enc_probing {
        col.append(&probing_row("Reading encode metadata…"));
        drop(s);
        return page(&col);
    }

    if let Some(err) = &s.enc_error {
        col.append(&banner_titled(
            "red",
            "dialog-error-symbolic",
            "Probe failed",
            err,
        ));
        drop(s);
        return page(&col);
    }

    let enc = s.encode.as_ref().expect("encode probed");
    let name = uimodel::file_name(&enc.path.display().to_string());

    let header = section_label("Encode to patch");
    header.set_margin_bottom(9);
    col.append(&header);

    let change = gtk::Button::with_label("Change");
    {
        let ctx = ctx.clone();
        change.connect_clicked(move |_| app::pick_and_probe(&ctx, false));
    }
    let badge = if enc.hdr.has_hdr10() {
        Some(("HDR10 present", "green"))
    } else {
        Some(("No HDR10", "amber"))
    };
    let enc_card = card();
    enc_card.append(&card_row(&file_row(
        "dialog-warning-symbolic",
        "warn",
        &name,
        &enc.detail(),
        badge,
        &change,
    )));
    app::attach_file_drop(ctx, &enc_card, |ctx, path| {
        app::start_probe(ctx, path, false)
    });
    col.append(&enc_card);

    // Output card.
    let out_header = section_label("Output");
    out_header.set_margin_top(22);
    out_header.set_margin_bottom(9);
    col.append(&out_header);

    let out_card = card();
    let save_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(13)
        .margin_top(13)
        .margin_bottom(13)
        .margin_start(16)
        .margin_end(16)
        .build();
    let save_text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    save_text.set_hexpand(true);
    let save_title = gtk::Label::builder()
        .label("Save as")
        .halign(gtk::Align::Start)
        .build();
    save_title.add_css_class("heading");
    let save_path = gtk::Label::builder()
        .label(
            s.output
                .as_ref()
                .map(|p| uimodel::compact_path(&p.display().to_string()))
                .unwrap_or_default(),
        )
        .halign(gtk::Align::Start)
        .ellipsize(pango::EllipsizeMode::Middle)
        .build();
    save_path.add_css_class("dim-label");
    save_path.add_css_class("caption");
    save_text.append(&save_title);
    save_text.append(&save_path);
    save_row.append(&save_text);
    let choose = icon_button("Choose…", "document-open-symbolic", false);
    choose.set_valign(gtk::Align::Center);
    {
        let ctx = ctx.clone();
        let container = s.container;
        let initial = s
            .output
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("out.{}", container.extension()));
        choose.connect_clicked(move |_| {
            app::pick_output(&ctx, Some(&initial), |ctx, path| {
                {
                    let mut st = ctx.state.borrow_mut();
                    let (normalized, container, rewritten) =
                        uimodel::normalize_output_pick(&path.display().to_string(), st.container);
                    st.output = Some(PathBuf::from(normalized));
                    st.container = container;
                    // The save dialog warned about an existing file — but only for the exact
                    // name the user typed; a rewritten extension may collide unseen, so that
                    // case keeps our own overwrite confirmation.
                    st.output_user_chosen = !rewritten;
                }
                app::render(ctx);
            });
        });
    }
    save_row.append(&choose);
    out_card.append(&card_row(&save_row));

    col.append(&out_card);

    // A missing external tool blocks the run — say so here, before the user reaches the
    // (disabled) Patch button, instead of failing mid-run.
    let missing = app::missing_tools(&s);
    if !missing.is_empty() {
        let body = missing
            .iter()
            .map(|t| format!("{}: {}", t.label(), t.install_hint()))
            .collect::<Vec<_>>()
            .join("\n");
        col.append(&banner_titled(
            "red",
            "dialog-error-symbolic",
            "Missing tools for this patch",
            &body,
        ));
    }

    col.append(&banner_titled(
        "blue",
        "dialog-information-symbolic",
        "No re-encoding",
        match s.container {
            Container::Mkv => "Only the SPS VUI and an HDR10 SEI are written. All audio & subtitle tracks are carried over untouched.",
            Container::Mp4 => "Only the SPS VUI and an HDR10 SEI are written. MP4 can't hold every track type: audio is carried over untouched, text subtitles are converted, the rest is left out.",
        },
    ));

    if let Some(err) = &s.run_error {
        col.append(&banner_titled(
            "red",
            "dialog-error-symbolic",
            "Patch failed",
            err,
        ));
    }

    drop(s);
    page(&col)
}

/// The processing view: a spinner plus staged progress. Built once per run and updated
/// in place, so the spinner animation runs uninterrupted across stage changes.
pub struct ProcessingUi {
    pub root: gtk::Widget,
    spinner: adw::Spinner,
    check: gtk::Image,
    title: gtk::Label,
    bar: gtk::ProgressBar,
    step: gtk::Label,
    pct: gtk::Label,
}

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

const STAGE_LABELS: [&str; 6] = [
    "Probing source metadata",
    "Extracting HEVC stream",
    "Patching bitstream",
    "Capturing frame timing",
    "Remuxing tracks",
    "Done",
];

impl ProcessingUi {
    pub fn update(&self, stage: PipeStage) {
        let idx = stage_index(stage);
        // The Done stage is held on screen briefly (the worker sleeps before delivering the
        // result), so the finished state registers: checkmark in place of the spinner, 100%.
        let done = stage == PipeStage::Done;
        self.spinner.set_visible(!done);
        self.check.set_visible(done);
        self.title.set_label(STAGE_LABELS[idx]);
        self.bar.set_fraction(idx as f64 / 5.0);
        self.step.set_label(&format!("Stage {} of 6", idx + 1));
        self.pct
            .set_label(&format!("{}%", (idx as f64 / 5.0 * 100.0).round()));
    }
}

/// Builds the centered spinner + staged progress view for a fresh run.
pub fn build_processing(stage: PipeStage) -> ProcessingUi {
    let col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(5)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();

    let spinner = adw::Spinner::new();
    spinner.add_css_class("processing-spinner");
    spinner.set_halign(gtk::Align::Center);
    col.append(&spinner);

    // Swapped in for the spinner while the finished state is held (same footprint).
    let check = gtk::Image::from_icon_name("object-select-symbolic");
    check.add_css_class("processing-done-circle");
    check.set_pixel_size(28);
    check.set_halign(gtk::Align::Center);
    check.set_visible(false);
    col.append(&check);

    let title = gtk::Label::new(None);
    title.add_css_class("title-3");
    title.set_margin_top(24);
    col.append(&title);
    col.append(&{
        let l = dim_label("Lossless bitstream edit · timing preserved exactly");
        l.set_halign(gtk::Align::Center);
        l
    });

    let bar_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    bar_box.set_margin_top(22);
    bar_box.set_size_request(320, -1);
    let bar = gtk::ProgressBar::new();
    bar_box.append(&bar);
    let caption_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let step = dim_label("");
    step.add_css_class("caption");
    step.set_hexpand(true);
    let pct = dim_label("");
    pct.add_css_class("caption");
    caption_row.append(&step);
    caption_row.append(&pct);
    bar_box.append(&caption_row);
    col.append(&bar_box);

    let ui = ProcessingUi {
        root: col.upcast(),
        spinner,
        check,
        title,
        bar,
        step,
        pct,
    };
    ui.update(stage);
    ui
}

// ===================== Stage 3: Result =====================

pub fn stage_result(ctx: &Rc<Ctx>) -> gtk::Widget {
    let s = ctx.state.borrow();
    let Some(result) = s.result.clone() else {
        drop(s);
        return gtk::Box::new(gtk::Orientation::Vertical, 0).upcast();
    };

    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // Success hero.
    let hero = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(5)
        .halign(gtk::Align::Center)
        .margin_bottom(24)
        .build();
    // Sparkles on the accent tint: the green checkmark moved to the processing view's
    // completion hold, so the result hero celebrates instead of re-confirming.
    let spark = gtk::Image::from_icon_name("sparkles-symbolic");
    spark.add_css_class("accent-circle");
    spark.set_pixel_size(38);
    spark.set_halign(gtk::Align::Center);
    hero.append(&spark);
    let title = gtk::Label::new(Some("HDR10 metadata embedded"));
    title.add_css_class("title-2");
    title.set_margin_top(14);
    hero.append(&title);
    let sub = gtk::Label::builder()
        .label(
            "Your encode now carries the source's HDR10 signalling, losslessly, with every frame's timing preserved.",
        )
        .wrap(true)
        .justify(gtk::Justification::Center)
        .max_width_chars(50)
        .build();
    sub.add_css_class("dim-label");
    hero.append(&sub);
    col.append(&hero);

    // Output card with a Play button.
    let out_name = uimodel::file_name(&result.output.display().to_string());
    let size = std::fs::metadata(&result.output)
        .map(|m| m.len())
        .unwrap_or(0);
    let detail = format!(
        "{} · {} · {}",
        uimodel::compact_path(&result.output.display().to_string()),
        uimodel::human_size(size),
        result
            .output
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_uppercase)
            .unwrap_or_default()
    );
    let play = gtk::Button::from_icon_name("media-playback-start-symbolic");
    play.set_tooltip_text(Some("Play"));
    {
        let window = ctx.window.clone();
        let path = result.output.clone();
        play.connect_clicked(move |_| app::launch_file(&window, &path));
    }
    let out_card = card();
    out_card.append(&card_row(&file_row(
        "video-reel-symbolic",
        "", // default accent tint (not the green "ok" tint)
        &out_name,
        &detail,
        None,
        &play,
    )));
    col.append(&out_card);

    // Summary card.
    let sum_header = section_label("Summary");
    sum_header.set_margin_top(22);
    sum_header.set_margin_bottom(9);
    col.append(&sum_header);

    let summary = card();
    let add_row = |icon: &str, ok: bool, label: &str, value: &str| {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(13)
            .margin_top(13)
            .margin_bottom(13)
            .margin_start(16)
            .margin_end(16)
            .build();
        let image = gtk::Image::from_icon_name(icon);
        if ok {
            image.add_css_class("success");
        } else {
            image.add_css_class("accent");
        }
        row.append(&image);
        let l = gtk::Label::new(Some(label));
        l.set_hexpand(true);
        l.set_halign(gtk::Align::Start);
        row.append(&l);
        let v = dim_label(value);
        v.set_halign(gtk::Align::End);
        row.append(&v);
        summary.append(&card_row(&row));
    };

    add_row(
        "object-select-symbolic",
        true,
        "HDR10 SEI inserted",
        &match result.sei_insertions {
            0 => "already present".to_owned(),
            1 => "ST 2086 + MaxCLL/FALL".to_owned(),
            n => format!("ST 2086 + MaxCLL/FALL · {n} IRAPs"),
        },
    );
    add_row(
        "object-select-symbolic",
        true,
        "SPS VUI colour rewritten",
        &format!("{} SPS · bit-exact", result.sps_patched),
    );
    if let Some(meta) = s.effective() {
        add_row(
            "color-select-symbolic",
            false,
            "Colour",
            &uimodel::colour_summary(&meta),
        );
    }
    add_row(
        "alarm-symbolic",
        false,
        "Timing",
        &result
            .frames
            .map(|n| format!("{n} frames preserved exactly"))
            .unwrap_or_else(|| "preserved exactly".to_owned()),
    );
    add_row(
        "audio-x-generic-symbolic",
        false,
        "Tracks carried over",
        &result
            .tracks
            .as_ref()
            .map(uimodel::tracks_line)
            .unwrap_or_else(|| "audio & subtitles untouched".to_owned()),
    );
    col.append(&summary);

    // MP4 output can't hold every track type; list what was left behind.
    if !result.dropped_tracks.is_empty() {
        let warn = banner_titled(
            "amber",
            "dialog-warning-symbolic",
            "Some tracks were left out",
            &format!(
                "Not carried into MP4 (unsupported there): {}.\nChoose Matroska output to keep every track.",
                result.dropped_tracks.join(", ")
            ),
        );
        warn.set_margin_top(16);
        col.append(&warn);
    }

    drop(s);
    page(&col)
}

// ===================== Footer bar =====================

pub fn build_footer(ctx: &Rc<Ctx>, footer: &gtk::Box) {
    let s = ctx.state.borrow();
    match s.stage {
        1 => {
            let hint = dim_label(if s.source.is_none() {
                "Pick your HDR10 master to continue"
            } else {
                ""
            });
            hint.add_css_class("caption");
            hint.set_hexpand(true);
            hint.set_valign(gtk::Align::Center);
            footer.append(&hint);

            let cont = icon_button("Continue", "go-next-symbolic", true);
            cont.add_css_class("suggested-action");
            cont.set_sensitive(s.source.is_some());
            {
                let ctx = ctx.clone();
                cont.connect_clicked(move |_| {
                    {
                        let mut st = ctx.state.borrow_mut();
                        if st.source.is_none() {
                            return;
                        }
                        st.stage = 2;
                    }
                    app::render(&ctx);
                });
            }
            footer.append(&cont);
        }
        2 => {
            let hint = dim_label(if s.encode.is_none() {
                "Select the encode that needs HDR10"
            } else {
                ""
            });
            hint.add_css_class("caption");
            hint.set_hexpand(true);
            hint.set_valign(gtk::Align::Center);
            footer.append(&hint);

            let run = icon_button("Patch HDR10", "starred-symbolic", false);
            run.add_css_class("suggested-action");
            run.set_sensitive(
                s.encode.is_some() && s.output.is_some() && app::missing_tools(&s).is_empty(),
            );
            run.set_visible(s.encode.is_some());
            {
                let ctx = ctx.clone();
                run.connect_clicked(move |_| app::confirm_and_run(&ctx));
            }
            footer.append(&run);
        }
        _ => {
            let another = icon_button("Patch another", "view-refresh-symbolic", false);
            another.add_css_class("flat");
            {
                let ctx = ctx.clone();
                another.connect_clicked(move |_| {
                    ctx.state.borrow_mut().reset_wizard();
                    app::render(&ctx);
                });
            }
            another.set_hexpand(true);
            another.set_halign(gtk::Align::Start);
            footer.append(&another);

            if let Some(result) = &s.result {
                let open = icon_button("Open output folder", "folder-open-symbolic", false);
                open.add_css_class("suggested-action");
                let window = ctx.window.clone();
                let path = result.output.clone();
                open.connect_clicked(move |_| app::open_containing_folder(&window, &path));
                footer.append(&open);
            }
        }
    }
}
