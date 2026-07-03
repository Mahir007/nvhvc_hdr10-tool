//! Fluent views: title bar, breadcrumb, the three wizard stages, and the Preferences overlay.
//!
//! Layout and styling follow the design mockup ("HDR10 Patcher (WinUI 3)"): Mica window,
//! card sections on `CardBackground`/`CardStroke`, accent primary buttons, `InfoBar` banners,
//! and a centred 560px content column per stage.

use windows_reactor::*;

use nvhvc_core::config::Settings;
use nvhvc_core::hevc::patch::Hdr10Metadata;
use nvhvc_core::naming;
use nvhvc_core::tools::{Resolution, Tool, Toolset};

use crate::model::*;
use crate::{Vm, fire_probe};

const ICON_FONT: &str = "Segoe Fluent Icons";

// Segoe Fluent Icons glyphs.
const GLYPH_CHECK: &str = "\u{E73E}";
const GLYPH_CHEVRON: &str = "\u{E76C}";
const GLYPH_MORE: &str = "\u{E712}";
const GLYPH_VIDEO: &str = "\u{E714}";
const GLYPH_UPLOAD: &str = "\u{E898}";
const GLYPH_COLOR: &str = "\u{E790}";
const GLYPH_CLOCK: &str = "\u{E823}";
const GLYPH_AUDIO: &str = "\u{E8D6}";

/// An icon glyph as a text block.
fn glyph(g: &'static str, size: f64) -> TextBlock {
    text_block(g).font_family(ICON_FONT).font_size(size)
}

/// A Fluent card: 8px corners, hairline stroke, card fill.
fn card(child: impl Into<Element>) -> Element {
    border(child)
        .corner_radius(8.0)
        .border_brush(ThemeRef::CardStroke)
        .border_thickness(Thickness::uniform(1.0))
        .background(ThemeRef::CardBackground)
        .into()
}

/// A 1px horizontal divider between card rows.
fn divider() -> Element {
    border(text_block(""))
        .border_thickness(Thickness::uniform(0.0))
        .height(1.0)
        .background(ThemeRef::DividerStroke)
        .into()
}

/// A bold section heading above a card.
fn section(title: &str) -> Element {
    body_strong(title)
        .margin(Thickness {
            left: 2.0,
            top: 22.0,
            right: 2.0,
            bottom: 9.0,
        })
        .into()
}

/// The 40×40 rounded icon block at the left of a file card.
fn icon_block(g: &'static str) -> Element {
    border(
        glyph(g, 20.0)
            .foreground(ThemeRef::AccentText)
            .horizontal_alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Center),
    )
    .corner_radius(5.0)
    .background(ThemeRef::SubtleFill)
    .width(40.0)
    .height(40.0)
    .into()
}

/// The scrollable, centred 560px content column of a stage.
fn page(children: Vec<Element>) -> Element {
    scroll_viewer(
        vstack(children)
            .spacing(0.0)
            .max_width(560.0)
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .margin(Thickness {
                left: 30.0,
                top: 26.0,
                right: 30.0,
                bottom: 26.0,
            }),
    )
    .into()
}

// ===================== Chrome =====================

/// The window title bar: app title, back button, and the "…" app menu.
pub fn title_bar(vm: &Vm, shown: Stage, processing: bool) -> Element {
    let check = |on: bool| if on { "  ✓" } else { "" };
    let items = vec![
        menu_item(format!(
            "Theme: System{}",
            check(vm.ui.theme == ThemeMode::System)
        )),
        menu_item(format!(
            "Theme: Light{}",
            check(vm.ui.theme == ThemeMode::Light)
        )),
        menu_item(format!(
            "Theme: Dark{}",
            check(vm.ui.theme == ThemeMode::Dark)
        )),
        menu_separator(),
        menu_item("Preferences"),
        menu_item("About nvhvc"),
    ];
    let upd = vm.upd.clone();
    let menu = button(GLYPH_MORE)
        .subtle()
        .menu_flyout(items)
        .on_item_clicked(move |item: String| {
            let upd = upd.clone();
            if item.starts_with("Theme:") {
                let mode = if item.contains("Light") {
                    ThemeMode::Light
                } else if item.contains("Dark") {
                    ThemeMode::Dark
                } else {
                    ThemeMode::System
                };
                apply_theme(mode);
                upd.call(move |mut s| {
                    s.theme = mode;
                    s
                });
            } else if item.starts_with("Preferences") {
                upd.call(|mut s| {
                    s.prefs_open = true;
                    s
                });
            } else if item.starts_with("About") {
                upd.call(|mut s| {
                    s.about_open = true;
                    s
                });
            }
        })
        .font_family(ICON_FONT)
        .font_size(14.0)
        .width(40.0)
        .height(34.0)
        .padding(0.0);

    let upd = vm.upd.clone();
    let run_token = vm.ui.run_token;
    let showing_result = shown == Stage::Result;
    TitleBar::new("HDR10 Patcher")
        .back_button_visible(shown != Stage::Source && !processing)
        .back_button_enabled(true)
        .on_back_requested(move || {
            upd.call(move |mut s| {
                if showing_result {
                    // Back from the result screen dismisses it (returns to the encode screen).
                    s.ack_token = run_token;
                } else {
                    s.stage = Stage::Source;
                }
                s
            });
        })
        .footer(menu)
        .into()
}

/// Applies the persisted theme at startup (without re-saving it).
pub fn apply_startup_theme() {
    let mode = ThemeMode::from_setting(Settings::load().ui_theme.as_deref());
    set_requested_theme(match mode {
        ThemeMode::System => RequestedTheme::Default,
        ThemeMode::Light => RequestedTheme::Light,
        ThemeMode::Dark => RequestedTheme::Dark,
    });
}

/// Applies and persists a theme choice.
pub fn apply_theme(mode: ThemeMode) {
    set_requested_theme(match mode {
        ThemeMode::System => RequestedTheme::Default,
        ThemeMode::Light => RequestedTheme::Light,
        ThemeMode::Dark => RequestedTheme::Dark,
    });
    let mut settings = Settings::load();
    settings.ui_theme = Some(mode.as_setting().to_owned());
    let _ = settings.save();
}

