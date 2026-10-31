//! Fluent views: title bar, breadcrumb, the three wizard stages, and the overlays (app menu,
//! Preferences, overwrite confirmation, About).
//!
//! Layout and styling follow the design mockup ("HDR10 Patcher (WinUI 3)"): Mica window,
//! card sections on `CardBackground`/`CardStroke`, accent primary buttons, `InfoBar` banners,
//! and a centred 560px content column per stage.

use windows_reactor::*;

use nvhvc_core::config::Settings;
use nvhvc_core::hevc::patch::Hdr10Metadata;
use nvhvc_core::naming;
use nvhvc_core::tools::{Resolution, Tool, Toolset};

use crate::app::{Message, Patcher, PrefEditor, Slot};
use crate::model::*;
use crate::palette::{Palette, faded, grey};

type Cx = ViewContext<Patcher>;

// Segoe Fluent Icons glyphs.
const GLYPH_CHECK: &str = "\u{E73E}";
/// "Sparkle": the double-sparkle mark (result hero; GLYPH_SPARKLE below is the single star on
/// the Patch button). E78D renders as a broken box on this font revision.
const GLYPH_SPARKLES: &str = "\u{F4A5}";
const GLYPH_CHEVRON: &str = "\u{E76C}";
const GLYPH_MORE: &str = "\u{E712}";
/// Film-strip ("MyMovies"): the video-clip glyph used everywhere a source/encode is shown,
/// mirroring GTK's `video-reel-symbolic` (E714 is a camera, not a clip).
const GLYPH_FILM: &str = "\u{E8B2}";
const GLYPH_UPLOAD: &str = "\u{E898}";
const GLYPH_COLOR: &str = "\u{E790}";
const GLYPH_CLOCK: &str = "\u{E823}";
const GLYPH_AUDIO: &str = "\u{E8D6}";
const GLYPH_ARROW_R: &str = "\u{E72A}";
const GLYPH_SPARKLE: &str = "\u{E735}";
const GLYPH_FOLDER_OPEN: &str = "\u{E838}";

// ===================== Building blocks =====================

/// Text that wraps, like every TextBlock the app has had (button captions don't).
fn text(s: impl Into<String>, size: f64) -> TextBlock {
    TextBlock::new()
        .text(s)
        .font_size(size)
        .text_wrapping(TextWrapping::Wrap)
}

fn body(s: impl Into<String>) -> TextBlock {
    text(s, 14.0)
}

fn body_strong(s: impl Into<String>) -> TextBlock {
    body(s).font_weight(FontWeight::SEMI_BOLD)
}

fn caption(s: impl Into<String>) -> TextBlock {
    text(s, 12.0)
}

/// A glyph inside a Button, which supplies its colour. FontIcon renders at a fixed 20px, so a
/// Viewbox scales it.
fn icon(g: &str, size: f64) -> View {
    Viewbox::new()
        .width(size)
        .height(size)
        .vertical_alignment(VerticalAlignment::Center)
        .slot(ViewboxSlot::Child, FontIcon::new().glyph(g))
}

/// The stock Button padding (11,5,11,6 plus the 1px border), negated: a content margin that lets
/// a narrow button centre its icon, or a margin that gives a button its content's footprint.
fn unpad() -> Thickness {
    Thickness::new(-11.0, -5.0, -11.0, -6.0)
}

/// A standalone glyph in `color`, centred in its slot.
///
/// FontIcon has no foreground and TextBlock no font family, so the colour comes from a disabled
/// text-link Button: its template paints the content with the overridable
/// `HyperlinkButtonForegroundDisabled` resource. The negative margin cancels the button's
/// padding so the glyph keeps its own layout box.
fn glyph(g: &str, size: f64, color: Color) -> View {
    Button::new()
        .style(ButtonStyle::TextLink)
        .is_enabled(false)
        .resource_overrides(
            ResourceOverrides::new().set("HyperlinkButtonForegroundDisabled", color),
        )
        .margin(unpad())
        .horizontal_alignment(HorizontalAlignment::Center)
        .vertical_alignment(VerticalAlignment::Center)
        .content(icon(g, size))
}

/// A Button's content as the stock icon + label pair.
fn icon_label(symbol: Symbol, label: &str) -> View {
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(8.0)
        .children((
            SymbolIcon::new().symbol(symbol),
            TextBlock::new().text(label),
        ))
}

/// `s` in Unicode monospace letters. TextBlock has no font family, so this stands in for a
/// monospace font in the inline-code chip; pair it with an automation name holding `s`.
fn monospace(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mono = match c {
                'A'..='Z' => 0x1D670 + (c as u32 - 'A' as u32),
                'a'..='z' => 0x1D68A + (c as u32 - 'a' as u32),
                '0'..='9' => 0x1D7F6 + (c as u32 - '0' as u32),
                _ => c as u32,
            };
            char::from_u32(mono).unwrap_or(c)
        })
        .collect()
}

/// Lightweight-styling resources for a Subtle button with its own fill per state and a
/// foreground that dims while pressed.
fn subtle_states(
    rest: Color,
    hover: Color,
    pressed: Color,
    fg: Color,
    pressed_fg: f64,
) -> ResourceOverrides {
    // The border brushes match the fills: Subtle buttons paint their 1px border separately.
    ResourceOverrides::new()
        .set("SubtleButtonBackground", rest)
        .set("SubtleButtonBorderBrush", rest)
        .set("SubtleButtonBackgroundPointerOver", hover)
        .set("SubtleButtonBorderBrushPointerOver", hover)
        .set("SubtleButtonBackgroundPressed", pressed)
        .set("SubtleButtonBorderBrushPressed", pressed)
        .set("SubtleButtonForeground", fg)
        .set("SubtleButtonForegroundPointerOver", fg)
        .set("SubtleButtonForegroundPressed", faded(fg, pressed_fg))
}

/// A Fluent card: 8px corners, hairline stroke, card fill.
fn card() -> Border {
    Border::new()
        .corner_radius(8.0)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(1.0)
        .background(ThemeBrush::CardBackground)
}

/// A 1px horizontal divider between card rows.
fn divider(pal: &Palette) -> View {
    Border::new().height(1.0).background(pal.divider).into()
}

/// A bold section heading above a card.
fn section(title: &str) -> View {
    body_strong(title)
        .margin(Thickness::new(2.0, 22.0, 2.0, 9.0))
        .into()
}

/// Places `view` in a grid row.
fn in_row(row: i32, view: impl Into<View>) -> View {
    Border::new().grid_row(row).content(view)
}

/// Places `view` in a grid column.
fn in_column(column: i32, view: impl Into<View>) -> View {
    Border::new().grid_column(column).content(view)
}

/// The rounded icon block at the left of a file card.
fn icon_block(g: &str, pal: &Palette, size: f64) -> View {
    Border::new()
        .corner_radius(5.0)
        .background(pal.attention_bg)
        .width(size)
        .height(size)
        .content(glyph(g, size / 2.0, pal.accent_text))
}

/// The scrollable, centred 560px content column of a stage.
fn page(children: Vec<KeyedView>) -> View {
    ScrollViewer::new().content(
        StackPanel::new()
            .max_width(560.0)
            // Center (not Stretch): with Stretch the column sits relative to the viewport the
            // scrollbar reservation leaves, drifting right of true window centre.
            .horizontal_alignment(HorizontalAlignment::Center)
            .width(560.0)
            .margin(Thickness::new(30.0, 26.0, 30.0, 26.0))
            .keyed_children(children),
    )
}

/// An always-open, non-closable InfoBar banner.
fn banner(
    title: impl Into<String>,
    message: impl Into<String>,
    severity: InfoBarSeverity,
    margin: Thickness,
) -> View {
    InfoBar::new()
        .title(title)
        .message(message)
        .severity(severity)
        .is_open(true)
        .is_closable(false)
        .margin(margin)
        .into()
}

fn top(px: f64) -> Thickness {
    Thickness::new(0.0, px, 0.0, 0.0)
}

// ===================== Window =====================

