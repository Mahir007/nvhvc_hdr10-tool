//! The three wizard stages and the footer bar, rebuilt from state on every render.

use std::rc::Rc;

use adw::prelude::*;
use gtk::pango;

use nvhvc_core::hevc::patch::Hdr10Metadata;
use nvhvc_core::naming;
use nvhvc_core::pipeline::Stage as PipeStage;
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

/// Tinted rounded banner with an icon and a (plain-text) label.
fn banner(kind: &str, icon: &str, text: &str) -> gtk::Box {
    banner_markup(kind, icon, &gtk::glib::markup_escape_text(text))
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

/// A list-item factory rendering a string label; `ellipsize` for `GtkDropDown` buttons
/// (the popup list keeps a full-width factory so options stay readable).
fn label_factory(ellipsize: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::builder().halign(gtk::Align::Start).build();
        if ellipsize {
            // Small minimum + capped natural width so three dropdowns fit the 560px
            // clamp; longer names ellipsize but stay readable.
            label.set_ellipsize(pango::EllipsizeMode::End);
            label.set_width_chars(6);
            label.set_max_width_chars(12);
            label.set_hexpand(true);
        }
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

/// A button whose content is an icon next to (or after) its label, per the mockup.
fn icon_button(label: &str, icon: &str, icon_after: bool) -> gtk::Button {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    row.set_halign(gtk::Align::Center);
    let image = gtk::Image::from_icon_name(icon);
    let text = gtk::Label::new(Some(label));
    if icon_after {
        row.append(&text);
        row.append(&image);
    } else {
        row.append(&image);
        row.append(&text);
    }
    gtk::Button::builder().child(&row).build()
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
        pill.connect_clicked(move |_| {
            let title = if is_source {
                "Choose HDR10 source"
            } else {
                "Choose the encode to patch"
            };
            app::pick_file(&ctx, false, title, None, move |ctx, path| {
                app::start_probe(ctx, path, is_source);
            });
        });
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

pub fn stage_source(ctx: &Rc<Ctx>) -> gtk::Widget {
    let s = ctx.state.borrow();

    if s.source.is_none() && !s.src_probing && s.src_error.is_none() {
        drop(s);
        return page(&drop_zone(
            ctx,
            "video-x-generic-symbolic",
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
        let probing = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        probing.append(&spinner);
        probing.append(&dim_label("Reading HDR10 metadata…"));
        col.append(&probing);
        drop(s);
        return page(&col);
    }

    if let Some(err) = &s.src_error {
        col.append(&banner(
            "red",
            "dialog-error-symbolic",
            &format!("Probe failed — {err}"),
        ));
        let retry = gtk::Button::with_label("Choose another file");
        retry.set_halign(gtk::Align::Start);
        retry.set_margin_top(12);
        {
            let ctx = ctx.clone();
            retry.connect_clicked(move |_| {
                app::pick_file(&ctx, false, "Choose HDR10 source", None, |ctx, path| {
                    app::start_probe(ctx, path, true);
                });
            });
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
        change.connect_clicked(move |_| {
            app::pick_file(&ctx, false, "Choose HDR10 source", None, |ctx, path| {
                app::start_probe(ctx, path, true);
            });
        });
    }
    let src_card = card();
    src_card.append(&card_row(&file_row(
        "video-x-generic-symbolic",
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
        banner(
            "amber",
            "dialog-warning-symbolic",
            "Manual overrides active — these values will be written instead of the detected ones.",
        )
    } else if info.hdr.has_hdr10() {
        banner(
            "green",
            "object-select-symbolic",
            "Valid HDR10 found — ready to copy into your encode.",
        )
    } else {
        banner(
            "amber",
            "dialog-warning-symbolic",
            "No HDR10 mastering-display metadata detected — the values above can still be edited.",
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
        // dropdowns fit side by side; the opened list still shows the full text.
        dd.set_factory(Some(&label_factory(true)));
        dd.set_list_factory(Some(&label_factory(false)));
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
    let s = ctx.state.borrow();

    // Processing view replaces the whole stage.
    if let Some(stage) = s.processing {
        drop(s);
        return processing_view(stage);
    }

    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // "Copying from …" banner (bold accent filename, per the mockup).
    if let (Some(src), Some(meta)) = (&s.source, s.effective()) {
        let markup = format!(
            "Copying from <b>{}</b> · {}",
            gtk::glib::markup_escape_text(&uimodel::file_name(&src.path.display().to_string())),
            gtk::glib::markup_escape_text(&uimodel::colour_summary(&meta)),
        );
        let b = banner_markup("blue", "video-x-generic-symbolic", &markup);
        b.set_margin_top(0);
        b.set_margin_bottom(22);
        col.append(&b);
    }

    if s.encode.is_none() && !s.enc_probing && s.enc_error.is_none() {
        drop(s);
        col.append(&drop_zone(
            ctx,
            "go-up-symbolic",
            "Select the encode to patch",
            "The HEVC encode that's missing its HDR10 signalling. Drag a video here, or browse.",
            false,
        ));
        return page(&col);
    }

    if s.enc_probing {
        let probing = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        probing.append(&spinner);
        probing.append(&dim_label("Reading encode metadata…"));
        col.append(&probing);
        drop(s);
        return page(&col);
    }

    if let Some(err) = &s.enc_error {
        col.append(&banner(
            "red",
            "dialog-error-symbolic",
            &format!("Probe failed — {err}"),
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
        change.connect_clicked(move |_| {
            app::pick_file(
                &ctx,
                false,
                "Choose the encode to patch",
                None,
                |ctx, path| {
                    app::start_probe(ctx, path, false);
                },
            );
        });
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
                .map(|p| p.display().to_string())
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
        let initial = s
            .output
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "out.mkv".to_owned());
        choose.connect_clicked(move |_| {
            app::pick_file(
                &ctx,
                true,
                "Choose output file",
                Some(&initial),
                |ctx, path| {
                    ctx.state.borrow_mut().output = Some(path);
                    app::render(ctx);
                },
            );
        });
    }
    save_row.append(&choose);
    out_card.append(&card_row(&save_row));

    let container_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .margin_top(13)
        .margin_bottom(13)
        .margin_start(16)
        .margin_end(16)
        .build();
    let c_label = gtk::Label::new(Some("Container"));
    c_label.set_hexpand(true);
    c_label.set_halign(gtk::Align::Start);
    container_row.append(&c_label);
    container_row.append(&dim_label("Matroska (.mkv)"));
    out_card.append(&card_row(&container_row));
    col.append(&out_card);

    col.append(&banner(
        "blue",
        "dialog-information-symbolic",
        "No re-encoding — only the SPS VUI and an HDR10 SEI are written.\nAll audio & subtitle tracks are carried over untouched.",
    ));

    if let Some(err) = &s.run_error {
        col.append(&banner(
            "red",
            "dialog-error-symbolic",
            &format!("Patch failed — {err}"),
        ));
    }

    drop(s);
    page(&col)
}

/// Centered spinner + staged progress bar while the pipeline runs.
fn processing_view(stage: PipeStage) -> gtk::Widget {
    let idx = match stage {
        PipeStage::Probing => 0,
        PipeStage::Extracting => 1,
        PipeStage::Patching => 2,
        PipeStage::CapturingTiming => 3,
        PipeStage::Remuxing => 4,
        PipeStage::Done => 5,
    };
    let labels = [
        "Probing source metadata",
        "Extracting HEVC stream",
        "Patching bitstream",
        "Capturing frame timing",
        "Remuxing tracks",
        "Finishing up",
    ];

    let col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(5)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();

    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.add_css_class("processing-spinner");
    spinner.set_halign(gtk::Align::Center);
    col.append(&spinner);

    let title = gtk::Label::new(Some(labels[idx]));
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
    bar.set_fraction(idx as f64 / 5.0);
    bar_box.append(&bar);
    let caption_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let step = dim_label(&format!("Stage {} of 6", idx + 1));
    step.add_css_class("caption");
    step.set_hexpand(true);
    let pct = dim_label(&format!("{}%", (idx as f64 / 5.0 * 100.0).round()));
    pct.add_css_class("caption");
    caption_row.append(&step);
    caption_row.append(&pct);
    bar_box.append(&caption_row);
    col.append(&bar_box);

    col.upcast()
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
    let check = gtk::Image::from_icon_name("object-select-symbolic");
    check.add_css_class("success-circle");
    check.set_pixel_size(34);
    check.set_halign(gtk::Align::Center);
    hero.append(&check);
    let title = gtk::Label::new(Some("HDR10 metadata embedded"));
    title.add_css_class("title-2");
    title.set_margin_top(14);
    hero.append(&title);
    let sub = gtk::Label::builder()
        .label(
            "Your encode now carries the source's HDR10 signalling — losslessly, with every frame's timing preserved.",
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
        "{} · {} · MKV",
        result.output.display(),
        uimodel::human_size(size)
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
        "video-x-generic-symbolic",
        "ok",
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
        if result.sei_inserted {
            "ST 2086 + MaxCLL/FALL"
        } else {
            "already present"
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
            run.set_sensitive(s.encode.is_some() && s.output.is_some());
            run.set_visible(s.encode.is_some());
            {
                let ctx = ctx.clone();
                run.connect_clicked(move |_| app::start_run(&ctx));
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