/// The "Source › Encode › Result" step indicator under the title bar.
pub fn breadcrumb(shown: Stage) -> Element {
    let state = |step: Stage| -> u8 {
        use Stage::*;
        let idx = |s: Stage| match s {
            Source => 0,
            Encode => 1,
            Result => 2,
        };
        match idx(shown).cmp(&idx(step)) {
            std::cmp::Ordering::Greater => 2, // done
            std::cmp::Ordering::Equal => 1,   // active
            std::cmp::Ordering::Less => 0,    // upcoming
        }
    };
    let crumb = |label: &str, st: u8| -> Element {
        let mut kids: Vec<Element> = Vec::new();
        if st == 2 {
            kids.push(
                glyph(GLYPH_CHECK, 12.0)
                    .foreground(ThemeRef::AccentText)
                    .vertical_alignment(VerticalAlignment::Center)
                    .with_key("check")
                    .into(),
            );
        }
        let text = text_block(label).font_size(14.0);
        let text = match st {
            1 => text.semibold().foreground(ThemeRef::PrimaryText),
            2 => text.foreground(ThemeRef::PrimaryText),
            _ => text.foreground(ThemeRef::TertiaryText),
        };
        kids.push(text.with_key("label").into());
        hstack(kids).spacing(6.0).into()
    };
    let sep = || -> Element {
        glyph(GLYPH_CHEVRON, 11.0)
            .foreground(ThemeRef::TertiaryText)
            .vertical_alignment(VerticalAlignment::Center)
            .into()
    };
    border(
        hstack((
            crumb("Source", state(Stage::Source)),
            sep(),
            crumb("Encode", state(Stage::Encode)),
            sep(),
            crumb("Result", state(Stage::Result)),
        ))
        .spacing(8.0)
        .horizontal_alignment(HorizontalAlignment::Center),
    )
    .border_thickness(Thickness {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 1.0,
    })
    .border_brush(ThemeRef::DividerStroke)
    .padding(Thickness {
        left: 24.0,
        top: 12.0,
        right: 24.0,
        bottom: 12.0,
    })
    .into()
}

/// The footer command bar: hint text on the left, primary action on the right.
fn footer(hint: &str, action: Option<Element>) -> Element {
    let mut cells: Vec<Element> = vec![
        caption(hint)
            .foreground(ThemeRef::TertiaryText)
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(0)
            .with_key("hint")
            .into(),
    ];
    if let Some(a) = action {
        cells.push(a.grid_column(1).with_key("action"));
    }
    border(
        grid(cells)
            .columns([GridLength::Star(1.0), GridLength::Auto])
            .padding(Thickness {
                left: 22.0,
                top: 13.0,
                right: 22.0,
                bottom: 13.0,
            }),
    )
    .border_thickness(Thickness {
        left: 0.0,
        top: 1.0,
        right: 0.0,
        bottom: 0.0,
    })
    .border_brush(ThemeRef::DividerStroke)
    .background(ThemeRef::LayerFill)
    .into()
}

// ===================== Drop zone =====================

/// A large "drop a file here or browse" zone with a native-picker button.
fn drop_zone(
    icon: &'static str,
    heading: &str,
    caption_text: &str,
    on_pick: impl Fn() + 'static,
    set_path: AsyncSetState<String>,
    probe: MutationTrigger<FileProbe>,
    set_output: Option<AsyncSetState<String>>,
) -> Element {
    let inner = vstack((
        border(
            glyph(icon, 30.0)
                .foreground(ThemeRef::AccentText)
                .horizontal_alignment(HorizontalAlignment::Center)
                .vertical_alignment(VerticalAlignment::Center),
        )
        .corner_radius(8.0)
        .background(ThemeRef::SubtleFill)
        .width(66.0)
        .height(66.0)
        .horizontal_alignment(HorizontalAlignment::Center)
        .margin(Thickness {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 4.0,
        }),
        text_block(heading)
            .font_size(20.0)
            .semibold()
            .horizontal_alignment(HorizontalAlignment::Center),
        text_block(caption_text)
            .font_size(13.0)
            .foreground(ThemeRef::SecondaryText)
            .wrap()
            .max_width(380.0)
            .horizontal_alignment(HorizontalAlignment::Center),
        button("Open File\u{2026}")
            .accent()
            .icon(Symbol::OpenFile)
            .on_click(on_pick)
            .horizontal_alignment(HorizontalAlignment::Center)
            .margin(Thickness {
                left: 0.0,
                top: 12.0,
                right: 0.0,
                bottom: 0.0,
            }),
    ))
    .spacing(8.0);

    border(inner)
        .corner_radius(8.0)
        .border_brush(ThemeRef::ControlStrokeSecondary)
        .border_thickness(Thickness::uniform(1.5))
        .background(ThemeRef::CardBackground)
        .padding(Thickness {
            left: 24.0,
            top: 40.0,
            right: 24.0,
            bottom: 34.0,
        })
        .allow_drop(true)
        .drag_over(crate::accept_files)
        .drag_drop(move |ctx| match ctx.get_storage_items().first() {
            Some(item) => {
                let path = item.path.clone();
                set_path.call(path.clone());
                if let Some(out) = &set_output {
                    out.call(default_output(&path));
                }
                fire_probe(&probe, path);
                DragOperation::Link
            }
            None => DragOperation::None,
        })
        .into()
}

// ===================== Stage 1: Source =====================