pub fn root(p: &Patcher, cx: &mut Cx) -> View {
    cx.window_title("HDR10 Patcher");
    cx.window_visuals(
        WindowVisuals::new()
            .backdrop(WindowBackdrop::Mica)
            .client_size(780.0, 760.0)
            .theme(match p.theme {
                ThemeMode::System => WindowTheme::System,
                ThemeMode::Light => WindowTheme::Light,
                ThemeMode::Dark => WindowTheme::Dark,
            }),
    );
    let on_scheme = cx.callback(Message::ColorScheme);
    cx.on_color_scheme(on_scheme);

    let shown = p.shown_stage();
    let processing = p.processing();
    let (content, footer) = match (&p.run, shown) {
        (Run::Running { stage, .. }, _) => (processing_view(&p.palette, *stage), View::empty()),
        (Run::Done(summary), Stage::Result) => stage_result(p, cx, summary),
        (_, Stage::Encode) => stage_encode(p, cx),
        _ => stage_source(p, cx),
    };

    // Stage content and footer are keyed per stage so switching stages swaps the whole subtree
    // (fresh scroll position) instead of morphing one stage's controls into the next's.
    let stage_key = match (shown, processing) {
        (Stage::Encode, true) => "processing",
        (Stage::Source, _) => "source",
        (Stage::Encode, _) => "encode",
        (Stage::Result, _) => "result",
    };
    let main = Grid::new()
        .rows([
            GridLength::Auto,
            GridLength::Auto,
            GridLength::Star(1.0),
            GridLength::Auto,
        ])
        .columns([GridLength::Star(1.0)])
        .keyed_children([
            KeyedView::new("titlebar", title_bar(cx, shown, processing)),
            KeyedView::new("crumbs", in_row(1, breadcrumb(&p.palette, shown))),
            KeyedView::new(format!("content-{stage_key}"), in_row(2, content)),
            KeyedView::new(format!("footer-{stage_key}"), in_row(3, footer)),
        ]);

    // Overlay layers stack in the same grid cell; later children draw on top.
    let mut layers = vec![
        KeyedView::new("app", main),
        // Over the breadcrumb row, in the client area: see `menu_button`.
        KeyedView::new("menu-btn", menu_button(p, cx)),
        KeyedView::new("about", about_dialog(p, cx)),
    ];
    if p.prefs_open {
        layers.push(KeyedView::new("prefs", prefs_overlay(p, cx)));
    }
    if p.confirm_overwrite {
        layers.push(KeyedView::new("overwrite", overwrite_overlay(p, cx)));
    }
    if p.menu_open {
        layers.push(KeyedView::new("menu", menu_overlay(p, cx)));
    }
    Grid::new().keyed_children(layers)
}

/// The window title bar: app title and back button.
fn title_bar(cx: &Cx, shown: Stage, processing: bool) -> View {
    TitleBar::new()
        .title("HDR10 Patcher")
        .is_back_button_visible(shown != Stage::Source && !processing)
        .is_back_button_enabled(true)
        .on_back_requested(cx.message(Message::Back))
        .grid_row(0)
        .into()
}

/// The "…" app-menu toggle, over the right end of the breadcrumb row. It sits in the client
/// area rather than the title bar, where the strip's non-client input handling made every
/// custom control drag the window. Fill ladder per the design: hover (brightest) > open
/// (persistent, between) > pressed (dimmest, glyph dimmed).
fn menu_button(p: &Patcher, cx: &Cx) -> View {
    let rest = if p.menu_open {
        grey(26)
    } else {
        Color::transparent()
    };
    Button::new()
        .style(ButtonStyle::Subtle)
        // Corners stay at the stock 4px: a CornerRadius resource override (for the 5px the
        // custom control had) brings WinUI down a few seconds after it's applied.
        .resource_overrides(subtle_states(
            rest,
            grey(40),
            grey(16),
            p.palette.text_primary,
            0.55,
        ))
        .width(40.0)
        .height(32.0)
        .horizontal_alignment(HorizontalAlignment::Right)
        .vertical_alignment(VerticalAlignment::Top)
        // Vertically centred on the breadcrumb row: the title bar is 32px and the row ~48px
        // (16+20+12), so a 32px button starts at 40. Right margin matches the menu panel's so
        // their edges align.
        .margin(Thickness::new(0.0, 40.0, 12.0, 0.0))
        .automation_name("App menu")
        .on_click(cx.message(Message::ToggleMenu))
        .content(icon(GLYPH_MORE, 14.0))
}

/// The "Source › Encode › Result" step indicator under the title bar.
fn breadcrumb(pal: &Palette, shown: Stage) -> View {
    let index = |s: Stage| match s {
        Stage::Source => 0,
        Stage::Encode => 1,
        Stage::Result => 2,
    };
    let crumb = |label: &str, step: Stage| -> View {
        let label = text(label, 14.0);
        let mut kids = Vec::new();
        let label = match index(shown).cmp(&index(step)) {
            std::cmp::Ordering::Greater => {
                kids.push(KeyedView::new(
                    "check",
                    glyph(GLYPH_CHECK, 12.0, pal.accent_text),
                ));
                label.foreground(ThemeBrush::PrimaryText)
            }
            std::cmp::Ordering::Equal => label
                .font_weight(FontWeight::SEMI_BOLD)
                .foreground(ThemeBrush::PrimaryText),
            std::cmp::Ordering::Less => label.foreground(pal.text_tertiary),
        };
        kids.push(KeyedView::new("label", label));
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(6.0)
            .keyed_children(kids)
    };
    let sep = || glyph(GLYPH_CHEVRON, 11.0, pal.text_tertiary);
    Border::new()
        .border_thickness(Thickness::new(0.0, 0.0, 0.0, 1.0))
        .border_brush(pal.divider)
        // Roomier than the mockup's 12px top: the row also hosts the 32px "…" menu button.
        .padding(Thickness::new(24.0, 16.0, 24.0, 12.0))
        .content(
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .horizontal_alignment(HorizontalAlignment::Center)
                .children((
                    crumb("Source", Stage::Source),
                    sep(),
                    crumb("Encode", Stage::Encode),
                    sep(),
                    crumb("Result", Stage::Result),
                )),
        )
}

/// The footer command bar: hint text on the left, primary action on the right.
fn footer(pal: &Palette, hint: &str, action: Option<View>) -> View {
    let mut cells = vec![KeyedView::new(
        "hint",
        caption(hint)
            .foreground(pal.text_tertiary)
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(0),
    )];
    if let Some(action) = action {
        cells.push(KeyedView::new("action", in_column(1, action)));
    }
    Border::new()
        .padding(Thickness::new(22.0, 16.0, 22.0, 16.0))
        .border_thickness(Thickness::new(0.0, 1.0, 0.0, 0.0))
        .border_brush(pal.divider)
        .background(pal.layer)
        .content(
            Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .keyed_children(cells),
        )
}

/// A primary action button: accent fill, a glyph before or after the label ("Continue →"),
/// 22px × 9px padding. Disabled keeps the accent fill at 40% opacity rather than Fluent's grey.
#[allow(clippy::too_many_arguments)]
fn accent_action(
    pal: &Palette,
    cx: &Cx,
    label: &str,
    glyph_str: &str,
    trailing: bool,
    enabled: bool,
    message: Message,
) -> View {
    let label_view: View = text(label, 14.0)
        .vertical_alignment(VerticalAlignment::Center)
        .into();
    let glyph_view = icon(glyph_str, 13.0);
    let cells = if trailing {
        [label_view, glyph_view]
    } else {
        [glyph_view, label_view]
    };
    Button::new()
        .style(ButtonStyle::Accent)
        .is_enabled(enabled)
        .opacity(if enabled { 1.0 } else { 0.4 })
        .resource_overrides(
            ResourceOverrides::new()
                .set("AccentButtonBackgroundDisabled", pal.accent_fill)
                .set("AccentButtonForegroundDisabled", pal.text_on_accent),
        )
        .automation_name(label)
        .on_click(cx.message(message))
        .content(
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .horizontal_alignment(HorizontalAlignment::Center)
                // With the button's own 1px border and 11,5,11,6 padding: 22,9,22,9 overall.
                .margin(Thickness::new(10.0, 3.0, 10.0, 2.0))
                .children(cells),
        )
}

// ===================== Drop zone =====================

