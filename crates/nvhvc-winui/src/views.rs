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
/// Film-strip ("MyMovies") — the video-clip glyph used everywhere a source/encode is shown,
/// mirroring GTK's `video-reel-symbolic` (E714 is a camera, not a clip).
const GLYPH_FILM: &str = "\u{E8B2}";
const GLYPH_UPLOAD: &str = "\u{E898}";
const GLYPH_COLOR: &str = "\u{E790}";
const GLYPH_CLOCK: &str = "\u{E823}";
const GLYPH_AUDIO: &str = "\u{E8D6}";
const GLYPH_ARROW_R: &str = "\u{E72A}";
const GLYPH_SPARKLE: &str = "\u{E735}";
const GLYPH_FOLDER_OPEN: &str = "\u{E838}";

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

/// Inner padding for a Grid row. The reactor backend applies `Padding` only to Border /
/// StackPanel / TextBlock / Control handles — on a `grid()` it is silently dropped, which
/// made every card row render with zero padding. Wrapping in a Border makes it real.
fn padded(child: impl Into<Element>, t: Thickness) -> Border {
    border(child).padding(t)
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

/// The 40×40 rounded icon block at the left of a file card, tinted per the card's meaning
/// (accent = source video, caution = encode missing HDR10, success = finished output).
fn icon_block(g: &'static str, bg: ThemeRef, fg: ThemeRef, size: f64) -> Element {
    border(
        glyph(g, size / 2.0)
            .foreground(fg)
            .horizontal_alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Center),
    )
    .corner_radius(5.0)
    .background(bg)
    .width(size)
    .height(size)
    .into()
}