pub fn stage_source(vm: &Vm) -> (Element, Element) {
    if vm.source.is_empty() {
        let content = page(vec![
            drop_zone(
                GLYPH_VIDEO,
                "Select your HDR10 source",
                "The master file to copy HDR10 metadata from. Drag a video here, or browse.",
                crate::pick_source_handler(vm),
                vm.set_source.clone(),
                vm.src_trigger.clone(),
                None,
            )
            .with_key("src-dropzone"),
        ]);
        let disabled = button("Continue").accent().enabled(false);
        return (
            content,
            footer("Pick your HDR10 master to continue", Some(disabled.into())),
        );
    }

    let probe = valid_probe(&vm.src_probe, &vm.source);
    let mut children: Vec<Element> = Vec::new();

    // Source file card.
    children.push(
        body_strong("Source file")
            .margin(Thickness {
                left: 2.0,
                top: 0.0,
                right: 2.0,
                bottom: 9.0,
            })
            .with_key("src-header")
            .into(),
    );
    children.push(
        file_card(
            GLYPH_VIDEO,
            &file_name(&vm.source),
            &file_detail(&vm.source, probe),
            None,
            button("Change")
                .on_click(crate::pick_source_handler(vm))
                .into(),
        )
        .with_key("src-card"),
    );

    match &vm.src_probe {
        MutationState::Loading => {
            children.push(
                hstack((
                    ProgressRing::indeterminate().width(20.0).height(20.0),
                    body("Reading HDR10 metadata\u{2026}").foreground(ThemeRef::SecondaryText),
                ))
                .spacing(10.0)
                .margin(Thickness {
                    left: 2.0,
                    top: 18.0,
                    right: 0.0,
                    bottom: 0.0,
                })
                .with_key("src-loading")
                .into(),
            );
        }
        MutationState::Error(e) => {
            children.push(
                InfoBar::new("Probe failed")
                    .message(e.clone())
                    .error()
                    .is_open(true)
                    .is_closable(false)
                    .margin(Thickness {
                        left: 0.0,
                        top: 14.0,
                        right: 0.0,
                        bottom: 0.0,
                    })
                    .with_key("src-error")
                    .into(),
            );
        }
        _ => {}
    }

    if let Some(p) = probe {
        let detected = p.hdr.to_metadata();
        let effective = vm.ui.override_for(&vm.source).unwrap_or(detected);
        let any_override = effective != detected;

        // "HDR10 metadata" header with optional "Reset all".
        let mut head: Vec<Element> = vec![
            body_strong("HDR10 metadata")
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(0)
                .with_key("title")
                .into(),
        ];
        if any_override {
            let upd = vm.upd.clone();
            head.push(
                button("Reset all")
                    .subtle()
                    .icon(Symbol::Refresh)
                    .on_click(move || {
                        upd.call(|mut s| {
                            s.overrides = None;
                            s.editing = None;
                            s
                        });
                    })
                    .grid_column(1)
                    .with_key("reset-all")
                    .into(),
            );
        }
        children.push(
            grid(head)
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .margin(Thickness {
                    left: 2.0,
                    top: 22.0,
                    right: 2.0,
                    bottom: 9.0,
                })
                .with_key("meta-header")
                .into(),
        );

        // The five editable metadata rows.
        let mut rows: Vec<Element> = Vec::new();
        for row in 0..META_ROWS.len() {
            if row > 0 {
                rows.push(divider().with_key(format!("div-{row}")));
            }
            // The key changes when the editor opens/closes so the row subtree is
            // rebuilt instead of morphing a Grid into a StackPanel in place.
            let editing = vm.ui.editing_for(&vm.source).is_some_and(|e| e.row == row);
            rows.push(meta_row(vm, row, &detected, &effective).with_key(format!(
                "row-{row}-{}",
                if editing { "edit" } else { "view" }
            )));
        }
        children.push(card(vstack(rows).spacing(0.0)).with_key("meta-card"));

        // Status banner.
        let banner = if any_override {
            InfoBar::new("Manual overrides active")
                .message("These values will be written instead of the detected ones.")
                .warning()
        } else if p.has_hdr10() {
            InfoBar::new("Valid HDR10 found")
                .message("Ready to copy into your encode.")
                .success()
        } else {
            InfoBar::new("No HDR10 signalling detected")
                .message("This file doesn't look like an HDR10 master; the detected values can still be edited above.")
                .warning()
        };
        children.push(
            banner
                .is_open(true)
                .is_closable(false)
                .margin(Thickness {
                    left: 0.0,
                    top: 14.0,
                    right: 0.0,
                    bottom: 0.0,
                })
                .with_key("src-banner")
                .into(),
        );
    }

    let upd = vm.upd.clone();
    let can_continue = probe.is_some();
    let action = button("Continue")
        .accent()
        .enabled(can_continue)
        .on_click(move || {
            upd.call(|mut s| {
                s.stage = Stage::Encode;
                s
            });
        });
    (content_and_hint(children), footer("", Some(action.into())))
}

/// Wraps stage children into the page column (helper so both arms return the same shape).
fn content_and_hint(children: Vec<Element>) -> Element {
    page(children)
}

/// A file (or output) card row: icon, title + detail, optional badge, trailing button.
fn file_card(
    icon: &'static str,
    title_text: &str,
    detail: &str,
    badge: Option<Element>,
    action: Element,
) -> Element {
    let mut cells: Vec<Element> = vec![
        icon_block(icon).grid_column(0).with_key("icon"),
        vstack((
            body_strong(title_text),
            caption(detail).foreground(ThemeRef::SecondaryText),
        ))
        .spacing(1.0)
        .vertical_alignment(VerticalAlignment::Center)
        .grid_column(1)
        .with_key("info")
        .into(),
    ];
    if let Some(b) = badge {
        cells.push(b.grid_column(2).with_key("badge"));
    }
    cells.push(action.grid_column(3).with_key("action"));
    card(
        grid(cells)
            .columns([
                GridLength::Auto,
                GridLength::Star(1.0),
                GridLength::Auto,
                GridLength::Auto,
            ])
            .column_spacing(13.0)
            .padding(Thickness {
                left: 15.0,
                top: 13.0,
                right: 15.0,
                bottom: 13.0,
            }),
    )
}

/// A small tinted pill badge ("No HDR10", "Edited").
fn pill(text: &str, bg: ThemeRef, fg: ThemeRef) -> Element {
    border(text_block(text).font_size(11.0).semibold().foreground(fg))
        .corner_radius(4.0)
        .background(bg)
        .padding(Thickness {
            left: 8.0,
            top: 2.0,
            right: 8.0,
            bottom: 2.0,
        })
        .vertical_alignment(VerticalAlignment::Center)
        .into()
}