/// A large "drop a file here or browse" zone with a native-picker button. While a file is
/// dragged over it, the zone tints accent.
fn drop_zone(
    p: &Patcher,
    cx: &Cx,
    slot: Slot,
    glyph_str: &str,
    heading: &str,
    caption_text: &str,
) -> View {
    let pal = &p.palette;
    // TextBlock has no TextAlignment, so a wrapped caption would left-align its second line.
    // The strings are fixed two-sentence copy: render each sentence as its own centred line.
    let lines: Vec<String> = match caption_text.split_once(". ") {
        Some((a, b)) => vec![format!("{a}."), b.to_owned()],
        None => vec![caption_text.to_owned()],
    };
    let caption_lines = lines.into_iter().enumerate().map(|(i, line)| {
        KeyedView::new(
            i,
            text(line, 13.0)
                .foreground(pal.text_secondary)
                .horizontal_alignment(HorizontalAlignment::Center),
        )
    });

    let inner = StackPanel::new().spacing(8.0).children((
        Border::new()
            .corner_radius(5.0)
            .background(pal.attention_bg)
            .width(66.0)
            .height(66.0)
            .horizontal_alignment(HorizontalAlignment::Center)
            .margin(Thickness::new(0.0, 0.0, 0.0, 4.0))
            .content(glyph(glyph_str, 30.0, pal.accent_text)),
        text(heading, 20.0)
            .font_weight(FontWeight::SEMI_BOLD)
            .horizontal_alignment(HorizontalAlignment::Center),
        StackPanel::new().spacing(2.0).keyed_children(caption_lines),
        Button::new()
            .style(ButtonStyle::Accent)
            .horizontal_alignment(HorizontalAlignment::Center)
            .margin(top(12.0))
            .automation_name("Open File\u{2026}")
            .on_click(cx.message(Message::Pick(slot)))
            .content(
                // 20,9,20,9 padding overall.
                Border::new()
                    .margin(Thickness::new(9.0, 4.0, 9.0, 3.0))
                    .content(icon_label(Symbol::Folder, "Open File\u{2026}")),
            ),
    ));

    let hover = p.drag_hover;
    Border::new()
        .corner_radius(8.0)
        .border_brush(if hover {
            Brush::from(ThemeBrush::Accent)
        } else {
            Brush::from(pal.control_stroke_secondary)
        })
        .border_thickness(1.5)
        .background(if hover {
            Brush::from(pal.attention_bg)
        } else {
            Brush::from(ThemeBrush::CardBackground)
        })
        .padding(Thickness::new(24.0, 40.0, 24.0, 34.0))
        .drop_policy(
            DragDropPolicy::new().storage_items(
                DragDropAction::new(DragDropOperation::Link).caption("Use this file"),
            ),
        )
        .on_drag_enter(
            cx.callback(|kind: DragKind| Message::DragHover(kind == DragKind::StorageItems)),
        )
        .on_drag_leave(cx.message(Message::DragHover(false)))
        .on_drop(cx.callback(move |data: DroppedData| {
            let path = match data {
                DroppedData::StorageItems(items) => items.into_iter().next().map(|i| i.path),
                _ => None,
            };
            Message::Dropped(slot, path)
        }))
        .content(inner)
}

// ===================== Stage 1: Source =====================

fn stage_source(p: &Patcher, cx: &Cx) -> (View, View) {
    let pal = &p.palette;
    if p.source.is_empty() {
        let content = page(vec![KeyedView::new(
            "src-dropzone",
            drop_zone(
                p,
                cx,
                Slot::Source,
                GLYPH_FILM,
                "Select your HDR10 source",
                "The master file to copy HDR10 metadata from. Drag a video here, or browse.",
            ),
        )]);
        let disabled = accent_action(
            pal,
            cx,
            "Continue",
            GLYPH_ARROW_R,
            true,
            false,
            Message::Continue,
        );
        return (
            content,
            footer(pal, "Pick your HDR10 master to continue", Some(disabled)),
        );
    }

    let probe = p.src_probe.ready();
    let mut children = vec![
        KeyedView::new(
            "src-header",
            body_strong("Source file").margin(Thickness::new(2.0, 0.0, 2.0, 9.0)),
        ),
        KeyedView::new(
            "src-card",
            file_card(
                icon_block(GLYPH_FILM, pal, 40.0),
                &file_name(&p.source),
                &file_detail(&p.source, probe),
                None,
                Button::new()
                    .on_click(cx.message(Message::Pick(Slot::Source)))
                    .content("Change"),
                pal,
            ),
        ),
    ];

    match &p.src_probe {
        Probe::Loading => children.push(KeyedView::new(
            "src-loading",
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(10.0)
                .margin(Thickness::new(2.0, 18.0, 0.0, 0.0))
                .children((
                    ProgressRing::new()
                        .is_indeterminate(true)
                        .is_active(true)
                        .width(20.0)
                        .height(20.0),
                    body("Reading HDR10 metadata\u{2026}").foreground(pal.text_secondary),
                )),
        )),
        Probe::Failed(e) => children.push(KeyedView::new(
            "src-error",
            banner("Probe failed", e.clone(), InfoBarSeverity::Error, top(14.0)),
        )),
        Probe::Idle | Probe::Ready(_) => {}
    }

    if let Some(probe) = probe {
        let detected = probe.hdr.to_metadata();
        let effective = p.overrides.unwrap_or(detected);
        let any_override = effective != detected;

        // "HDR10 metadata" header with "Reset all" while anything is overridden.
        let mut head = vec![KeyedView::new(
            "title",
            body_strong("HDR10 metadata")
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(0),
        )];
        if any_override {
            head.push(KeyedView::new(
                "reset-all",
                Button::new()
                    .style(ButtonStyle::Subtle)
                    .grid_column(1)
                    .automation_name("Reset all")
                    .on_click(cx.message(Message::ResetAll))
                    .content(icon_label(Symbol::Refresh, "Reset all")),
            ));
        }
        children.push(KeyedView::new(
            "meta-header",
            Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .margin(Thickness::new(2.0, 22.0, 2.0, 9.0))
                .keyed_children(head),
        ));

        // The five editable metadata rows.
        let mut rows = Vec::new();
        for row in 0..META_ROWS.len() {
            if row > 0 {
                rows.push(KeyedView::new(format!("div-{row}"), divider(pal)));
            }
            rows.push(KeyedView::new(
                format!("row-{row}"),
                meta_row(p, cx, row, &detected, &effective),
            ));
        }
        children.push(KeyedView::new(
            "meta-card",
            card().content(StackPanel::new().keyed_children(rows)),
        ));

        let status = if any_override {
            banner(
                "Manual overrides active",
                "These values will be written instead of the detected ones.",
                InfoBarSeverity::Warning,
                top(14.0),
            )
        } else if probe.has_hdr10() {
            banner(
                "Valid HDR10 found",
                "Ready to copy into your encode.",
                InfoBarSeverity::Success,
                top(14.0),
            )
        } else {
            banner(
                "No HDR10 signalling detected",
                "This file doesn't look like an HDR10 master; the detected values can still be edited above.",
                InfoBarSeverity::Warning,
                top(14.0),
            )
        };
        children.push(KeyedView::new("src-banner", status));
    }

    let action = accent_action(
        pal,
        cx,
        "Continue",
        GLYPH_ARROW_R,
        true,
        probe.is_some(),
        Message::Continue,
    );
    (page(children), footer(pal, "", Some(action)))
}

/// A file (or output) card row: icon block, title + detail, optional badge, trailing button.
fn file_card(
    icon: View,
    title_text: &str,
    detail: &str,
    badge: Option<View>,
    action: impl Into<View>,
    pal: &Palette,
) -> View {
    let mut cells = vec![
        KeyedView::new("icon", in_column(0, icon)),
        KeyedView::new(
            "info",
            StackPanel::new()
                .spacing(1.0)
                .vertical_alignment(VerticalAlignment::Center)
                .grid_column(1)
                .children((
                    body_strong(title_text),
                    caption(detail).foreground(pal.text_secondary),
                )),
        ),
    ];
    if let Some(badge) = badge {
        cells.push(KeyedView::new("badge", in_column(2, badge)));
    }
    cells.push(KeyedView::new("action", in_column(3, action)));
    card().content(
        Border::new()
            .padding(Thickness::new(15.0, 14.0, 15.0, 14.0))
            .content(
                Grid::new()
                    .columns([
                        GridLength::Auto,
                        GridLength::Star(1.0),
                        GridLength::Auto,
                        GridLength::Auto,
                    ])
                    .column_spacing(13.0)
                    .keyed_children(cells),
            ),
    )
}

