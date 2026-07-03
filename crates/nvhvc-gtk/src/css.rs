//! The app stylesheet: drop zones, cards, pills, banners and the page-dot indicator.
//!
//! Colours come from libadwaita's named palette (`@accent_bg_color`, `@card_bg_color`,
//! `@success_color`, `@warning_color`, …) so light/dark and system accent both apply.

pub const STYLE: &str = r#"
/* ===== drop zone ===== */
.dropzone {
  border: 2px dashed alpha(currentColor, .25);
  border-radius: 16px;
  background: @card_bg_color;
  padding: 40px 24px 34px 24px;
  transition: all .15s ease;
}
/* Highlight only while a drag hovers the zone — not on plain mouse-over. */
.dropzone:drop(active) {
  border-color: @accent_bg_color;
  background: alpha(@accent_bg_color, .08);
}
.drop-icon {
  min-width: 66px;
  min-height: 66px;
  border-radius: 18px;
  background: alpha(@accent_bg_color, .12);
  color: @accent_color;
}
button.open-pill {
  background: @accent_bg_color;
  color: @accent_fg_color;
  border-radius: 8px;
  padding: 9px 20px;
  font-weight: 700;
}
button.open-pill:hover {
  background: shade(@accent_bg_color, 0.92);
}

/* ===== cards & rows ===== */
.file-icon {
  min-width: 40px;
  min-height: 40px;
  border-radius: 10px;
  background: alpha(@accent_bg_color, .12);
  color: @accent_color;
}
.file-icon.warn {
  background: alpha(@warning_color, .16);
  color: @warning_color;
}
.file-icon.ok {
  background: alpha(@success_color, .14);
  color: @success_color;
}
.section-title {
  font-weight: 700;
  font-size: 15px;
}
.value-accent { color: @accent_color; }

/* ===== pills & badges ===== */
.pill {
  border-radius: 999px;
  padding: 3px 10px;
  font-size: 11.5px;
  font-weight: 700;
}
.pill.amber { background: alpha(@warning_color, .16); color: @warning_color; }
.pill.green { background: alpha(@success_color, .14); color: @success_color; }
.edited-pill {
  border-radius: 999px;
  padding: 2px 8px;
  font-size: 10.5px;
  font-weight: 700;
  background: alpha(@accent_bg_color, .12);
  color: @accent_color;
}

/* ===== banners ===== */
.banner {
  border-radius: 11px;
  padding: 11px 14px;
}
.banner.green { background: alpha(@success_color, .14); color: @success_color; }
.banner.amber { background: alpha(@warning_color, .16); color: @warning_color; }
.banner.blue  {
  background: alpha(@accent_bg_color, .12);
  color: @accent_color;
  border: 1px solid alpha(@accent_bg_color, .26);
}
.banner.red   { background: alpha(@error_color, .14); color: @error_color; }

/* ===== metadata row editor ===== */
.editor-area {
  background: alpha(@accent_bg_color, .08);
  padding: 8px 16px 14px 16px;
}
.caption-upper {
  font-size: 11.5px;
  font-weight: 700;
  color: alpha(currentColor, .6);
}

/* ===== page dots ===== */
.dot {
  min-width: 7px;
  min-height: 7px;
  border-radius: 999px;
  background: alpha(currentColor, .25);
  transition: all .25s ease;
}
.dot.active {
  min-width: 22px;
  background: @accent_bg_color;
}
.dots-row {
  padding: 14px 24px;
  border-bottom: 1px solid alpha(currentColor, .08);
}

/* ===== footer bar ===== */
.footer-bar {
  padding: 13px 22px;
  border-top: 1px solid alpha(currentColor, .08);
}

/* ===== result ===== */
.success-circle {
  min-width: 68px;
  min-height: 68px;
  border-radius: 999px;
  background: alpha(@success_color, .14);
  color: @success_color;
}

/* ===== theme selector circles (app menu) ===== */
button.theme-circle {
  min-width: 44px;
  min-height: 44px;
  border-radius: 999px;
  padding: 0;
  border: 1px solid alpha(currentColor, .25);
  background: none;
}
button.theme-circle.selected {
  border: 2px solid @accent_bg_color;
}
button.theme-circle.light { background: #fafafb; }
button.theme-circle.dark { background: #2c2c32; }
button.theme-circle.system {
  background: linear-gradient(135deg, #ffffff 0%, #ffffff 50%, #2c2c32 50%, #2c2c32 100%);
}
.badge-check {
  background: @accent_bg_color;
  color: @accent_fg_color;
  border-radius: 999px;
  padding: 3px;
  border: 2px solid @popover_bg_color;
}
button.menu-entry { padding: 8px 9px; }

/* Disabled primary action keeps its accent tint (mockup style) instead of going grey. */
button.suggested-action:disabled {
  background: alpha(@accent_bg_color, .45);
  color: alpha(@accent_fg_color, .85);
}

/* ===== tool status dots =====
   green = found on PATH · blue = custom user path · purple = well-known install dir */
.tool-dot {
  min-width: 9px;
  min-height: 9px;
  border-radius: 999px;
}
.tool-dot.green  { background: @success_color; }
.tool-dot.blue   { background: @accent_bg_color; }
.tool-dot.purple { background: @purple_3; }
.tool-dot.red    { background: @error_color; }

"#;