/// One metadata row: label · (Edited) · value · edit button, plus the expanded editor.
fn meta_row(vm: &Vm, row: usize, detected: &Hdr10Metadata, effective: &Hdr10Metadata) -> Element {
    let overridden = row_overridden(row, effective, detected);
    let editing = vm
        .ui
        .editing_for(&vm.source)
        .filter(|e| e.row == row)
        .cloned();

    let mut cells: Vec<Element> = vec![
        body(META_ROWS[row])
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(0)
            .with_key("label")
            .into(),
    ];
    if overridden {
        cells.push(
            pill("EDITED", ThemeRef::SubtleFill, ThemeRef::AccentText)
                .grid_column(1)
                .with_key("pill"),
        );
    }
    let value_fg = if overridden {
        ThemeRef::AccentText
    } else {
        ThemeRef::SecondaryText
    };
    cells.push(
        text_block(row_value(row, effective))
            .font_size(13.0)
            .foreground(value_fg)
            .horizontal_alignment(HorizontalAlignment::Right)
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(2)
            .with_key("value")
            .into(),
    );
    if editing.is_none() {
        let upd = vm.upd.clone();
        let path = vm.source.clone();
        let eff = *effective;
        cells.push(
            button("")
                .icon(Symbol::Edit)
                .on_click(move || {
                    let path = path.clone();
                    upd.call(move |mut s| {
                        s.editing = Some(EditState {
                            path,
                            row,
                            draft: Draft::from_meta(&eff),
                        });
                        s
                    });
                })
                .width(34.0)
                .grid_column(3)
                .with_key("edit")
                .into(),
        );
    }
    let header = grid(cells)
        .columns([
            GridLength::Auto,
            GridLength::Auto,
            GridLength::Star(1.0),
            GridLength::Auto,
        ])
        .column_spacing(12.0)
        .padding(Thickness {
            left: 16.0,
            top: 12.0,
            right: 14.0,
            bottom: 12.0,
        });

    match editing {
        None => header.into(),
        Some(edit) => vstack((header, meta_editor(vm, row, edit, detected, overridden)))
            .spacing(0.0)
            .into(),
    }
}

/// The expanded "Raw HEVC values" editor under a metadata row.
fn meta_editor(
    vm: &Vm,
    row: usize,
    edit: EditState,
    detected: &Hdr10Metadata,
    overridden: bool,
) -> Element {
    let d = edit.draft;
    let upd = vm.upd.clone();

    // A draft-field mutator: patches the open editor's draft.
    let set = move |f: fn(&mut Draft, f64)| {
        let upd = upd.clone();
        move |v: f64| {
            upd.call(move |mut s| {
                if let Some(e) = s.editing.as_mut() {
                    f(&mut e.draft, v);
                }
                s
            });
        }
    };

    let num = |value: f64, header: &str, apply: fn(&mut Draft, f64)| -> Element {
        NumberBox::new(value)
            .header(header)
            .on_value_changed(set(apply))
            .into()
    };

    let fields: Element = match row {
        0 => {
            let combo = |header: &str,
                         opts: &[u8],
                         current: u8,
                         namer: fn(u8) -> &'static str,
                         apply: fn(&mut Draft, u8)|
             -> Element {
                let items: Vec<String> = opts.iter().map(|&c| code_label(c, namer(c))).collect();
                let selected = opts.iter().position(|&c| c == current).unwrap_or(0) as i32;
                let opts = opts.to_vec();
                let upd = vm.upd.clone();
                ComboBox::new(items)
                    .header(header)
                    .selected_index(selected)
                    .on_selection_changed(move |i: i32| {
                        if let Some(&code) = opts.get(i as usize) {
                            upd.call(move |mut s| {
                                if let Some(e) = s.editing.as_mut() {
                                    apply(&mut e.draft, code);
                                }
                                s
                            });
                        }
                    })
                    .into()
            };
            grid((
                combo(
                    "Colour primaries",
                    &PRIMARIES_OPTS,
                    d.primaries,
                    naming::primaries_name,
                    |d, v| d.primaries = v,
                )
                .grid_column(0),
                combo(
                    "Transfer (EOTF)",
                    &TRANSFER_OPTS,
                    d.transfer,
                    naming::transfer_name,
                    |d, v| d.transfer = v,
                )
                .grid_column(1),
                combo(
                    "Matrix coefficients",
                    &MATRIX_OPTS,
                    d.matrix,
                    naming::matrix_name,
                    |d, v| d.matrix = v,
                )
                .grid_column(2),
            ))
            .columns([
                GridLength::Star(1.0),
                GridLength::Star(1.0),
                GridLength::Star(1.0),
            ])
            .column_spacing(10.0)
            .into()
        }
        1 => {
            let upd = vm.upd.clone();
            ComboBox::new(["0 · Limited (TV)".to_owned(), "1 · Full (PC)".to_owned()])
                .header("video_full_range_flag")
                .selected_index(if d.full_range { 1 } else { 0 })
                .on_selection_changed(move |i: i32| {
                    upd.call(move |mut s| {
                        if let Some(e) = s.editing.as_mut() {
                            e.draft.full_range = i == 1;
                        }
                        s
                    });
                })
                .max_width(220.0)
                .horizontal_alignment(HorizontalAlignment::Left)
                .into()
        }
        2 => {
            let cell = |v: u16, h: &str, apply: fn(&mut Draft, f64)| num(v as f64, h, apply);
            vstack((
                grid((
                    cell(d.gx, "Green x", |d, v| d.gx = v as u16).grid_column(0),
                    cell(d.gy, "Green y", |d, v| d.gy = v as u16).grid_column(1),
                    cell(d.bx, "Blue x", |d, v| d.bx = v as u16).grid_column(2),
                    cell(d.by, "Blue y", |d, v| d.by = v as u16).grid_column(3),
                ))
                .columns([
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                ])
                .column_spacing(10.0),
                grid((
                    cell(d.rx, "Red x", |d, v| d.rx = v as u16).grid_column(0),
                    cell(d.ry, "Red y", |d, v| d.ry = v as u16).grid_column(1),
                    cell(d.wx, "White x", |d, v| d.wx = v as u16).grid_column(2),
                    cell(d.wy, "White y", |d, v| d.wy = v as u16).grid_column(3),
                ))
                .columns([
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                ])
                .column_spacing(10.0),
            ))
            .spacing(10.0)
            .into()
        }
        3 => grid((
            num(d.max_lum as f64, "max_luminance (0.0001 cd/m²)", |d, v| {
                d.max_lum = v as u32
            })
            .grid_column(0),
            num(d.min_lum as f64, "min_luminance (0.0001 cd/m²)", |d, v| {
                d.min_lum = v as u32
            })
            .grid_column(1),
        ))
        .columns([GridLength::Star(1.0), GridLength::Star(1.0)])
        .column_spacing(10.0)
        .into(),
        _ => grid((
            num(d.max_cll as f64, "MaxCLL (cd/m²)", |d, v| {
                d.max_cll = v as u16
            })
            .grid_column(0),
            num(d.max_fall as f64, "MaxFALL (cd/m²)", |d, v| {
                d.max_fall = v as u16
            })
            .grid_column(1),
        ))
        .columns([GridLength::Star(1.0), GridLength::Star(1.0)])
        .column_spacing(10.0)
        .into(),
    };

    // Reset-to-detected · Cancel · Apply.
    let mut buttons: Vec<Element> = Vec::new();
    if overridden {
        let upd = vm.upd.clone();
        let det = *detected;
        let path = edit.path.clone();
        buttons.push(
            button("Reset to detected")
                .subtle()
                .on_click(move || {
                    let path = path.clone();
                    upd.call(move |mut s| {
                        // Re-applying this row's detected values over the current override.
                        if let Some((p, m)) = s.overrides.clone()
                            && p == path
                        {
                            let reset = Draft::from_meta(&det).apply_row(row, m);
                            s.overrides = Some((p, reset));
                        }
                        s.editing = None;
                        s
                    });
                })
                .grid_column(0)
                .horizontal_alignment(HorizontalAlignment::Left)
                .with_key("reset")
                .into(),
        );
    }
    let upd = vm.upd.clone();
    buttons.push(
        button("Cancel")
            .on_click(move || {
                upd.call(|mut s| {
                    s.editing = None;
                    s
                });
            })
            .grid_column(1)
            .with_key("cancel")
            .into(),
    );
    let upd = vm.upd.clone();
    let eff_current = vm.ui.override_for(&vm.source).unwrap_or(*detected);
    let path = edit.path.clone();
    buttons.push(
        button("Apply")
            .accent()
            .on_click(move || {
                let path = path.clone();
                upd.call(move |mut s| {
                    if let Some(e) = s.editing.take() {
                        let merged = e.draft.apply_row(row, eff_current);
                        s.overrides = Some((path, merged));
                    }
                    s
                });
            })
            .grid_column(2)
            .with_key("apply")
            .into(),
    );

    border(
        vstack((
            caption("RAW HEVC VALUES")
                .semibold()
                .foreground(ThemeRef::SecondaryText),
            fields,
            grid(buttons)
                .columns([GridLength::Star(1.0), GridLength::Auto, GridLength::Auto])
                .column_spacing(9.0)
                .margin(Thickness {
                    left: 0.0,
                    top: 13.0,
                    right: 0.0,
                    bottom: 0.0,
                }),
        ))
        .spacing(9.0),
    )
    .background(ThemeRef::SubtleFill)
    .padding(Thickness {
        left: 16.0,
        top: 8.0,
        right: 16.0,
        bottom: 14.0,
    })
    .into()
}