/// A small tinted pill badge ("No HDR10", "Edited").
fn pill(label: &str, bg: Color, fg: impl Into<Brush>) -> View {
    Border::new()
        .corner_radius(4.0)
        .background(bg)
        .padding(Thickness::new(8.0, 2.0, 8.0, 2.0))
        .vertical_alignment(VerticalAlignment::Center)
        .content(
            text(label, 11.0)
                .font_weight(FontWeight::SEMI_BOLD)
                .foreground(fg),
        )
}

/// A narrow icon-only button (metadata pencils, the result screen's play button).
fn icon_button(symbol: Symbol, name: &str, width: f64, height: f64, message: Callback<()>) -> View {
    Button::new()
        .width(width)
        .height(height)
        .automation_name(name)
        .on_click(message)
        // The stock padding would leave too little room for the 20px symbol.
        .content(SymbolIcon::new().symbol(symbol).margin(unpad()))
}

/// One metadata row: label · (Edited) · value · edit button, plus the expanded editor.
fn meta_row(
    p: &Patcher,
    cx: &Cx,
    row: usize,
    detected: &Hdr10Metadata,
    effective: &Hdr10Metadata,
) -> View {
    let pal = &p.palette;
    let overridden = row_overridden(row, effective, detected);
    let editing = p.editing.as_ref().filter(|e| e.row == row);

    let mut cells = vec![KeyedView::new(
        "label",
        body(META_ROWS[row])
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(0),
    )];
    if overridden {
        cells.push(KeyedView::new(
            "pill",
            in_column(1, pill("EDITED", pal.subtle, ThemeBrush::AccentText)),
        ));
    }
    let value = text(row_value(row, effective), 13.0)
        .horizontal_alignment(HorizontalAlignment::Right)
        .vertical_alignment(VerticalAlignment::Center)
        .grid_column(2);
    cells.push(KeyedView::new(
        "value",
        if overridden {
            value.foreground(ThemeBrush::AccentText)
        } else {
            value.foreground(pal.text_secondary)
        },
    ));
    if editing.is_none() {
        cells.push(KeyedView::new(
            "edit",
            in_column(
                3,
                icon_button(
                    Symbol::Edit,
                    &format!("Edit {}", META_ROWS[row]),
                    34.0,
                    32.0,
                    cx.message(Message::EditRow(row)),
                ),
            ),
        ));
    }
    let header = Border::new()
        .padding(Thickness::new(16.0, 15.0, 14.0, 15.0))
        .content(
            Grid::new()
                .columns([
                    GridLength::Auto,
                    GridLength::Auto,
                    GridLength::Star(1.0),
                    GridLength::Auto,
                ])
                .column_spacing(12.0)
                .keyed_children(cells),
        );

    let mut parts = vec![KeyedView::new("hdr", header)];
    if let Some(edit) = editing {
        parts.push(KeyedView::new(
            "panel",
            meta_editor(p, cx, row, edit, overridden),
        ));
    }
    StackPanel::new().keyed_children(parts)
}

/// The expanded "Raw HEVC values" editor under a metadata row.
fn meta_editor(p: &Patcher, cx: &Cx, row: usize, edit: &EditState, overridden: bool) -> View {
    let pal = &p.palette;
    let d = edit.draft;

    let num = |value: f64, header: &str, column: i32, apply: fn(&mut Draft, f64)| -> View {
        NumberBox::new()
            .value(value)
            .on_value_changed(cx.callback(move |v| Message::DraftNumber(apply, v)))
            .grid_column(column)
            .slot(NumberBoxSlot::Header, header)
    };
    let combo = |header: &str,
                 opts: &'static [u8],
                 current: u8,
                 namer: fn(u8) -> &'static str,
                 column: i32,
                 apply: fn(&mut Draft, u8)|
     -> View {
        let items: Vec<String> = opts.iter().map(|&c| code_label(c, namer(c))).collect();
        let selected = opts.iter().position(|&c| c == current).unwrap_or(0);
        ComboBox::new()
            .items_source(items)
            .selected_index(selected)
            .on_selection_changed(cx.callback(move |i: Option<usize>| {
                Message::DraftCode(apply, i.and_then(|i| opts.get(i).copied()))
            }))
            // Fill the Star column: auto-sized combos render as uneven widths.
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .grid_column(column)
            .slot(ComboBoxSlot::Header, header)
    };
    let star_columns = |n: usize| vec![GridLength::Star(1.0); n];

    let fields: View = match row {
        0 => Grid::new()
            .columns(star_columns(3))
            .column_spacing(10.0)
            .children((
                combo(
                    "Colour primaries",
                    &PRIMARIES_OPTS,
                    d.primaries,
                    naming::primaries_name,
                    0,
                    |d, v| d.primaries = v,
                ),
                combo(
                    "Transfer (EOTF)",
                    &TRANSFER_OPTS,
                    d.transfer,
                    naming::transfer_name,
                    1,
                    |d, v| d.transfer = v,
                ),
                combo(
                    "Matrix coefficients",
                    &MATRIX_OPTS,
                    d.matrix,
                    naming::matrix_name,
                    2,
                    |d, v| d.matrix = v,
                ),
            )),
        // Full width, matching the GTK editor.
        1 => ComboBox::new()
            .items_source(["0 · Limited (TV)", "1 · Full (PC)"])
            .selected_index(usize::from(d.full_range))
            .on_selection_changed(
                cx.callback(|i: Option<usize>| Message::DraftRange(i.map(|i| i == 1))),
            )
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .slot(ComboBoxSlot::Header, "video_full_range_flag"),
        2 => {
            let cell = |v: u16, h: &str, column: i32, apply: fn(&mut Draft, f64)| {
                num(f64::from(v), h, column, apply)
            };
            StackPanel::new().spacing(10.0).children((
                Grid::new()
                    .columns(star_columns(4))
                    .column_spacing(10.0)
                    .children((
                        cell(d.gx, "Green x", 0, |d, v| d.gx = v as u16),
                        cell(d.gy, "Green y", 1, |d, v| d.gy = v as u16),
                        cell(d.bx, "Blue x", 2, |d, v| d.bx = v as u16),
                        cell(d.by, "Blue y", 3, |d, v| d.by = v as u16),
                    )),
                Grid::new()
                    .columns(star_columns(4))
                    .column_spacing(10.0)
                    .children((
                        cell(d.rx, "Red x", 0, |d, v| d.rx = v as u16),
                        cell(d.ry, "Red y", 1, |d, v| d.ry = v as u16),
                        cell(d.wx, "White x", 2, |d, v| d.wx = v as u16),
                        cell(d.wy, "White y", 3, |d, v| d.wy = v as u16),
                    )),
            ))
        }
        3 => Grid::new()
            .columns(star_columns(2))
            .column_spacing(10.0)
            .children((
                num(
                    f64::from(d.max_lum),
                    "max_luminance (0.0001 cd/m²)",
                    0,
                    |d, v| d.max_lum = v as u32,
                ),
                num(
                    f64::from(d.min_lum),
                    "min_luminance (0.0001 cd/m²)",
                    1,
                    |d, v| d.min_lum = v as u32,
                ),
            )),
        _ => Grid::new()
            .columns(star_columns(2))
            .column_spacing(10.0)
            .children((
                num(f64::from(d.max_cll), "MaxCLL (cd/m²)", 0, |d, v| {
                    d.max_cll = v as u16
                }),
                num(f64::from(d.max_fall), "MaxFALL (cd/m²)", 1, |d, v| {
                    d.max_fall = v as u16
                }),
            )),
    };

    // Reset-to-detected · Cancel · Apply.
    let mut buttons = Vec::new();
    if overridden {
        buttons.push(KeyedView::new(
            "reset",
            Button::new()
                .style(ButtonStyle::Subtle)
                .horizontal_alignment(HorizontalAlignment::Left)
                .grid_column(0)
                .on_click(cx.message(Message::ResetRow))
                .content("Reset to detected"),
        ));
    }
    buttons.push(KeyedView::new(
        "cancel",
        Button::new()
            .grid_column(1)
            .on_click(cx.message(Message::CancelEdit))
            .content("Cancel"),
    ));
    buttons.push(KeyedView::new(
        "apply",
        Button::new()
            .style(ButtonStyle::Accent)
            .grid_column(2)
            .on_click(cx.message(Message::ApplyEdit))
            .content("Apply"),
    ));

    // Accent-soft, like the mockup's tinted editing panel.
    Border::new()
        .background(pal.attention_bg)
        .padding(Thickness::new(16.0, 8.0, 16.0, 14.0))
        .content(
            StackPanel::new().spacing(9.0).children((
                caption("RAW HEVC VALUES")
                    .font_weight(FontWeight::SEMI_BOLD)
                    .foreground(pal.text_secondary),
                fields,
                Grid::new()
                    .columns([GridLength::Star(1.0), GridLength::Auto, GridLength::Auto])
                    .column_spacing(9.0)
                    .margin(top(13.0))
                    .keyed_children(buttons),
            )),
        )
}