/// The scrollable, centred 560px content column of a stage.
fn page(children: Vec<Element>) -> Element {
    scroll_viewer(
        vstack(children)
            .spacing(0.0)
            .max_width(560.0)
            // Center (not Stretch): with Stretch the column sits relative to the viewport the
            // scrollbar reservation leaves, drifting right of true window centre.
            .horizontal_alignment(HorizontalAlignment::Center)
            .width(560.0)
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
    // The menu itself is a custom overlay layer (see `menu_overlay`) — a native MenuFlyout
    // can't carry item icons or the theme switcher. This button just toggles it. TOP-aligned
    // at the caption-button height: min/max/close hug the top edge of the 48px bar (glyph
    // centres ≈ 16px), so a vertically-centred button sits visibly lower than they do.
    //
    // The toggle reads LIVE state inside the update: `on_click` closures are wired at mount
    // and never rewired on re-render, so anything captured at render time goes stale. The
    // overlay's click-catcher leaves a notch under this button (see `menu_overlay`) so a
    // click here fires only the toggle — with both firing, close+reopen looked like a no-op.
    let upd = vm.upd.clone();
    let menu = button(GLYPH_MORE)
        .subtle()
        .on_click(move || {
            upd.call(|mut s| {
                s.menu_open = !s.menu_open;
                s
            });
        })
        .font_family(ICON_FONT)
        .font_size(14.0)
        .width(40.0)
        .height(32.0)
        .padding(0.0)
        .vertical_alignment(VerticalAlignment::Top)
        // The TitleBar's RightHeader slot sits ~12px below the top edge and ~48px left of the
        // caption buttons; the system buttons hug the corner (glyph centres ≈ 15px), so pull
        // the button up and right to sit beside minimize like the mockup.
        .margin(Thickness {
            left: 0.0,
            top: -12.0,
            right: -48.0,
            bottom: 0.0,
        });

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
    // Padding lives on the Border — a Grid drops it (see `padded`).
    border(grid(cells).columns([GridLength::Star(1.0), GridLength::Auto]))
        .padding(Thickness {
            left: 22.0,
            top: 16.0,
            right: 22.0,
            bottom: 16.0,
        })
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

// ===================== Action buttons =====================

/// A mockup-styled primary action button built from a Border: a reactor `Button`'s hover
/// visual state repaints any custom background, so custom-styled buttons track hover
/// themselves via [`Ui::hovered_action`] (keyed by `id`). `trailing` puts the glyph after
/// the label ("Continue →"); disabled renders at 40% opacity with no handlers.
#[allow(clippy::too_many_arguments)]
fn action_button(
    vm: &Vm,
    id: &'static str,
    label: &str,
    glyph_str: &'static str,
    trailing: bool,
    enabled: bool,
    rest_bg: BrushBinding,
    hover_bg: BrushBinding,
    on_tap: impl Fn() + Clone + 'static,
) -> Element {
    let fg = || BrushBinding::from(ThemeRef::custom("TextOnAccentFillColorPrimaryBrush"));
    let text = text_block(label)
        .font_size(14.0)
        .foreground(fg())
        .vertical_alignment(VerticalAlignment::Center);
    let g = glyph(glyph_str, 13.0)
        .foreground(fg())
        .vertical_alignment(VerticalAlignment::Center);
    let cells: Vec<Element> = if trailing {
        vec![text.with_key("t").into(), g.with_key("g").into()]
    } else {
        vec![g.with_key("g").into(), text.with_key("t").into()]
    };
    let hovered = enabled && vm.ui.hovered_action == Some(id);
    let pressed = enabled && vm.ui.pressed_action == Some(id);
    let content = hstack(cells)
        .spacing(8.0)
        .horizontal_alignment(HorizontalAlignment::Center)
        // Dimmed content while held — the Fluent "pressed" cue.
        .opacity(if pressed { 0.6 } else { 1.0 });

    let b = border(content)
        .corner_radius(4.0)
        .background(if hovered || pressed {
            hover_bg
        } else {
            rest_bg
        })
        .padding(Thickness {
            left: 22.0,
            top: 9.0,
            right: 22.0,
            bottom: 9.0,
        });
    if !enabled {
        return b.opacity(0.4).into();
    }
    let enter = vm.upd.clone();
    let exit = vm.upd.clone();
    let press = vm.upd.clone();
    let release = vm.upd.clone();
    b.on_tapped(on_tap)
        .on_pointer_entered(move |_| {
            enter.call(move |mut s| {
                s.hovered_action = Some(id);
                s
            });
        })
        .on_pointer_exited(move || {
            exit.call(|mut s| {
                s.hovered_action = None;
                s.pressed_action = None;
                s
            });
        })
        .on_pointer_pressed(move |_| {
            press.call(move |mut s| {
                s.pressed_action = Some(id);
                s
            });
        })
        .on_pointer_released(move |_| {
            release.call(|mut s| {
                s.pressed_action = None;
                s
            });
        })
        .into()
}

/// [`action_button`] in the standard accent palette.
fn accent_action(
    vm: &Vm,
    id: &'static str,
    label: &str,
    glyph_str: &'static str,
    trailing: bool,
    enabled: bool,
    on_tap: impl Fn() + Clone + 'static,
) -> Element {
    action_button(
        vm,
        id,
        label,
        glyph_str,
        trailing,
        enabled,
        ThemeRef::Accent.into(),
        ThemeRef::AccentSecondary.into(),
        on_tap,
    )
}

// ===================== Drop zone =====================

/// A large "drop a file here or browse" zone with a native-picker button. While a file is
/// dragged over it, the zone tints accent (`hover`, set from `drag_enter`/`drag_leave`).
#[allow(clippy::too_many_arguments)]
fn drop_zone(
    icon: &'static str,
    heading: &str,
    caption_text: &str,
    on_pick: impl Fn() + 'static,
    set_path: AsyncSetState<String>,
    probe: MutationTrigger<FileProbe>,
    set_output: Option<AsyncSetState<String>>,
    hover: bool,
    set_hover: AsyncSetState<bool>,
) -> Element {
    // TextBlock has no TextAlignment, so a wrapped caption would left-align its second line.
    // The strings are fixed two-sentence copy — render each sentence as its own centred line.
    let (cap1, cap2) = caption_text
        .split_once(". ")
        .map(|(a, b)| (format!("{a}."), b.to_owned()))
        .unwrap_or_else(|| (caption_text.to_owned(), String::new()));
    let mut caption_lines: Vec<Element> = vec![
        text_block(cap1)
            .font_size(13.0)
            .foreground(ThemeRef::SecondaryText)
            .horizontal_alignment(HorizontalAlignment::Center)
            .with_key("cap1")
            .into(),
    ];
    if !cap2.is_empty() {
        caption_lines.push(
            text_block(cap2)
                .font_size(13.0)
                .foreground(ThemeRef::SecondaryText)
                .horizontal_alignment(HorizontalAlignment::Center)
                .with_key("cap2")
                .into(),
        );
    }

    let inner = vstack((
        border(
            glyph(icon, 30.0)
                .foreground(ThemeRef::AccentText)
                .horizontal_alignment(HorizontalAlignment::Center)
                .vertical_alignment(VerticalAlignment::Center),
        )
        .corner_radius(5.0)
        .background(ThemeRef::SystemAttentionBackground)
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
        vstack(caption_lines).spacing(2.0),
        button("Open File\u{2026}")
            .accent()
            .icon(Symbol::Folder)
            .font_size(14.0)
            .padding(Thickness {
                left: 20.0,
                top: 9.0,
                right: 20.0,
                bottom: 9.0,
            })
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

    let enter_hover = set_hover.clone();
    let leave_hover = set_hover.clone();
    border(inner)
        .corner_radius(8.0)
        .border_brush(if hover {
            BrushBinding::from(ThemeRef::Accent)
        } else {
            BrushBinding::from(ThemeRef::ControlStrokeSecondary)
        })
        .border_thickness(Thickness::uniform(1.5))
        .background(if hover {
            BrushBinding::from(ThemeRef::SystemAttentionBackground)
        } else {
            BrushBinding::from(ThemeRef::CardBackground)
        })
        .padding(Thickness {
            left: 24.0,
            top: 40.0,
            right: 24.0,
            bottom: 34.0,
        })
        .allow_drop(true)
        .drag_enter(move |ctx| {
            let op = crate::accept_files(ctx);
            if !matches!(op, DragOperation::None) {
                enter_hover.call(true);
            }
            op
        })
        .drag_leave(move |_| leave_hover.call(false))
        .drag_over(crate::accept_files)
        .drag_drop(move |ctx| {
            set_hover.call(false);
            match ctx.get_storage_items().first() {
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
            }
        })
        .into()
}

// ===================== Stage 1: Source =====================

pub fn stage_source(vm: &Vm) -> (Element, Element) {
    if vm.source.is_empty() {
        let content = page(vec![
            drop_zone(
                GLYPH_FILM,
                "Select your HDR10 source",
                "The master file to copy HDR10 metadata from. Drag a video here, or browse.",
                crate::pick_source_handler(vm),
                vm.set_source.clone(),
                vm.src_trigger.clone(),
                None,
                vm.drag_hover,
                vm.set_drag_hover.clone(),
            )
            .with_key("src-dropzone"),
        ]);
        let disabled = accent_action(
            vm,
            "continue",
            "Continue",
            GLYPH_ARROW_R,
            true,
            false,
            || {},
        );
        return (
            content,
            footer("Pick your HDR10 master to continue", Some(disabled)),
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
            icon_block(
                GLYPH_FILM,
                ThemeRef::SystemAttentionBackground,
                ThemeRef::AccentText,
                40.0,
            ),
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

        // The five editable metadata rows. Keys are STABLE and every row is the same element
        // type (a vstack) whether or not its editor is open — a keyed row that swapped type
        // when the editor opened desynced the reconciler's child mirror at the tail of the
        // list, leaving a duplicate of the last row on screen.
        let mut rows: Vec<Element> = Vec::new();
        for row in 0..META_ROWS.len() {
            if row > 0 {
                rows.push(divider().with_key(format!("div-{row}")));
            }
            rows.push(meta_row(vm, row, &detected, &effective).with_key(format!("row-{row}")));
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
    let action = accent_action(
        vm,
        "continue",
        "Continue",
        GLYPH_ARROW_R,
        true,
        can_continue,
        move || {
            upd.call(|mut s| {
                s.stage = Stage::Encode;
                s
            });
        },
    );
    (content_and_hint(children), footer("", Some(action)))
}

/// Wraps stage children into the page column (helper so both arms return the same shape).
fn content_and_hint(children: Vec<Element>) -> Element {
    page(children)
}

/// A file (or output) card row: icon block, title + detail, optional badge, trailing button.
fn file_card(
    icon: Element,
    title_text: &str,
    detail: &str,
    badge: Option<Element>,
    action: Element,
) -> Element {
    let mut cells: Vec<Element> = vec![
        icon.grid_column(0).with_key("icon"),
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
    card(padded(
        grid(cells)
            .columns([
                GridLength::Auto,
                GridLength::Star(1.0),
                GridLength::Auto,
                GridLength::Auto,
            ])
            .column_spacing(13.0),
        Thickness {
            left: 15.0,
            top: 14.0,
            right: 15.0,
            bottom: 14.0,
        },
    ))
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
    let header = padded(
        grid(cells)
            .columns([
                GridLength::Auto,
                GridLength::Auto,
                GridLength::Star(1.0),
                GridLength::Auto,
            ])
            .column_spacing(12.0),
        Thickness {
            left: 16.0,
            top: 15.0,
            right: 14.0,
            bottom: 15.0,
        },
    );

    // Both branches are a vstack so opening the editor never changes the row's element type
    // (see the keying comment at the row loop); the panel is appended/truncated at the tail,
    // which the reconciler handles cleanly.
    let mut parts: Vec<Element> = vec![header.with_key("hdr").into()];
    if let Some(edit) = editing {
        parts.push(meta_editor(vm, row, edit, detected, overridden).with_key("panel"));
    }
    vstack(parts).spacing(0.0).into()
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
                    // Fill the Star column — auto-sized combos render as uneven widths.
                    .horizontal_alignment(HorizontalAlignment::Stretch)
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
                .grid_column(0)
                .with_key("ed0-prim"),
                combo(
                    "Transfer (EOTF)",
                    &TRANSFER_OPTS,
                    d.transfer,
                    naming::transfer_name,
                    |d, v| d.transfer = v,
                )
                .grid_column(1)
                .with_key("ed0-tf"),
                combo(
                    "Matrix coefficients",
                    &MATRIX_OPTS,
                    d.matrix,
                    naming::matrix_name,
                    |d, v| d.matrix = v,
                )
                .grid_column(2)
                .with_key("ed0-mx"),
            ))
            .columns([
                GridLength::Star(1.0),
                GridLength::Star(1.0),
                GridLength::Star(1.0),
            ])
            .column_spacing(10.0)
            .with_key("ed0-fields")
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
                    cell(d.gx, "Green x", |d, v| d.gx = v as u16)
                        .grid_column(0)
                        .with_key("ed2-gx"),
                    cell(d.gy, "Green y", |d, v| d.gy = v as u16)
                        .grid_column(1)
                        .with_key("ed2-gy"),
                    cell(d.bx, "Blue x", |d, v| d.bx = v as u16)
                        .grid_column(2)
                        .with_key("ed2-bx"),
                    cell(d.by, "Blue y", |d, v| d.by = v as u16)
                        .grid_column(3)
                        .with_key("ed2-by"),
                ))
                .columns([
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                ])
                .column_spacing(10.0)
                .with_key("ed2-row1"),
                grid((
                    cell(d.rx, "Red x", |d, v| d.rx = v as u16)
                        .grid_column(0)
                        .with_key("ed2-rx"),
                    cell(d.ry, "Red y", |d, v| d.ry = v as u16)
                        .grid_column(1)
                        .with_key("ed2-ry"),
                    cell(d.wx, "White x", |d, v| d.wx = v as u16)
                        .grid_column(2)
                        .with_key("ed2-wx"),
                    cell(d.wy, "White y", |d, v| d.wy = v as u16)
                        .grid_column(3)
                        .with_key("ed2-wy"),
                ))
                .columns([
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                    GridLength::Star(1.0),
                ])
                .column_spacing(10.0)
                .with_key("ed2-row2"),
            ))
            .spacing(10.0)
            .into()
        }
        3 => grid((
            num(d.max_lum as f64, "max_luminance (0.0001 cd/m²)", |d, v| {
                d.max_lum = v as u32
            })
            .grid_column(0)
            .with_key("ed3-max"),
            num(d.min_lum as f64, "min_luminance (0.0001 cd/m²)", |d, v| {
                d.min_lum = v as u32
            })
            .grid_column(1)
            .with_key("ed3-min"),
        ))
        .columns([GridLength::Star(1.0), GridLength::Star(1.0)])
        .column_spacing(10.0)
        .with_key("ed3-fields")
        .into(),
        _ => grid((
            num(d.max_cll as f64, "MaxCLL (cd/m²)", |d, v| {
                d.max_cll = v as u16
            })
            .grid_column(0)
            .with_key("ed4-cll"),
            num(d.max_fall as f64, "MaxFALL (cd/m²)", |d, v| {
                d.max_fall = v as u16
            })
            .grid_column(1)
            .with_key("ed4-fall"),
        ))
        .columns([GridLength::Star(1.0), GridLength::Star(1.0)])
        .column_spacing(10.0)
        .with_key("ed4-fields")
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
                .foreground(ThemeRef::SecondaryText)
                .with_key("ed-head"),
            fields.with_key("ed-fields"),
            grid(buttons)
                .columns([GridLength::Star(1.0), GridLength::Auto, GridLength::Auto])
                .column_spacing(9.0)
                .margin(Thickness {
                    left: 0.0,
                    top: 13.0,
                    right: 0.0,
                    bottom: 0.0,
                })
                .with_key("ed-buttons"),
        ))
        .spacing(9.0),
    )
    // Accent-soft, like the mockup's tinted editing panel (SubtleFill read as plain grey).
    .background(ThemeRef::SystemAttentionBackground)
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
                vm.drag_hover,
                vm.set_drag_hover.clone(),
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
            // Per the user: an accent-tinted video-clip icon here (not the mockup's amber
            // warning) — the "No HDR10" badge already carries the caution.
            icon_block(
                GLYPH_FILM,
                ThemeRef::SystemAttentionBackground,
                ThemeRef::AccentText,
                40.0,
            ),
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
                caption(compact_path(&vm.output)).foreground(ThemeRef::SecondaryText),
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
        padded(
            grid(cells)
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .column_spacing(13.0),
            Thickness {
                left: 16.0,
                top: 13.0,
                right: 16.0,
                bottom: 13.0,
            },
        )
    };
    let container_row = padded(
        grid((
            body("Container")
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(0),
            text_block("Matroska (.mkv)")
                .font_size(13.0)
                .foreground(ThemeRef::SecondaryText)
                .grid_column(1),
        ))
        .columns([GridLength::Star(1.0), GridLength::Auto]),
        Thickness {
            left: 16.0,
            top: 13.0,
            right: 16.0,
            bottom: 13.0,
        },
    );
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

    let action = accent_action(
        vm,
        "patch",
        "Patch HDR10",
        GLYPH_SPARKLE,
        false,
        !vm.output.is_empty(),
        crate::run_handler(vm),
    );
    (page(children), Some(footer("", Some(action))))
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
        ProgressRing::indeterminate().width(56.0).height(56.0),
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
            // Explicit thickness — the stock bar renders a couple of pixels thin.
            ProgressBar::new(idx as f64).range(0.0, 5.0).height(6.0),
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
            // Two manually-centred lines: TextBlock has no TextAlignment, so a wrapped
            // block would left-align its second line.
            text_block("Your encode now carries the source's HDR10 signalling —")
                .font_size(13.0)
                .foreground(ThemeRef::SecondaryText)
                .horizontal_alignment(HorizontalAlignment::Center),
            text_block("losslessly, with every frame's timing preserved.")
                .font_size(13.0)
                .foreground(ThemeRef::SecondaryText)
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
    let detail = format!("{} · {} · MKV", compact_path(&out), human_size(size));
    let play = {
        let out = out.clone();
        // Zero padding: with the default control padding the 38px button clips the glyph.
        button("")
            .icon(Symbol::Play)
            .on_click(move || crate::open_path(&out))
            .padding(0.0)
            .width(38.0)
            .height(34.0)
    };
    children.push(
        file_card(
            // Accent tint like the GTK result card (the green hero + ticks already say "done").
            icon_block(
                GLYPH_FILM,
                ThemeRef::SystemAttentionBackground,
                ThemeRef::AccentText,
                42.0,
            ),
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
            padded(
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
                .column_spacing(13.0),
                Thickness {
                    left: 16.0,
                    top: 14.0,
                    right: 16.0,
                    bottom: 14.0,
                },
            )
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

    // If MKVToolNix wasn't available, ffmpeg handled the remux (only permitted for no-B-frame,
    // constant-rate encodes, so timing is still exact). Mirror the GTK banner and note it.
    if summary.used_ffmpeg_fallback {
        children.push(
            InfoBar::new("Remuxed with ffmpeg")
                .message(
                    "mkvmerge wasn't found, so ffmpeg remuxed this file. Timing was reproduced exactly (this encode has no B-frames and a constant frame rate). Install MKVToolNix to use the primary remux path.",
                )
                .warning()
                .is_open(true)
                .is_closable(false)
                .margin(Thickness {
                    left: 0.0,
                    top: 16.0,
                    right: 0.0,
                    bottom: 0.0,
                })
                .with_key("res-fallback")
                .into(),
        );
    }

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
    let open_folder = accent_action(
        vm,
        "open-folder",
        "Open output folder",
        GLYPH_FOLDER_OPEN,
        false,
        true,
        move || crate::reveal_in_folder(&out),
    );
    let bar = border(
        grid((
            another
                .grid_column(0)
                .horizontal_alignment(HorizontalAlignment::Left),
            open_folder.grid_column(1),
        ))
        .columns([GridLength::Star(1.0), GridLength::Auto]),
    )
    // Padding on the Border — a Grid drops it (see `padded`).
    .padding(Thickness {
        left: 22.0,
        top: 16.0,
        right: 22.0,
        bottom: 16.0,
    })
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

    // Stable keys + a fixed element type per row (see the metadata-row loop): a keyed row
    // that swaps type when its editor opens desyncs the reconciler at the tail of the list,
    // duplicating the last row (MP4Box here).
    let mut rows: Vec<Element> = Vec::new();
    for (i, (tool, resolution)) in toolset.all().into_iter().enumerate() {
        if i > 0 {
            rows.push(divider().with_key(format!("div-{i}")));
        }
        rows.push(tool_row(vm, i, tool, resolution).with_key(format!("tool-{i}")));
    }

    let about_row_pad = Thickness {
        left: 16.0,
        top: 13.0,
        right: 16.0,
        bottom: 13.0,
    };
    let about_card = card(
        vstack((
            padded(
                grid((
                    body("nvhvc HDR10 patcher").grid_column(0),
                    text_block(nvhvc_core::VERSION)
                        .font_size(13.0)
                        .foreground(ThemeRef::SecondaryText)
                        .grid_column(1),
                ))
                .columns([GridLength::Star(1.0), GridLength::Auto]),
                about_row_pad,
            ),
            divider(),
            padded(
                grid((
                    body("Default output container").grid_column(0),
                    text_block("Matroska (.mkv)")
                        .font_size(13.0)
                        .foreground(ThemeRef::SecondaryText)
                        .grid_column(1),
                ))
                .columns([GridLength::Star(1.0), GridLength::Auto]),
                about_row_pad,
            ),
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
                bottom: 24.0,
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

// ===================== App menu overlay =====================

/// The "…" app menu, mirroring the GTK popover: a three-way theme icon switcher on top, then
/// icon menu entries (Preferences / Help / About). A custom overlay layer rather than a native
/// `MenuFlyout` — flyout items can't carry icons, custom content, or the segmented theme row.
/// A full-window transparent click-catcher behind the panel dismisses it.
pub fn menu_overlay(vm: &Vm) -> Element {
    // Two click-catchers with a notch left uncovered over the "…" titlebar button: a click
    // there reaches BOTH the catcher and the button (titlebar input routes through the
    // non-client passthrough), and close-then-toggle nets out to "menu never closes". The
    // notch lets the button's own toggle be the only handler in its corner.
    // Transparent (not None) backgrounds: a null background isn't hit-testable in WinUI.
    let close_main = vm.upd.clone();
    let catcher_main: Element = border(text_block(""))
        .background(Color::transparent())
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .vertical_alignment(VerticalAlignment::Stretch)
        .margin(Thickness {
            left: 0.0,
            top: 48.0,
            right: 0.0,
            bottom: 0.0,
        })
        .on_tapped(move || {
            close_main.call(|mut s| {
                s.menu_open = false;
                s
            });
        })
        .with_key("catcher")
        .into();
    let close_top = vm.upd.clone();
    let catcher_top: Element = border(text_block(""))
        .background(Color::transparent())
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .vertical_alignment(VerticalAlignment::Top)
        .height(48.0)
        // The notch spans the "…" button and the caption buttons (the latter are non-client,
        // so the catcher never saw their clicks anyway).
        .margin(Thickness {
            left: 0.0,
            top: 0.0,
            right: 200.0,
            bottom: 0.0,
        })
        .on_tapped(move || {
            close_top.call(|mut s| {
                s.menu_open = false;
                s
            });
        })
        .with_key("catcher-top")
        .into();

    // --- Theme switcher: three equal segments, selected = accent. ---
    // Two nested borders so no background ever changes brush-binding KIND (Direct↔Theme
    // recreates the element mid-interaction — see the menu-item comment): the OUTER border
    // flips Theme(Accent)↔Theme(SubtleFill) for selection, the INNER flips Direct greys for
    // hover/pressed. Mixing them on one border broke the accent overlay on click.
    let theme_btn = |mode: ThemeMode, g: &'static str, label: &'static str, col: i32| -> Element {
        let selected = vm.ui.theme == mode;
        let hovered = vm.ui.hovered_action == Some(label);
        let pressed = vm.ui.pressed_action == Some(label);
        let fg = || -> BrushBinding {
            if selected {
                ThemeRef::custom("TextOnAccentFillColorPrimaryBrush").into()
            } else {
                ThemeRef::PrimaryText.into()
            }
        };
        let upd = vm.upd.clone();
        let enter = vm.upd.clone();
        let exit = vm.upd.clone();
        let press = vm.upd.clone();
        let release = vm.upd.clone();
        let content = vstack((
            glyph(g, 16.0)
                .foreground(fg())
                .horizontal_alignment(HorizontalAlignment::Center),
            text_block(label)
                .font_size(11.0)
                .foreground(fg())
                .horizontal_alignment(HorizontalAlignment::Center),
        ))
        .spacing(3.0)
        // Dimmed content is the "held down" cue (covers glyph + label alike).
        .opacity(if pressed { 0.6 } else { 1.0 });
        let inner = border(content)
            .corner_radius(4.0)
            .background(match (pressed, hovered) {
                (true, _) => Color {
                    a: 64,
                    r: 0,
                    g: 0,
                    b: 0,
                },
                (false, true) => Color {
                    a: 34,
                    r: 145,
                    g: 145,
                    b: 145,
                },
                (false, false) => Color::transparent(),
            })
            .padding(Thickness {
                left: 0.0,
                top: 8.0,
                right: 0.0,
                bottom: 6.0,
            })
            .on_tapped(move || {
                apply_theme(mode);
                upd.call(move |mut s| {
                    s.theme = mode;
                    s
                });
            })
            .on_pointer_entered(move |_| {
                enter.call(move |mut s| {
                    s.hovered_action = Some(label);
                    s
                });
            })
            .on_pointer_exited(move || {
                exit.call(|mut s| {
                    s.hovered_action = None;
                    s.pressed_action = None;
                    s
                });
            })
            .on_pointer_pressed(move |_| {
                press.call(move |mut s| {
                    s.pressed_action = Some(label);
                    s
                });
            })
            .on_pointer_released(move |_| {
                release.call(|mut s| {
                    s.pressed_action = None;
                    s
                });
            });
        border(inner)
            .corner_radius(4.0)
            .background(if selected {
                BrushBinding::from(ThemeRef::Accent)
            } else {
                BrushBinding::from(ThemeRef::SubtleFill)
            })
            .grid_column(col)
            .with_key(label)
            .into()
    };
    let theme_row: Element = grid((
        theme_btn(ThemeMode::System, "\u{E7F8}", "System", 0),
        theme_btn(ThemeMode::Light, "\u{E706}", "Light", 1),
        theme_btn(ThemeMode::Dark, "\u{E708}", "Dark", 2),
    ))
    .columns([
        GridLength::Star(1.0),
        GridLength::Star(1.0),
        GridLength::Star(1.0),
    ])
    .column_spacing(4.0)
    .margin(Thickness {
        left: 4.0,
        top: 4.0,
        right: 4.0,
        bottom: 6.0,
    })
    .with_key("themes")
    .into();

    // --- Icon menu entries; hover via the shared `hovered_action` tracking. ---
    let item = |id: &'static str,
                g: &'static str,
                label: &'static str,
                action: Box<dyn Fn()>|
     -> Element {
        let hovered = vm.ui.hovered_action == Some(id);
        let pressed = vm.ui.pressed_action == Some(id);
        let close = vm.upd.clone();
        let enter = vm.upd.clone();
        let exit = vm.upd.clone();
        let press = vm.upd.clone();
        let release = vm.upd.clone();
        border(
            hstack((
                glyph(g, 15.0).vertical_alignment(VerticalAlignment::Center),
                text_block(label)
                    .font_size(13.0)
                    .vertical_alignment(VerticalAlignment::Center),
            ))
            .spacing(11.0)
            // Dimmed content while held — the Fluent "pressed" cue.
            .opacity(if pressed { 0.6 } else { 1.0 }),
        )
        .corner_radius(4.0)
        // All states are Direct colors: flipping a background between a Direct color and a
        // ThemeRef swaps the brush-binding kind, which recreates the element mid-hover and
        // fires a spurious PointerExited (hover never sticks). A neutral translucent grey
        // reads correctly on both the dark and light panel; pressed goes dimmer than hover.
        .background(match (pressed, hovered) {
            (true, _) => Color {
                a: 20,
                r: 145,
                g: 145,
                b: 145,
            },
            (false, true) => Color {
                a: 34,
                r: 145,
                g: 145,
                b: 145,
            },
            (false, false) => Color::transparent(),
        })
        .padding(Thickness {
            left: 10.0,
            top: 9.0,
            right: 10.0,
            bottom: 9.0,
        })
        .on_tapped(move || {
            action();
            close.call(|mut s| {
                s.menu_open = false;
                s.hovered_action = None;
                s.pressed_action = None;
                s
            });
        })
        .on_pointer_entered(move |_| {
            enter.call(move |mut s| {
                s.hovered_action = Some(id);
                s
            });
        })
        .on_pointer_exited(move || {
            exit.call(|mut s| {
                s.hovered_action = None;
                s.pressed_action = None;
                s
            });
        })
        .on_pointer_pressed(move |_| {
            press.call(move |mut s| {
                s.pressed_action = Some(id);
                s
            });
        })
        .on_pointer_released(move |_| {
            release.call(|mut s| {
                s.pressed_action = None;
                s
            });
        })
        .with_key(id)
        .into()
    };
    let prefs_upd = vm.upd.clone();
    let about_upd = vm.upd.clone();
    let entries: Vec<Element> = vec![
        theme_row,
        border(text_block(""))
            .height(1.0)
            .background(ThemeRef::DividerStroke)
            .margin(Thickness {
                left: 4.0,
                top: 2.0,
                right: 4.0,
                bottom: 2.0,
            })
            .with_key("sep")
            .into(),
        item(
            "menu-prefs",
            "\u{E713}",
            "Preferences",
            Box::new(move || {
                prefs_upd.call(|mut s| {
                    s.prefs_open = true;
                    s
                });
            }),
        ),
        item(
            "menu-help",
            "\u{E897}",
            "Help",
            // Same target as the GTK Help entry.
            Box::new(|| crate::open_path("https://github.com/Mahir007/nvhvc_hdr10-tool")),
        ),
        item(
            "menu-about",
            "\u{E946}",
            "About nvhvc",
            Box::new(move || {
                about_upd.call(|mut s| {
                    s.about_open = true;
                    s
                });
            }),
        ),
    ];

    let panel: Element = border(vstack(entries).spacing(1.0))
        // In-app acrylic per the mockup's translucent flyout (falls back visually to a plain
        // panel if the resource is unavailable on this OS).
        .background(ThemeRef::custom("AcrylicBackgroundFillColorDefaultBrush"))
        .corner_radius(8.0)
        .border_brush(ThemeRef::SurfaceStroke)
        .border_thickness(Thickness::uniform(1.0))
        .padding(Thickness::uniform(5.0))
        .width(240.0)
        .horizontal_alignment(HorizontalAlignment::Right)
        .vertical_alignment(VerticalAlignment::Top)
        .margin(Thickness {
            left: 0.0,
            top: 48.0,
            right: 8.0,
            bottom: 0.0,
        })
        .with_key("panel")
        .into();

    grid(vec![catcher_main, catcher_top, panel]).into()
}

/// The overwrite confirmation, as a smoke overlay so the destructive Replace button can be
/// red (reactor's `ContentDialog` exposes no button styling). Replace sits on the LEFT —
/// deliberately away from the pointer's resting spot — and Cancel is the safe right-hand button.
pub fn overwrite_overlay(vm: &Vm, launch: impl Fn() + 'static) -> Element {
    let name = file_name(&vm.output);

    // A red Border (not a Button): a reactor Button's PointerOver visual state overrides a
    // locally-set Background, so a red button reverts to grey on hover. A Border has no visual
    // states, so it stays reliably red — hover feedback (a lighter red) comes from the shared
    // `hovered_action` tracking instead. Replace being a deliberate tap target rather than a
    // one-keystroke default is a feature. Cancel stays a real button.
    // `launch()` clears `confirm_overwrite` itself (in the same update that starts the run).
    let hovered = vm.ui.hovered_action == Some("replace");
    let pressed = vm.ui.pressed_action == Some("replace");
    let enter = vm.upd.clone();
    let exit = vm.upd.clone();
    let press = vm.upd.clone();
    let release = vm.upd.clone();
    let replace = border(
        text_block("Replace")
            .semibold()
            .foreground(Color::rgb(255, 255, 255))
            .horizontal_alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Center)
            .opacity(if pressed { 0.7 } else { 1.0 }),
    )
    .background(match (pressed, hovered) {
        (true, _) => Color::rgb(160, 35, 23),      // darker while held
        (false, true) => Color::rgb(215, 63, 48),  // lighter critical red on hover
        (false, false) => Color::rgb(196, 43, 28), // Fluent SystemFillColorCritical
    })
    .corner_radius(4.0)
    .on_tapped(launch)
    .on_pointer_entered(move |_| {
        enter.call(|mut s| {
            s.hovered_action = Some("replace");
            s
        });
    })
    .on_pointer_exited(move || {
        exit.call(|mut s| {
            s.hovered_action = None;
            s.pressed_action = None;
            s
        });
    })
    .on_pointer_pressed(move |_| {
        press.call(|mut s| {
            s.pressed_action = Some("replace");
            s
        });
    })
    .on_pointer_released(move |_| {
        release.call(|mut s| {
            s.pressed_action = None;
            s
        });
    })
    .horizontal_alignment(HorizontalAlignment::Stretch)
    .vertical_alignment(VerticalAlignment::Stretch)
    .grid_column(0)
    .with_key("replace");

    let upd = vm.upd.clone();
    let cancel = button("Cancel")
        .on_click(move || {
            upd.call(|mut s| {
                s.confirm_overwrite = false;
                s
            });
        })
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .grid_column(1)
        .with_key("cancel");

    let buttons = grid((replace, cancel))
        .columns([GridLength::Star(1.0), GridLength::Star(1.0)])
        .column_spacing(8.0)
        .margin(Thickness {
            left: 0.0,
            top: 20.0,
            right: 0.0,
            bottom: 0.0,
        });

    let card = border(
        vstack((
            text_block(format!("Replace \u{201C}{name}\u{201D}?"))
                .font_size(18.0)
                .semibold(),
            text_block(
                "A file with this name already exists. Replacing it will overwrite its contents.",
            )
            .font_size(13.0)
            .foreground(ThemeRef::SecondaryText)
            .wrap(),
            buttons,
        ))
        .spacing(6.0),
    )
    .corner_radius(8.0)
    .background(ThemeRef::SolidBackground)
    .border_brush(ThemeRef::SurfaceStroke)
    .border_thickness(Thickness::uniform(1.0))
    .padding(Thickness::uniform(24.0))
    .width(400.0)
    .horizontal_alignment(HorizontalAlignment::Center)
    .vertical_alignment(VerticalAlignment::Center);

    border(card)
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
            let editor = border(
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
            // Accent-soft so the row being edited visibly stands out (mockup tint).
            .background(ThemeRef::SystemAttentionBackground)
            .padding(Thickness {
                left: 15.0,
                top: 14.0,
                right: 15.0,
                bottom: 14.0,
            });
            // vstack wrapper: same element type as the view branch (see the row loop).
            return vstack(vec![Element::from(editor.with_key("edit"))])
                .spacing(0.0)
                .into();
        }
    }

    // green = found on PATH · blue (accent) = custom user path · purple = install dir.
    let (dot, detail): (BrushBinding, String) = match resolution {
        Resolution::OnPath(p) => (
            ThemeRef::SystemSuccess.into(),
            format!("On PATH · {}", p.display()),
        ),
        Resolution::Override(p) => (
            ThemeRef::Accent.into(),
            format!("Custom path · {}", p.display()),
        ),
        Resolution::Fallback(p) => (
            Color::rgb(160, 84, 200).into(),
            format!("Install dir · {}", p.display()),
        ),
        Resolution::OverrideMissing(p) => (
            ThemeRef::SystemCritical.into(),
            format!("Custom path not found · {}", p.display()),
        ),
        Resolution::NotFound => (
            ThemeRef::SystemCritical.into(),
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
    let row = padded(
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
        .column_spacing(12.0),
        Thickness {
            left: 15.0,
            top: 14.0,
            right: 15.0,
            bottom: 14.0,
        },
    );
    vstack(vec![Element::from(row.with_key("view"))])
        .spacing(0.0)
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