// ===================== Stage 2: Encode =====================

pub fn stage_encode(vm: &Vm, processing: bool) -> (Element, Option<Element>) {
    if processing {
        return (processing_view(vm), None);
    }

    let src_probe = valid_probe(&vm.src_probe, &vm.source);
    let mut children: Vec<Element> = Vec::new();

    // "Copying from …" banner.
    let src_meta = src_probe.map(|p| {
        vm.ui
            .override_for(&vm.source)
            .unwrap_or_else(|| p.hdr.to_metadata())
    });
    let copying = match &src_meta {
        Some(m) => colour_summary(m),
        None => String::new(),
    };
    children.push(
        InfoBar::new(format!("Copying from {}", file_name(&vm.source)))
            .message(copying)
            .informational()
            .is_open(true)
            .is_closable(false)
            .margin(Thickness {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 22.0,
            })
            .with_key("enc-src-banner")
            .into(),
    );

    if vm.encode.is_empty() {
        children.push(
            drop_zone(
                GLYPH_UPLOAD,
                "Select the encode to patch",
                "The HEVC encode that's missing its HDR10 signalling. Drag a video here, or browse.",
                crate::pick_encode_handler(vm),
                vm.set_encode.clone(),
                vm.enc_trigger.clone(),
                Some(vm.set_output.clone()),
            )
            .with_key("enc-dropzone"),
        );
        return (
            page(children),
            Some(footer("Select the encode that needs HDR10", None)),
        );
    }

    let enc_probe = valid_probe(&vm.enc_probe, &vm.encode);

    // Encode card with a "No HDR10" badge.
    children.push(
        body_strong("Encode to patch")
            .margin(Thickness {
                left: 2.0,
                top: 0.0,
                right: 2.0,
                bottom: 9.0,
            })
            .with_key("enc-header")
            .into(),
    );
    let badge = enc_probe.map(|p| {
        if p.has_hdr10() {
            pill(
                "HDR10 PRESENT",
                ThemeRef::SystemSuccessBackground,
                ThemeRef::SystemSuccess,
            )
        } else {
            pill(
                "NO HDR10",
                ThemeRef::SystemCautionBackground,
                ThemeRef::SystemCaution,
            )
        }
    });
    children.push(
        file_card(
            GLYPH_VIDEO,
            &file_name(&vm.encode),
            &file_detail(&vm.encode, enc_probe),
            badge,
            button("Change")
                .on_click(crate::pick_encode_handler(vm))
                .into(),
        )
        .with_key("enc-card"),
    );

    // Output card.
    children.push(section("Output").with_key("out-header"));
    let save_row = {
        let mut cells: Vec<Element> = vec![
            vstack((
                body_strong("Save as"),
                caption(vm.output.clone()).foreground(ThemeRef::SecondaryText),
            ))
            .spacing(1.0)
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(0)
            .into(),
        ];
        cells.push(
            button("Choose\u{2026}")
                .icon(Symbol::Save)
                .on_click(crate::pick_output_handler(vm))
                .grid_column(1)
                .into(),
        );
        grid(cells)
            .columns([GridLength::Star(1.0), GridLength::Auto])
            .column_spacing(13.0)
            .padding(Thickness {
                left: 16.0,
                top: 13.0,
                right: 16.0,
                bottom: 13.0,
            })
    };
    let container_row = grid((
        body("Container")
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(0),
        text_block("Matroska (.mkv)")
            .font_size(13.0)
            .foreground(ThemeRef::SecondaryText)
            .grid_column(1),
    ))
    .columns([GridLength::Star(1.0), GridLength::Auto])
    .padding(Thickness {
        left: 16.0,
        top: 13.0,
        right: 16.0,
        bottom: 13.0,
    });
    children
        .push(card(vstack((save_row, divider(), container_row)).spacing(0.0)).with_key("out-card"));

    // "No re-encoding" note.
    children.push(
        InfoBar::new("No re-encoding")
            .message(
                "Only the SPS VUI and an HDR10 SEI are written. All audio & subtitle tracks are carried over untouched.",
            )
            .informational()
            .is_open(true)
            .is_closable(false)
            .margin(Thickness {
                left: 0.0,
                top: 14.0,
                right: 0.0,
                bottom: 0.0,
            })
            .with_key("enc-note")
            .into(),
    );

    // Run error from a previous attempt.
    if let MutationState::Error(e) = &vm.run {
        children.push(
            InfoBar::new("Patch failed")
                .message(e.clone())
                .error()
                .is_open(true)
                .is_closable(false)
                .margin(Thickness {
                    left: 0.0,
                    top: 14.0,
                    right: 0.0,
                    bottom: 0.0,
                })
                .with_key("run-error")
                .into(),
        );
    }

    let action = button("Patch HDR10")
        .accent()
        .icon(Symbol::Accept)
        .enabled(!vm.output.is_empty())
        .on_click(crate::run_handler(vm));
    (page(children), Some(footer("", Some(action.into()))))
}