// ===================== Stage 2: Encode =====================

fn stage_encode(p: &Patcher, cx: &Cx) -> (View, View) {
    let pal = &p.palette;
    let mut children = Vec::new();

    // "Copying from …" banner.
    let copying = p
        .effective_metadata()
        .map(|m| colour_summary(&m))
        .unwrap_or_default();
    children.push(KeyedView::new(
        "enc-src-banner",
        banner(
            format!("Copying from {}", file_name(&p.source)),
            copying,
            InfoBarSeverity::Informational,
            Thickness::new(0.0, 0.0, 0.0, 22.0),
        ),
    ));

    if p.encode.is_empty() {
        children.push(KeyedView::new(
            "enc-dropzone",
            drop_zone(
                p,
                cx,
                Slot::Encode,
                GLYPH_UPLOAD,
                "Select the encode to patch",
                "The HEVC encode that's missing its HDR10 signalling. Drag a video here, or browse.",
            ),
        ));
        return (
            page(children),
            footer(pal, "Select the encode that needs HDR10", None),
        );
    }

    let enc_probe = p.enc_probe.ready();

    children.push(KeyedView::new(
        "enc-header",
        body_strong("Encode to patch").margin(Thickness::new(2.0, 0.0, 2.0, 9.0)),
    ));
    let badge = enc_probe.map(|probe| {
        if probe.has_hdr10() {
            pill("HDR10 PRESENT", pal.success_bg, pal.success)
        } else {
            pill("NO HDR10", pal.caution_bg, pal.caution)
        }
    });
    children.push(KeyedView::new(
        "enc-card",
        file_card(
            // An accent-tinted video-clip icon here (not the mockup's amber warning): the
            // "No HDR10" badge already carries the caution.
            icon_block(GLYPH_FILM, pal, 40.0),
            &file_name(&p.encode),
            &file_detail(&p.encode, enc_probe),
            badge,
            Button::new()
                .on_click(cx.message(Message::Pick(Slot::Encode)))
                .content("Change"),
            pal,
        ),
    ));

    // Output card. The shown (and patched) path derives its extension from the container
    // selection at render time; the stored path's own extension is never used directly.
    children.push(KeyedView::new("out-header", section("Output")));
    let out_display = effective_output(&p.output, p.container);
    children.push(KeyedView::new(
        "out-card",
        card().content(
            Border::new()
                .padding(Thickness::new(16.0, 13.0, 16.0, 13.0))
                .content(
                    Grid::new()
                        .columns([GridLength::Star(1.0), GridLength::Auto])
                        .column_spacing(13.0)
                        .children((
                            StackPanel::new()
                                .spacing(1.0)
                                .vertical_alignment(VerticalAlignment::Center)
                                .grid_column(0)
                                .children((
                                    body_strong("Save as"),
                                    caption(compact_path(&out_display))
                                        .foreground(pal.text_secondary),
                                )),
                            Button::new()
                                .grid_column(1)
                                .automation_name("Choose\u{2026}")
                                .on_click(cx.message(Message::PickOutput))
                                .content(icon_label(Symbol::Save, "Choose\u{2026}")),
                        )),
                ),
        ),
    ));

    // A missing external tool blocks the run: say so here, before the user reaches the
    // (disabled) Patch button, instead of failing mid-run.
    let missing = p.missing_tools();
    if !missing.is_empty() {
        let body = missing
            .iter()
            .map(|t| format!("{}: {}", t.label(), t.install_hint()))
            .collect::<Vec<_>>()
            .join("\n");
        children.push(KeyedView::new(
            "missing-tools",
            banner(
                "Missing tools for this patch",
                body,
                InfoBarSeverity::Error,
                top(14.0),
            ),
        ));
    }

    children.push(KeyedView::new(
        "enc-note",
        banner(
            "No re-encoding",
            match p.container {
                Container::Mkv => {
                    "Only the SPS VUI and an HDR10 SEI are written. All audio & subtitle tracks are carried over untouched."
                }
                Container::Mp4 => {
                    "Only the SPS VUI and an HDR10 SEI are written. MP4 can't hold every track type: audio is carried over untouched, text subtitles are converted, the rest is left out."
                }
            },
            InfoBarSeverity::Informational,
            top(14.0),
        ),
    ));

    // Run error from a previous attempt.
    if let Run::Failed(e) = &p.run {
        children.push(KeyedView::new(
            "run-error",
            banner("Patch failed", e.clone(), InfoBarSeverity::Error, top(14.0)),
        ));
    }

    let action = accent_action(
        pal,
        cx,
        "Patch HDR10",
        GLYPH_SPARKLE,
        false,
        !p.output.is_empty() && missing.is_empty(),
        Message::Patch,
    );
    (page(children), footer(pal, "", Some(action)))
}

/// The centred processing view (spinner, stage label, progress bar). At the Done stage the
/// spinner gives way to a green checkmark, held briefly (the worker sleeps before delivering
/// the result) so completion registers.
fn processing_view(pal: &Palette, stage: usize) -> View {
    let idx = stage.min(5);
    let label = [
        "Probing source metadata",
        "Extracting HEVC stream",
        "Patching bitstream",
        "Capturing frame timing",
        "Remuxing tracks",
        "Done",
    ][idx];
    let pct = (idx as f64 / 5.0 * 100.0).round();
    let indicator: View = if idx == 5 {
        // Same footprint as the ring: a green circle with a check, like the result hero.
        Border::new()
            .corner_radius(28.0)
            .background(pal.success_bg)
            .width(56.0)
            .height(56.0)
            .horizontal_alignment(HorizontalAlignment::Center)
            .content(glyph(GLYPH_CHECK, 26.0, pal.success))
    } else {
        ProgressRing::new()
            .is_indeterminate(true)
            .is_active(true)
            .width(56.0)
            .height(56.0)
            .into()
    };
    StackPanel::new()
        .spacing(5.0)
        .horizontal_alignment(HorizontalAlignment::Center)
        .vertical_alignment(VerticalAlignment::Center)
        .children((
            indicator,
            text(label, 18.0)
                .font_weight(FontWeight::SEMI_BOLD)
                .horizontal_alignment(HorizontalAlignment::Center)
                .margin(top(24.0)),
            caption("Lossless bitstream edit · timing preserved exactly")
                .foreground(pal.text_secondary)
                .horizontal_alignment(HorizontalAlignment::Center),
            StackPanel::new()
                .spacing(8.0)
                .width(320.0)
                .margin(top(22.0))
                .children((
                    // Explicit thickness: the stock bar renders a couple of pixels thin.
                    ProgressBar::new()
                        .minimum(0.0)
                        .maximum(5.0)
                        .value(idx as f64)
                        .height(6.0),
                    Grid::new()
                        .columns([GridLength::Star(1.0), GridLength::Auto])
                        .children((
                            caption(format!("Stage {} of 6", idx + 1))
                                .foreground(pal.text_tertiary)
                                .grid_column(0),
                            caption(format!("{pct}%"))
                                .foreground(pal.text_tertiary)
                                .grid_column(1),
                        )),
                )),
        ))
}

// ===================== Stage 3: Result =====================

