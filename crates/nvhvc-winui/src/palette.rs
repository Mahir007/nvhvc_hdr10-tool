//! Fluent colour tokens as concrete colours.
//!
//! windows-reactor 0.100 exposes eight theme brushes (`ThemeBrush`), and the only way to colour a
//! glyph icon is a Button's lightweight-styling resources, which take colours rather than theme
//! brushes. Every other token comes from here, picked per colour scheme. Values are WinUI's
//! `Common_themeresources_any.xaml`; the accent shades are read from the system.

use windows_reactor::{Color, ColorScheme};

/// The system accent shades the Fluent accent tokens derive from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Accent {
    pub light2: Color,
    pub light3: Color,
    pub dark1: Color,
    pub dark2: Color,
}

impl Default for Accent {
    /// Windows' default blue, for when the system colours can't be read.
    fn default() -> Self {
        Self {
            light2: Color::rgb(0x4C, 0xC2, 0xFF),
            light3: Color::rgb(0x99, 0xEB, 0xFF),
            dark1: Color::rgb(0x00, 0x67, 0xC0),
            dark2: Color::rgb(0x00, 0x3E, 0x92),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Palette {
    pub dark: bool,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub text_on_accent: Color,
    /// AccentTextFillColorPrimary.
    pub accent_text: Color,
    /// AccentFillColorDefault.
    pub accent_fill: Color,
    pub divider: Color,
    pub layer: Color,
    /// SubtleFillColorSecondary.
    pub subtle: Color,
    pub smoke: Color,
    pub surface_stroke: Color,
    pub control_stroke_secondary: Color,
    pub attention_bg: Color,
    pub success: Color,
    pub success_bg: Color,
    pub caution: Color,
    pub caution_bg: Color,
    /// The solid fallback of AcrylicBackgroundFillColorDefaultBrush (in-app acrylic never
    /// rendered as acrylic under reactor, so the menu panel has always shown this).
    pub menu_bg: Color,
}

const fn argb(v: u32) -> Color {
    Color::argb((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Palette {
    pub fn new(scheme: ColorScheme, accent: Accent) -> Self {
        match scheme {
            ColorScheme::Dark => Self {
                dark: true,
                text_primary: argb(0xFFFFFFFF),
                text_secondary: argb(0xC5FFFFFF),
                text_tertiary: argb(0x87FFFFFF),
                text_on_accent: argb(0xFF000000),
                accent_text: accent.light3,
                accent_fill: accent.light2,
                divider: argb(0x15FFFFFF),
                layer: argb(0x4C3A3A3A),
                subtle: argb(0x0FFFFFFF),
                smoke: argb(0x4D000000),
                surface_stroke: argb(0x66757575),
                control_stroke_secondary: argb(0x18FFFFFF),
                attention_bg: argb(0x08FFFFFF),
                success: argb(0xFF6CCB5F),
                success_bg: argb(0xFF393D1B),
                caution: argb(0xFFFCE100),
                caution_bg: argb(0xFF433519),
                menu_bg: argb(0xFF2C2C2C),
            },
            _ => Self {
                dark: false,
                text_primary: argb(0xE4000000),
                text_secondary: argb(0x9E000000),
                text_tertiary: argb(0x72000000),
                text_on_accent: argb(0xFFFFFFFF),
                accent_text: accent.dark2,
                accent_fill: accent.dark1,
                divider: argb(0x0F000000),
                layer: argb(0x80FFFFFF),
                subtle: argb(0x09000000),
                smoke: argb(0x4D000000),
                surface_stroke: argb(0x66757575),
                control_stroke_secondary: argb(0x29000000),
                attention_bg: argb(0x80F6F6F6),
                success: argb(0xFF0F7B0F),
                success_bg: argb(0xFFDFF6DD),
                caution: argb(0xFF9D5D00),
                caution_bg: argb(0xFFFFF4CE),
                menu_bg: argb(0xFFF9F9F9),
            },
        }
    }
}

/// `c` with its alpha scaled by `opacity` (a "dimmed" foreground for pressed states).
pub fn faded(c: Color, opacity: f64) -> Color {
    Color::argb((f64::from(c.a) * opacity).round() as u8, c.r, c.g, c.b)
}

/// The translucent neutral grey the custom buttons layer over any background.
pub const fn grey(alpha: u8) -> Color {
    Color::argb(alpha, 145, 145, 145)
}