/// The centred processing view (spinner, stage label, progress bar).
fn processing_view(vm: &Vm) -> Element {
    let idx = vm.progress.min(5);
    let label = [
        "Probing source metadata",
        "Extracting HEVC stream",
        "Patching bitstream",
        "Capturing frame timing",
        "Remuxing tracks",
        "Finishing up",
    ][idx];
    let pct = (idx as f64 / 5.0 * 100.0).round();
    vstack((
        ProgressRing::indeterminate().width(54.0).height(54.0),
        text_block(label)
            .font_size(18.0)
            .semibold()
            .horizontal_alignment(HorizontalAlignment::Center)
            .margin(Thickness {
                left: 0.0,
                top: 24.0,
                right: 0.0,
                bottom: 0.0,
            }),
        caption("Lossless bitstream edit · timing preserved exactly")
            .foreground(ThemeRef::SecondaryText)
            .horizontal_alignment(HorizontalAlignment::Center),
        vstack((
            ProgressBar::new(idx as f64).range(0.0, 5.0),
            grid((
                caption(format!("Stage {} of 6", idx + 1))
                    .foreground(ThemeRef::TertiaryText)
                    .grid_column(0),
                caption(format!("{pct}%"))
                    .foreground(ThemeRef::TertiaryText)
                    .grid_column(1),
            ))
            .columns([GridLength::Star(1.0), GridLength::Auto]),
        ))
        .spacing(8.0)
        .width(320.0)
        .margin(Thickness {
            left: 0.0,
            top: 22.0,
            right: 0.0,
            bottom: 0.0,
        }),
    ))
    .spacing(5.0)
    .horizontal_alignment(HorizontalAlignment::Center)
    .vertical_alignment(VerticalAlignment::Center)
    .into()
}

// ===================== Stage 3: Result =====================