fn stage_result(p: &Patcher, cx: &Cx, summary: &RunSummary) -> (View, View) {
    let pal = &p.palette;
    let mut children = Vec::new();

    // Success header. Sparkles on the accent tint: the green checkmark belongs to the
    // processing view's completion hold, so the result hero celebrates instead of
    // re-confirming.
    children.push(KeyedView::new(
        "res-hero",
        StackPanel::new()
            .spacing(5.0)
            .margin(Thickness::new(0.0, 4.0, 0.0, 24.0))
            .children((
                Border::new()
                    .corner_radius(34.0)
                    .background(pal.attention_bg)
                    .width(68.0)
                    .height(68.0)
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .content(glyph(GLYPH_SPARKLES, 30.0, pal.accent_text)),
                text("HDR10 metadata embedded", 20.0)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .margin(top(14.0)),
                // Two manually centred lines: TextBlock has no TextAlignment, so a wrapped
                // block would left-align its second line.
                text(
                    "Your encode now carries the source's HDR10 signalling,",
                    13.0,
                )
                .foreground(pal.text_secondary)
                .horizontal_alignment(HorizontalAlignment::Center),
                text("losslessly, with every frame's timing preserved.", 13.0)
                    .foreground(pal.text_secondary)
                    .horizontal_alignment(HorizontalAlignment::Center),
            )),
    ));

    // Output file card with a Play button.
    let out = &summary.output;
    let size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    let detail = format!(
        "{} · {} · {}",
        compact_path(out),
        human_size(size),
        std::path::Path::new(out)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_uppercase)
            .unwrap_or_default()
    );
    children.push(KeyedView::new(
        "res-card",
        file_card(
            icon_block(GLYPH_FILM, pal, 42.0),
            &file_name(out),
            &detail,
            None,
            icon_button(
                Symbol::Play,
                "Play",
                38.0,
                34.0,
                cx.message(Message::PlayOutput),
            ),
            pal,
        ),
    ));

    // Summary card.
    children.push(KeyedView::new("sum-header", section("Summary")));
    let mut rows = Vec::new();
    let mut push_row = |g: &str, g_color: Color, label: &str, value: String| {
        if !rows.is_empty() {
            rows.push(KeyedView::new(format!("div-{}", rows.len()), divider(pal)));
        }
        rows.push(KeyedView::new(
            format!("sum-{}", rows.len()),
            Border::new()
                .padding(Thickness::new(16.0, 14.0, 16.0, 14.0))
                .content(
                    Grid::new()
                        .columns([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
                        .column_spacing(13.0)
                        .children((
                            in_column(0, glyph(g, 15.0, g_color)),
                            body(label)
                                .vertical_alignment(VerticalAlignment::Center)
                                .grid_column(1),
                            text(value, 13.0)
                                .foreground(pal.text_secondary)
                                .horizontal_alignment(HorizontalAlignment::Right)
                                .vertical_alignment(VerticalAlignment::Center)
                                .grid_column(2),
                        )),
                ),
        ));
    };
    push_row(
        GLYPH_CHECK,
        pal.success,
        "HDR10 SEI inserted",
        match summary.sei_insertions {
            0 => "already present".to_owned(),
            1 => "ST 2086 + MaxCLL/FALL".to_owned(),
            n => format!("ST 2086 + MaxCLL/FALL · {n} IRAPs"),
        },
    );
    push_row(
        GLYPH_CHECK,
        pal.success,
        "SPS VUI colour rewritten",
        format!("{} SPS · bit-exact", summary.sps_patched),
    );
    if let Some(m) = p.effective_metadata() {
        push_row(GLYPH_COLOR, pal.accent_text, "Colour", colour_summary(&m));
    }
    push_row(
        GLYPH_CLOCK,
        pal.accent_text,
        "Timing",
        match summary.frames {
            Some(n) => format!("{n} frames preserved exactly"),
            None => "preserved exactly".to_owned(),
        },
    );
    push_row(
        GLYPH_AUDIO,
        pal.accent_text,
        "Tracks carried over",
        summary
            .tracks
            .clone()
            .unwrap_or_else(|| "audio & subtitles untouched".to_owned()),
    );
    children.push(KeyedView::new(
        "sum-card",
        card().content(StackPanel::new().keyed_children(rows)),
    ));

    // MP4 output can't hold every track type; list what was left behind (mirrors GTK).
    if !summary.dropped_tracks.is_empty() {
        children.push(KeyedView::new(
            "res-dropped",
            banner(
                "Some tracks were left out",
                format!(
                    "Not carried into MP4 (unsupported there): {}.\nChoose Matroska output to keep every track.",
                    summary.dropped_tracks.join(", ")
                ),
                InfoBarSeverity::Warning,
                top(16.0),
            ),
        ));
    }

    // Footer: Patch another (left) · Open output folder (accent).
    let bar = Border::new()
        .padding(Thickness::new(22.0, 16.0, 22.0, 16.0))
        .border_thickness(Thickness::new(0.0, 1.0, 0.0, 0.0))
        .border_brush(pal.divider)
        .background(pal.layer)
        .content(
            Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .children((
                    Button::new()
                        .style(ButtonStyle::Subtle)
                        .horizontal_alignment(HorizontalAlignment::Left)
                        .grid_column(0)
                        .automation_name("Patch another")
                        .on_click(cx.message(Message::PatchAnother))
                        .content(icon_label(Symbol::Refresh, "Patch another")),
                    in_column(
                        1,
                        accent_action(
                            pal,
                            cx,
                            "Open output folder",
                            GLYPH_FOLDER_OPEN,
                            false,
                            true,
                            Message::RevealOutput,
                        ),
                    ),
                )),
        );

    (page(children), bar)
}

// ===================== Preferences overlay =====================

/// The Preferences dialog, a smoke-layer overlay on top of the app.
fn prefs_overlay(p: &Patcher, cx: &Cx) -> View {
    let pal = &p.palette;
    let toolset = Toolset::discover(&Settings::load().tool_overrides);

    let mut rows = Vec::new();
    for (i, (tool, resolution)) in toolset.all().into_iter().enumerate() {
        if i > 0 {
            rows.push(KeyedView::new(format!("div-{i}"), divider(pal)));
        }
        rows.push(KeyedView::new(
            format!("tool-{i}"),
            tool_row(p, cx, i, tool, resolution),
        ));
    }

    let body_col = StackPanel::new()
        .spacing(3.0)
        .margin(Thickness::new(24.0, 10.0, 24.0, 24.0))
        .children((
            body_strong("External tools"),
            card()
                .margin(top(10.0))
                .content(StackPanel::new().keyed_children(rows)),
            section("Output"),
            card().content(StackPanel::new().children((
                container_pref_row(pal, cx),
                divider(pal),
                suffix_row(p, cx),
                divider(pal),
                folder_row(p, cx),
            ))),
        ));

    let dialog = Border::new()
        .corner_radius(8.0)
        .background(ThemeBrush::SolidBackground)
        .border_brush(pal.surface_stroke)
        .border_thickness(1.0)
        .width(524.0)
        .horizontal_alignment(HorizontalAlignment::Center)
        .vertical_alignment(VerticalAlignment::Center)
        .content(
            StackPanel::new().children((
                text("Preferences", 20.0)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .margin(Thickness::new(24.0, 23.0, 24.0, 5.0)),
                ScrollViewer::new().max_height(420.0).content(body_col),
                Border::new()
                    .border_thickness(Thickness::new(0.0, 1.0, 0.0, 0.0))
                    .border_brush(pal.divider)
                    .padding(20.0)
                    .content(
                        Button::new()
                            .style(ButtonStyle::Accent)
                            .horizontal_alignment(HorizontalAlignment::Stretch)
                            .on_click(cx.message(Message::ClosePrefs))
                            // 12,8,12,8 padding overall.
                            .content(
                                TextBlock::new()
                                    .text("Done")
                                    .margin(Thickness::new(1.0, 3.0, 1.0, 2.0)),
                            ),
                    ),
            )),
        );

    Border::new()
        .background(pal.smoke)
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .vertical_alignment(VerticalAlignment::Stretch)
        .content(dialog)
}

/// "Default output container": persisted on change; per-file switching happens in the Save as
/// dialog instead.
fn container_pref_row(pal: &Palette, cx: &Cx) -> View {
    let current = Container::from_setting(Settings::load().default_container.as_deref());
    let selected = Container::ALL
        .iter()
        .position(|&c| c == current)
        .unwrap_or(0);
    let note = |s: &str| {
        caption(s)
            .foreground(pal.text_secondary)
            .vertical_alignment(VerticalAlignment::Center)
    };
    Border::new()
        .padding(Thickness::new(16.0, 13.0, 16.0, 13.0))
        .content(
            Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .column_spacing(13.0)
                .children((
                    StackPanel::new()
                        .spacing(1.0)
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(0)
                        .children((
                            body("Default output container"),
                            // "Save as" is set off like inline code (mono chip), mirroring the
                            // GTK subtitle's monospace run.
                            StackPanel::new()
                                .orientation(Orientation::Horizontal)
                                .spacing(4.0)
                                .children((
                                    note("You can still switch it per file in the"),
                                    Border::new()
                                        .background(grey(26))
                                        .corner_radius(4.0)
                                        .padding(Thickness::new(5.0, 0.0, 5.0, 1.0))
                                        .vertical_alignment(VerticalAlignment::Center)
                                        .content(
                                            caption(monospace("Save as"))
                                                .foreground(pal.text_secondary)
                                                .automation_name("Save as"),
                                        ),
                                    note("dialog"),
                                )),
                        )),
                    ComboBox::new()
                        .items_source(Container::ALL.iter().map(|c| c.label()))
                        .selected_index(selected)
                        .on_selection_changed(cx.callback(|i: Option<usize>| {
                            Message::SetContainer(i.and_then(|i| Container::ALL.get(i).copied()))
                        }))
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(1),
                )),
        )
}

/// A Preferences row: title, detail line and an Edit button.
fn pref_row(pal: &Palette, cx: &Cx, title: &str, detail: String, edit: Message) -> View {
    Border::new()
        .padding(Thickness::new(16.0, 13.0, 16.0, 13.0))
        .content(
            Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Auto])
                .column_spacing(13.0)
                .children((
                    StackPanel::new()
                        .spacing(1.0)
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(0)
                        .children((body(title), caption(detail).foreground(pal.text_secondary))),
                    Button::new()
                        .grid_column(1)
                        .automation_name(format!("Edit {title}"))
                        .on_click(cx.message(edit))
                        .content(icon_label(Symbol::Edit, "Edit")),
                )),
        )
}

/// A Preferences inline editor: caption, text box, optional Browse, Cancel and Set.
fn pref_editor(
    pal: &Palette,
    cx: &Cx,
    editor: PrefEditor,
    heading: String,
    draft: &str,
    placeholder: &str,
    browse: bool,
) -> View {
    let mut cells = vec![KeyedView::new(
        "input",
        TextBox::new()
            .text(draft)
            .placeholder_text(placeholder)
            .text_wrapping(TextWrapping::Wrap)
            .on_text_changed(cx.callback(move |text| Message::PrefDraft(editor, text)))
            .grid_column(0),
    )];
    if browse {
        cells.push(KeyedView::new(
            "browse",
            Button::new()
                .grid_column(1)
                .on_click(cx.message(Message::BrowseFolder))
                .content("Browse\u{2026}"),
        ));
    }
    let base = if browse { 2 } else { 1 };
    cells.push(KeyedView::new(
        "cancel",
        Button::new()
            .grid_column(base)
            .on_click(cx.message(Message::CancelPref(editor)))
            .content("Cancel"),
    ));
    cells.push(KeyedView::new(
        "set",
        Button::new()
            .style(ButtonStyle::Accent)
            .grid_column(base + 1)
            .on_click(cx.message(Message::SavePref(editor)))
            .content("Set"),
    ));
    let mut columns = vec![GridLength::Star(1.0), GridLength::Auto, GridLength::Auto];
    if browse {
        columns.push(GridLength::Auto);
    }
    // Accent-soft so the row being edited stands out (mockup tint).
    Border::new()
        .background(pal.attention_bg)
        .padding(Thickness::new(15.0, 14.0, 15.0, 14.0))
        .content(
            StackPanel::new().spacing(8.0).children((
                caption(heading).font_weight(FontWeight::SEMI_BOLD),
                Grid::new()
                    .columns(columns)
                    .column_spacing(8.0)
                    .keyed_children(cells),
            )),
        )
}

/// "Default output filename suffix", or its editor. The suffix goes between the encode's name
/// and the extension when auto-naming the output; clearing it resets to
/// [`DEFAULT_OUTPUT_SUFFIX`].
fn suffix_row(p: &Patcher, cx: &Cx) -> View {
    if let Some(draft) = &p.suffix_edit {
        return pref_editor(
            &p.palette,
            cx,
            PrefEditor::Suffix,
            "Default output filename suffix (leave empty for the default)".to_owned(),
            draft,
            DEFAULT_OUTPUT_SUFFIX,
            false,
        );
    }
    let current = Settings::load()
        .default_output_suffix
        .filter(|s| !s.trim().is_empty());
    let detail = current
        .clone()
        .unwrap_or_else(|| DEFAULT_OUTPUT_SUFFIX.to_owned());
    pref_row(
        &p.palette,
        cx,
        "Default output filename suffix",
        detail,
        Message::EditPref(PrefEditor::Suffix, current.unwrap_or_default()),
    )
}

/// "Default output folder", or its editor. Auto-named outputs are saved there; clearing it
/// resets to "same folder as the encode".
fn folder_row(p: &Patcher, cx: &Cx) -> View {
    if let Some(draft) = &p.folder_edit {
        return pref_editor(
            &p.palette,
            cx,
            PrefEditor::Folder,
            "Default output folder (leave empty to save next to the encode)".to_owned(),
            draft,
            r"C:\path\to\output\folder",
            true,
        );
    }
    let current = Settings::load()
        .default_output_dir
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.display().to_string());
    let detail = current
        .clone()
        .unwrap_or_else(|| "Same folder as the encode".to_owned());
    pref_row(
        &p.palette,
        cx,
        "Default output folder",
        detail,
        Message::EditPref(PrefEditor::Folder, current.unwrap_or_default()),
    )
}

/// One tool row in Preferences: status dot, name + resolution detail, Edit button, or the
/// inline custom-path editor when this row is being edited.
fn tool_row(p: &Patcher, cx: &Cx, index: usize, tool: Tool, resolution: &Resolution) -> View {
    let pal = &p.palette;
    if let Some((i, draft)) = &p.tool_edit
        && *i == index
    {
        return pref_editor(
            pal,
            cx,
            PrefEditor::Tool(index),
            format!(
                "Custom {} executable (empty resets to auto-detect)",
                tool.label()
            ),
            draft,
            r"C:\path\to\executable.exe",
            false,
        );
    }

    // green = on PATH · accent = custom user path · purple = install dir · red = missing.
    let (dot, detail): (Brush, String) = match resolution {
        Resolution::OnPath(p) => (pal.success.into(), format!("On PATH · {}", p.display())),
        Resolution::Override(p) => (
            ThemeBrush::Accent.into(),
            format!("Custom path · {}", p.display()),
        ),
        Resolution::Fallback(p) => (
            Color::rgb(160, 84, 200).into(),
            format!("Install dir · {}", p.display()),
        ),
        Resolution::OverrideMissing(p) => (
            ThemeBrush::SystemCritical.into(),
            format!("Custom path not found · {}", p.display()),
        ),
        Resolution::NotFound => (
            ThemeBrush::SystemCritical.into(),
            format!("Not found · {}", tool.install_hint()),
        ),
    };
    let current = resolution
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    Border::new()
        .padding(Thickness::new(15.0, 14.0, 15.0, 14.0))
        .content(
            Grid::new()
                .columns([GridLength::Auto, GridLength::Star(1.0), GridLength::Auto])
                .column_spacing(12.0)
                .children((
                    Border::new()
                        .width(9.0)
                        .height(9.0)
                        .corner_radius(4.5)
                        .background(dot)
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(0),
                    StackPanel::new()
                        .vertical_alignment(VerticalAlignment::Center)
                        .grid_column(1)
                        .children((
                            text(tool.label(), 13.0).font_weight(FontWeight::SEMI_BOLD),
                            caption(detail).foreground(pal.text_tertiary),
                        )),
                    Button::new()
                        .grid_column(2)
                        .automation_name(format!("Edit {}", tool.label()))
                        .on_click(cx.message(Message::EditPref(PrefEditor::Tool(index), current)))
                        .content(icon_label(Symbol::Edit, "Edit")),
                )),
        )
}

// ===================== App menu overlay =====================