pub fn stage_result(vm: &Vm, summary: &RunSummary) -> (Element, Element) {
    let mut children: Vec<Element> = Vec::new();

    // Success header.
    children.push(
        vstack((
            border(
                glyph(GLYPH_CHECK, 30.0)
                    .foreground(ThemeRef::SystemSuccess)
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .vertical_alignment(VerticalAlignment::Center),
            )
            .corner_radius(34.0)
            .background(ThemeRef::SystemSuccessBackground)
            .width(68.0)
            .height(68.0)
            .horizontal_alignment(HorizontalAlignment::Center),
            text_block("HDR10 metadata embedded")
                .font_size(20.0)
                .semibold()
                .horizontal_alignment(HorizontalAlignment::Center)
                .margin(Thickness {
                    left: 0.0,
                    top: 14.0,
                    right: 0.0,
                    bottom: 0.0,
                }),
            text_block(
                "Your encode now carries the source's HDR10 signalling — losslessly, with every frame's timing preserved.",
            )
            .font_size(13.0)
            .foreground(ThemeRef::SecondaryText)
            .wrap()
            .max_width(400.0)
            .horizontal_alignment(HorizontalAlignment::Center),
        ))
        .spacing(5.0)
        .margin(Thickness {
            left: 0.0,
            top: 4.0,
            right: 0.0,
            bottom: 24.0,
        })
        .with_key("res-hero")
        .into(),
    );

    // Output file card with a Play button.
    let out = summary.output.clone();
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    let detail = format!("{out} · {} · MKV", human_size(size));
    let play = {
        let out = out.clone();
        button("")
            .icon(Symbol::Play)
            .on_click(move || crate::open_path(&out))
            .width(38.0)
    };
    children.push(
        file_card(
            GLYPH_VIDEO,
            &file_name(&summary.output),
            &detail,
            None,
            play.into(),
        )
        .with_key("res-card"),
    );

    // Summary card.
    children.push(section("Summary").with_key("sum-header"));
    let meta = valid_probe(&vm.src_probe, &vm.source).map(|p| {
        vm.ui
            .override_for(&vm.source)
            .unwrap_or_else(|| p.hdr.to_metadata())
    });
    let mut rows: Vec<Element> = Vec::new();
    let mut push_row = |icon: &'static str, icon_fg: ThemeRef, label: &str, value: String| {
        if !rows.is_empty() {
            rows.push(divider().with_key(format!("div-{}", rows.len())));
        }
        let key = format!("sum-{}", rows.len());
        rows.push(
            grid((
                glyph(icon, 15.0)
                    .foreground(icon_fg)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(0),
                body(label)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(1),
                text_block(value)
                    .font_size(13.0)
                    .foreground(ThemeRef::SecondaryText)
                    .horizontal_alignment(HorizontalAlignment::Right)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(2),
            ))
            .columns([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
            .column_spacing(13.0)
            .padding(Thickness {
                left: 16.0,
                top: 13.0,
                right: 16.0,
                bottom: 13.0,
            })
            .with_key(key)
            .into(),
        );
    };
    push_row(
        GLYPH_CHECK,
        ThemeRef::SystemSuccess,
        "HDR10 SEI inserted",
        if summary.sei_inserted {
            "ST 2086 + MaxCLL/FALL".to_owned()
        } else {
            "already present".to_owned()
        },
    );
    push_row(
        GLYPH_CHECK,
        ThemeRef::SystemSuccess,
        "SPS VUI colour rewritten",
        format!("{} SPS · bit-exact", summary.sps_patched),
    );
    if let Some(m) = &meta {
        push_row(
            GLYPH_COLOR,
            ThemeRef::AccentText,
            "Colour",
            colour_summary(m),
        );
    }
    push_row(
        GLYPH_CLOCK,
        ThemeRef::AccentText,
        "Timing",
        match summary.frames {
            Some(n) => format!("{n} frames preserved exactly"),
            None => "preserved exactly".to_owned(),
        },
    );
    push_row(
        GLYPH_AUDIO,
        ThemeRef::AccentText,
        "Tracks carried over",
        summary
            .tracks
            .clone()
            .unwrap_or_else(|| "audio & subtitles untouched".to_owned()),
    );
    children.push(card(vstack(rows).spacing(0.0)).with_key("sum-card"));

    // Footer: Patch another (left) · Open output folder (accent).
    let upd = vm.upd.clone();
    let (set_source, set_encode, set_output) = (
        vm.set_source.clone(),
        vm.set_encode.clone(),
        vm.set_output.clone(),
    );
    let another = button("Patch another")
        .subtle()
        .icon(Symbol::Refresh)
        .on_click(move || {
            set_source.call(String::new());
            set_encode.call(String::new());
            set_output.call(String::new());
            upd.call(|s| Ui {
                theme: s.theme,
                run_token: s.run_token,
                ack_token: s.run_token,
                tools_rev: s.tools_rev,
                ..Ui::default()
            });
        });
    let out = summary.output.clone();
    let open_folder = button("Open output folder")
        .accent()
        .icon(Symbol::Folder)
        .on_click(move || crate::reveal_in_folder(&out));
    let bar = border(
        grid((
            another
                .grid_column(0)
                .horizontal_alignment(HorizontalAlignment::Left),
            open_folder.grid_column(1),
        ))
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .padding(Thickness {
            left: 22.0,
            top: 13.0,
            right: 22.0,
            bottom: 13.0,
        }),
    )
    .border_thickness(Thickness {
        left: 0.0,
        top: 1.0,
        right: 0.0,
        bottom: 0.0,
    })
    .border_brush(ThemeRef::DividerStroke)
    .background(ThemeRef::LayerFill)
    .into();

    (page(children), bar)
}

// ===================== Preferences overlay =====================

/// The Preferences dialog rendered as a smoke-layer overlay on top of the app.
pub fn prefs_overlay(vm: &Vm) -> Element {
    let toolset = Toolset::discover(&Settings::load().tool_overrides);

    let mut rows: Vec<Element> = Vec::new();
    for (i, (tool, resolution)) in toolset.all().into_iter().enumerate() {
        if i > 0 {
            rows.push(divider().with_key(format!("div-{i}")));
        }
        let editing = vm.ui.tool_edit.as_ref().is_some_and(|(t, _)| *t == i);
        rows.push(tool_row(vm, i, tool, resolution).with_key(format!(
            "tool-{i}-{}",
            if editing { "edit" } else { "view" }
        )));
    }

    let about_card = card(
        vstack((
            grid((
                body("nvhvc HDR10 patcher").grid_column(0),
                text_block(nvhvc_core::VERSION)
                    .font_size(13.0)
                    .foreground(ThemeRef::SecondaryText)
                    .grid_column(1),
            ))
            .columns([GridLength::Star(1.0), GridLength::Auto])
            .padding(Thickness {
                left: 16.0,
                top: 12.0,
                right: 16.0,
                bottom: 12.0,
            }),
            divider(),
            grid((
                body("Default output container").grid_column(0),
                text_block("Matroska (.mkv)")
                    .font_size(13.0)
                    .foreground(ThemeRef::SecondaryText)
                    .grid_column(1),
            ))
            .columns([GridLength::Star(1.0), GridLength::Auto])
            .padding(Thickness {
                left: 16.0,
                top: 12.0,
                right: 16.0,
                bottom: 12.0,
            }),
        ))
        .spacing(0.0),
    );

    let body_col = vstack((
        body_strong("External tools"),
        caption(
            "Point any tool at your own build. A custom path is always used, even when a copy is found on PATH.",
        )
        .foreground(ThemeRef::SecondaryText)
        .wrap(),
        card(vstack(rows).spacing(0.0)).margin(Thickness {
            left: 0.0,
            top: 10.0,
            right: 0.0,
            bottom: 0.0,
        }),
        section("About"),
        about_card,
    ))
    .spacing(3.0);

    let upd = vm.upd.clone();
    let done = button("Done")
        .accent()
        .on_click(move || {
            upd.call(|mut s| {
                s.prefs_open = false;
                s.tool_edit = None;
                s
            });
        })
        .horizontal_alignment(HorizontalAlignment::Stretch);

    let dialog = border(
        vstack((
            text_block("Preferences")
                .font_size(20.0)
                .semibold()
                .margin(Thickness {
                    left: 24.0,
                    top: 23.0,
                    right: 24.0,
                    bottom: 5.0,
                }),
            scroll_viewer(body_col.margin(Thickness {
                left: 24.0,
                top: 10.0,
                right: 24.0,
                bottom: 20.0,
            }))
            .max_height(420.0),
            border(done.padding(Thickness {
                left: 12.0,
                top: 8.0,
                right: 12.0,
                bottom: 8.0,
            }))
            .border_thickness(Thickness {
                left: 0.0,
                top: 1.0,
                right: 0.0,
                bottom: 0.0,
            })
            .border_brush(ThemeRef::DividerStroke)
            .padding(Thickness::uniform(20.0)),
        ))
        .spacing(0.0),
    )
    .corner_radius(8.0)
    .background(ThemeRef::SolidBackground)
    .border_brush(ThemeRef::SurfaceStroke)
    .border_thickness(Thickness::uniform(1.0))
    .width(524.0)
    .horizontal_alignment(HorizontalAlignment::Center)
    .vertical_alignment(VerticalAlignment::Center);

    border(dialog)
        .background(ThemeRef::SmokeFill)
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .vertical_alignment(VerticalAlignment::Stretch)
        .into()
}

/// One tool row in Preferences: status dot, name + resolution detail, Edit button —
/// or the inline custom-path editor when this row is being edited.
fn tool_row(vm: &Vm, index: usize, tool: Tool, resolution: &Resolution) -> Element {
    if let Some((i, draft)) = &vm.ui.tool_edit
        && *i == index
    {
        {
            let upd = vm.upd.clone();
            let input = text_box(draft.clone())
                .placeholder_text(r"C:\path\to\executable.exe")
                .on_text_changed(move |v: String| {
                    upd.call(move |mut s| {
                        if let Some((_, d)) = s.tool_edit.as_mut() {
                            *d = v;
                        }
                        s
                    });
                });
            let upd = vm.upd.clone();
            let cancel = button("Cancel").on_click(move || {
                upd.call(|mut s| {
                    s.tool_edit = None;
                    s
                });
            });
            let upd = vm.upd.clone();
            let draft_now = draft.clone();
            let set = button("Set").accent().on_click(move || {
                let value = draft_now.trim().to_owned();
                let mut settings = Settings::load();
                set_tool_override(&mut settings, tool, &value);
                let _ = settings.save();
                upd.call(|mut s| {
                    s.tool_edit = None;
                    s.tools_rev += 1;
                    s
                });
            });
            return border(
                vstack((
                    caption(format!(
                        "{} — custom executable (empty resets to auto-detect)",
                        tool.label()
                    ))
                    .semibold(),
                    grid((
                        input.grid_column(0),
                        cancel.grid_column(1),
                        set.grid_column(2),
                    ))
                    .columns([GridLength::Star(1.0), GridLength::Auto, GridLength::Auto])
                    .column_spacing(8.0),
                ))
                .spacing(8.0),
            )
            .background(ThemeRef::SubtleFill)
            .padding(Thickness {
                left: 15.0,
                top: 12.0,
                right: 15.0,
                bottom: 12.0,
            })
            .into();
        }
    }

    let (dot, detail) = match resolution {
        Resolution::OnPath(p) => (
            ThemeRef::SystemSuccess,
            format!("On PATH · {}", p.display()),
        ),
        Resolution::Override(p) => (
            ThemeRef::SystemSuccess,
            format!("Custom path · {}", p.display()),
        ),
        Resolution::Fallback(p) => (ThemeRef::Accent, format!("Install dir · {}", p.display())),
        Resolution::NotFound => (
            ThemeRef::SystemCritical,
            format!("Not found · {}", tool.install_hint()),
        ),
    };
    let current = resolution
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let upd = vm.upd.clone();
    let edit = button("Edit").icon(Symbol::Edit).on_click(move || {
        let current = current.clone();
        upd.call(move |mut s| {
            s.tool_edit = Some((index, current));
            s
        });
    });
    grid((
        border(text_block(""))
            .width(9.0)
            .height(9.0)
            .corner_radius(4.5)
            .background(dot)
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(0),
        vstack((
            text_block(tool.label()).font_size(13.0).semibold(),
            caption(detail).foreground(ThemeRef::TertiaryText),
        ))
        .spacing(0.0)
        .vertical_alignment(VerticalAlignment::Center)
        .grid_column(1),
        edit.grid_column(2),
    ))
    .columns([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
    .column_spacing(12.0)
    .padding(Thickness {
        left: 15.0,
        top: 12.0,
        right: 15.0,
        bottom: 12.0,
    })
    .into()
}

/// Writes (or clears, when `value` is empty) a tool's custom path in settings.
fn set_tool_override(settings: &mut Settings, tool: Tool, value: &str) {
    let path = if value.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(value))
    };
    let o = &mut settings.tool_overrides;
    match tool {
        Tool::Ffmpeg => o.ffmpeg = path,
        Tool::Ffprobe => o.ffprobe = path,
        Tool::Mkvmerge => o.mkvmerge = path,
        Tool::Mkvextract => o.mkvextract = path,
        Tool::Mp4box => o.mp4box = path,
    }
}

/// The mutation's probe result, but only if it belongs to `path`.
pub fn valid_probe<'a>(state: &'a MutationState<FileProbe>, path: &str) -> Option<&'a FileProbe> {
    match state {
        MutationState::Success(p) if p.path == path => Some(p),
        _ => None,
    }
}