/// The "…" app menu, mirroring the GTK popover: a three-way theme switcher on top, then icon
/// menu entries (Preferences / Help / About). A custom overlay rather than a native flyout: menu
/// flyout items can't carry icons or custom rows, and a rich flyout can't be closed when an
/// entry is picked. Transparent click-catchers behind the panel dismiss it.
fn menu_overlay(p: &Patcher, cx: &Cx) -> View {
    let pal = &p.palette;
    // The catchers leave a notch over the "…" button so its own click toggles the menu shut
    // (a click there would otherwise reach both and net out to "never closes"). Three pieces:
    // the title bar strip, the breadcrumb row minus the notch, and everything below.
    // Transparent (not unset) backgrounds: a null background isn't hit-testable.
    let catcher = |height: Option<f64>, margin: Thickness| -> View {
        let catcher = Border::new()
            .background(Color::transparent())
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .margin(margin)
            .on_pointer_pressed(cx.callback(|_: PointerEventInfo| Message::CloseMenu));
        match height {
            Some(h) => catcher
                .height(h)
                .vertical_alignment(VerticalAlignment::Top)
                .into(),
            None => catcher
                .vertical_alignment(VerticalAlignment::Stretch)
                .into(),
        }
    };

    // Theme switcher: three equal segments, selected = accent.
    let theme_button = |mode: ThemeMode, g: &str, label: &str, column: i32| -> View {
        let selected = p.theme == mode;
        let fg = if selected {
            pal.text_on_accent
        } else {
            pal.text_primary
        };
        Border::new()
            .corner_radius(4.0)
            .background(if selected {
                Brush::from(ThemeBrush::Accent)
            } else {
                Brush::from(pal.subtle)
            })
            .grid_column(column)
            .content(
                Button::new()
                    .style(ButtonStyle::Subtle)
                    .resource_overrides(subtle_states(
                        Color::transparent(),
                        grey(34),
                        Color::argb(64, 0, 0, 0),
                        fg,
                        0.6,
                    ))
                    .horizontal_alignment(HorizontalAlignment::Stretch)
                    .vertical_alignment(VerticalAlignment::Stretch)
                    .automation_name(label)
                    .on_click(cx.message(Message::SetTheme(mode)))
                    .content(
                        // 0,8,0,6 padding overall.
                        StackPanel::new()
                            .spacing(3.0)
                            .margin(Thickness::new(0.0, 2.0, 0.0, -1.0))
                            .children((
                                Border::new()
                                    .horizontal_alignment(HorizontalAlignment::Center)
                                    .content(icon(g, 16.0)),
                                text(label, 11.0).horizontal_alignment(HorizontalAlignment::Center),
                            )),
                    ),
            )
    };
    let theme_row = Grid::new()
        .columns(vec![GridLength::Star(1.0); 3])
        .column_spacing(4.0)
        .margin(Thickness::new(4.0, 4.0, 4.0, 6.0))
        .children((
            theme_button(ThemeMode::System, "\u{E7F8}", "System", 0),
            theme_button(ThemeMode::Light, "\u{E706}", "Light", 1),
            theme_button(ThemeMode::Dark, "\u{E708}", "Dark", 2),
        ));

    let item = |g: &str, label: &str, message: Message| -> View {
        Button::new()
            .style(ButtonStyle::Subtle)
            .resource_overrides(subtle_states(
                Color::transparent(),
                grey(34),
                grey(20),
                pal.text_primary,
                0.6,
            ))
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .horizontal_content_alignment(HorizontalAlignment::Left)
            .automation_name(label)
            .on_click(cx.message(message))
            .content(
                // 10,9,10,9 padding overall.
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(11.0)
                    .margin(Thickness::new(-2.0, 3.0, -2.0, 2.0))
                    .children((
                        icon(g, 15.0),
                        text(label, 13.0).vertical_alignment(VerticalAlignment::Center),
                    )),
            )
    };

    let panel = Border::new()
        .background(pal.menu_bg)
        .corner_radius(8.0)
        .border_brush(pal.surface_stroke)
        .border_thickness(1.0)
        .padding(5.0)
        .width(240.0)
        .horizontal_alignment(HorizontalAlignment::Right)
        .vertical_alignment(VerticalAlignment::Top)
        // Anchored under the "…" button at the right end of the breadcrumb row.
        .margin(Thickness::new(0.0, 82.0, 12.0, 0.0))
        .content(
            StackPanel::new().spacing(1.0).children((
                theme_row,
                Border::new()
                    .height(1.0)
                    .background(pal.divider)
                    .margin(Thickness::new(4.0, 2.0, 4.0, 2.0)),
                item("\u{E713}", "Preferences", Message::OpenPrefs),
                item("\u{E897}", "Help", Message::OpenHelp),
                item("\u{E946}", "About nvhvc", Message::OpenAbout),
            )),
        );

    Grid::new().children((
        catcher(Some(32.0), Thickness::new(0.0, 0.0, 0.0, 0.0)),
        // The notch: the "…" button spans ~12..52px from the right edge.
        catcher(Some(48.0), Thickness::new(0.0, 32.0, 90.0, 0.0)),
        catcher(None, top(80.0)),
        panel,
    ))
}

// ===================== Dialogs =====================

/// The overwrite confirmation, a smoke overlay so the destructive Replace button can be red.
/// Replace sits on the left, away from the pointer's resting spot; Cancel is the safe
/// right-hand button.
fn overwrite_overlay(p: &Patcher, cx: &Cx) -> View {
    let pal = &p.palette;
    let name = file_name(&p.output);
    let red = Color::rgb(196, 43, 28); // Fluent SystemFillColorCritical
    let hover = Color::rgb(215, 63, 48);
    let pressed = Color::rgb(160, 35, 23);
    let white = Color::rgb(255, 255, 255);
    let replace = Button::new()
        .resource_overrides(
            ResourceOverrides::new()
                .set("ButtonBackground", red)
                .set("ButtonBorderBrush", red)
                .set("ButtonBackgroundPointerOver", hover)
                .set("ButtonBorderBrushPointerOver", hover)
                .set("ButtonBackgroundPressed", pressed)
                .set("ButtonBorderBrushPressed", pressed)
                .set("ButtonForeground", white)
                .set("ButtonForegroundPointerOver", white)
                .set("ButtonForegroundPressed", faded(white, 0.7)),
        )
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .vertical_alignment(VerticalAlignment::Stretch)
        .grid_column(0)
        .on_click(cx.message(Message::ConfirmReplace))
        .content(text("Replace", 14.0).font_weight(FontWeight::SEMI_BOLD));
    let cancel = Button::new()
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .grid_column(1)
        .on_click(cx.message(Message::CancelReplace))
        .content("Cancel");

    let dialog = Border::new()
        .corner_radius(8.0)
        .background(ThemeBrush::SolidBackground)
        .border_brush(pal.surface_stroke)
        .border_thickness(1.0)
        .padding(24.0)
        .width(400.0)
        .horizontal_alignment(HorizontalAlignment::Center)
        .vertical_alignment(VerticalAlignment::Center)
        .content(StackPanel::new().spacing(6.0).children((
            text(format!("Replace \u{201C}{name}\u{201D}?"), 18.0)
                .font_weight(FontWeight::SEMI_BOLD),
            text(
                "A file with this name already exists.\nReplacing it will overwrite its contents.",
                13.0,
            )
            .foreground(pal.text_secondary),
            Grid::new()
                .columns([GridLength::Star(1.0), GridLength::Star(1.0)])
                .column_spacing(8.0)
                .margin(top(20.0))
                .children((replace, cancel)),
        )));

    Border::new()
        .background(pal.smoke)
        .horizontal_alignment(HorizontalAlignment::Stretch)
        .vertical_alignment(VerticalAlignment::Stretch)
        .content(dialog)
}

fn about_dialog(p: &Patcher, cx: &Cx) -> View {
    ContentDialog::new()
        .title("About nvhvc")
        .close_button_text("OK")
        .is_open(p.about_open)
        .on_closed(cx.callback(|_: ContentDialogResult| Message::AboutClosed))
        // Two balanced ~60-char lines keep the dialog comfortably wide.
        .content(
            TextBlock::new()
                .text(format!(
                    "nvhvc HDR10 patcher {}\n\nInjects HDR10 metadata into HEVC streams without \
                     re-encoding:\nbit-exact SPS/VUI editing, frame timing preserved \
                     exactly.\n\nGPL-3.0",
                    nvhvc_core::VERSION
                ))
                .text_wrapping(TextWrapping::Wrap),
        )
}
