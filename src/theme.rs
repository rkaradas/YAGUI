//! Calm dark look: near-black chrome, dark grey panels, a single blue accent, the platform UI
//! font (Segoe UI on Windows) and line icons.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};

/// Whether the light theme is on (Window > Switch Theme, Appearance > Theme).
static LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn set_light(on: bool) { LIGHT.store(on, std::sync::atomic::Ordering::Relaxed); }
/// A dark-theme fill colour and its light-theme counterpart (same tint, light background).
pub fn dk(r: u8, g: u8, b: u8) -> Color32 {
    if !is_light() {
        return Color32::from_rgb(r, g, b);
    }
    let avg = (r as i32 + g as i32 + b as i32) / 3;
    let base = 0xF4 - (avg - 0x20) / 2;
    let f = |c: u8| (base + (c as i32 - avg) * 2).clamp(0, 255) as u8;
    Color32::from_rgb(f(r), f(g), f(b))
}

pub fn is_light() -> bool { LIGHT.load(std::sync::atomic::Ordering::Relaxed) }

#[allow(non_snake_case)]
pub fn TOOLBAR() -> Color32 { if is_light() { Color32::from_rgb(0xee, 0xee, 0xee) } else { Color32::from_rgb(0x1f, 0x1f, 0x1f) } }
#[allow(non_snake_case)]
pub fn TABS() -> Color32 { if is_light() { Color32::from_rgb(0xe4, 0xe4, 0xe4) } else { Color32::from_rgb(0x1a, 0x1a, 0x1a) } }
#[allow(non_snake_case)]
pub fn SIDEBAR() -> Color32 { if is_light() { Color32::from_rgb(0xf3, 0xf3, 0xf3) } else { Color32::from_rgb(0x26, 0x26, 0x26) } }
#[allow(non_snake_case)]
pub fn PANEL() -> Color32 { if is_light() { Color32::from_rgb(0xff, 0xff, 0xff) } else { Color32::from_rgb(0x2b, 0x2b, 0x2b) } }
#[allow(non_snake_case)]
pub fn PANEL_DARK() -> Color32 { if is_light() { Color32::from_rgb(0xf7, 0xf7, 0xf7) } else { Color32::from_rgb(0x23, 0x23, 0x23) } }
#[allow(non_snake_case)]
pub fn LINE() -> Color32 { if is_light() { Color32::from_rgb(0xdc, 0xdc, 0xdc) } else { Color32::from_rgb(0x38, 0x38, 0x38) } }
#[allow(non_snake_case)]
pub fn ROW_SEL() -> Color32 { if is_light() { Color32::from_rgb(0xd6, 0xe6, 0xfb) } else { Color32::from_rgb(0x3c, 0x3f, 0x44) } }
#[allow(non_snake_case)]
pub fn ROW_HOVER() -> Color32 { if is_light() { Color32::from_rgb(0xec, 0xf1, 0xf8) } else { Color32::from_rgb(0x32, 0x33, 0x36) } }
#[allow(non_snake_case)]
pub fn ACCENT() -> Color32 { if is_light() { Color32::from_rgb(0x1f, 0x74, 0xd6) } else { Color32::from_rgb(0x3d, 0x8e, 0xe6) } }
#[allow(non_snake_case)]
pub fn ACCENT_DARK() -> Color32 { if is_light() { Color32::from_rgb(0x1f, 0x6b, 0xc6) } else { Color32::from_rgb(0x2a, 0x6c, 0xb8) } }
#[allow(non_snake_case)]
pub fn TEXT() -> Color32 { if is_light() { Color32::from_rgb(0x24, 0x24, 0x24) } else { Color32::from_rgb(0xdc, 0xdc, 0xdc) } }
#[allow(non_snake_case)]
pub fn TEXT_STRONG() -> Color32 { if is_light() { Color32::from_rgb(0x0a, 0x0a, 0x0a) } else { Color32::from_rgb(0xf2, 0xf2, 0xf2) } }
#[allow(non_snake_case)]
pub fn TEXT_DIM() -> Color32 { if is_light() { Color32::from_rgb(0x6a, 0x6a, 0x6a) } else { Color32::from_rgb(0x95, 0x95, 0x95) } }
#[allow(non_snake_case)]
pub fn TEXT_FAINT() -> Color32 { if is_light() { Color32::from_rgb(0x96, 0x96, 0x96) } else { Color32::from_rgb(0x70, 0x70, 0x70) } }
#[allow(non_snake_case)]
pub fn GREEN() -> Color32 { if is_light() { Color32::from_rgb(0x2e, 0x9a, 0x45) } else { Color32::from_rgb(0x5f, 0xb8, 0x6a) } }
#[allow(non_snake_case)]
pub fn RED() -> Color32 { if is_light() { Color32::from_rgb(0xc9, 0x3c, 0x3c) } else { Color32::from_rgb(0xe0, 0x5f, 0x5f) } }
#[allow(non_snake_case)]
pub fn YELLOW() -> Color32 { if is_light() { Color32::from_rgb(0xc0, 0x8a, 0x10) } else { Color32::from_rgb(0xe2, 0xb1, 0x3c) } }
#[allow(non_snake_case)]
pub fn ORANGE() -> Color32 { if is_light() { Color32::from_rgb(0xe0, 0x7a, 0x10) } else { Color32::from_rgb(0xf0, 0x8c, 0x1a) } }
#[allow(non_snake_case)]
pub fn DIFF_ADD() -> Color32 { if is_light() { Color32::from_rgb(0xdd, 0xf4, 0xe2) } else { Color32::from_rgb(0x22, 0x38, 0x28) } }
#[allow(non_snake_case)]
pub fn DIFF_DEL() -> Color32 { if is_light() { Color32::from_rgb(0xfb, 0xe2, 0xe2) } else { Color32::from_rgb(0x42, 0x24, 0x26) } }
#[allow(non_snake_case)]
pub fn DIFF_HUNK() -> Color32 { if is_light() { Color32::from_rgb(0xe6, 0xee, 0xfa) } else { Color32::from_rgb(0x2a, 0x30, 0x3a) } }
#[allow(non_snake_case)]
pub fn DIFF_ADD_FG() -> Color32 { if is_light() { Color32::from_rgb(0x1d, 0x5e, 0x2a) } else { Color32::from_rgb(0xc4, 0xe8, 0xc9) } }
#[allow(non_snake_case)]
pub fn DIFF_DEL_FG() -> Color32 { if is_light() { Color32::from_rgb(0x8a, 0x1f, 0x1f) } else { Color32::from_rgb(0xf0, 0xc0, 0xc0) } }
#[allow(non_snake_case)]
pub fn DIFF_HUNK_FG() -> Color32 { if is_light() { Color32::from_rgb(0x3a, 0x5a, 0x86) } else { Color32::from_rgb(0x8f, 0xa9, 0xc7) } }
/// The empty side of a side-by-side row (no line there) and its hatching.
#[allow(non_snake_case)]
pub fn DIFF_FILLER() -> Color32 { if is_light() { Color32::from_rgb(0xf1, 0xf1, 0xf1) } else { Color32::from_rgb(0x27, 0x27, 0x27) } }
#[allow(non_snake_case)]
pub fn DIFF_HATCH() -> Color32 { if is_light() { Color32::from_rgb(0xdc, 0xdc, 0xdc) } else { Color32::from_rgb(0x36, 0x36, 0x36) } }

pub const LANES: [Color32; 8] = [
    Color32::from_rgb(0xf0, 0x8c, 0x1a),
    Color32::from_rgb(0x3d, 0x8e, 0xe6),
    Color32::from_rgb(0x6c, 0xc6, 0x4f),
    Color32::from_rgb(0xb3, 0x7f, 0xeb),
    Color32::from_rgb(0xe8, 0x5d, 0x75),
    Color32::from_rgb(0x2f, 0xc4, 0xb2),
    Color32::from_rgb(0xe8, 0xd2, 0x4a),
    Color32::from_rgb(0xf2, 0x7e, 0xc2),
];

pub fn lane_color(i: usize) -> Color32 {
    LANES[i % LANES.len()]
}

pub fn avatar_color(name: &str) -> Color32 {
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let palette = [
        Color32::from_rgb(0xb0, 0x6f, 0xd8),
        Color32::from_rgb(0x4f, 0x8c, 0xd8),
        Color32::from_rgb(0x46, 0xa8, 0x7c),
        Color32::from_rgb(0xd0, 0x7e, 0x44),
        Color32::from_rgb(0xc9, 0x55, 0x73),
        Color32::from_rgb(0x43, 0xa8, 0xb8),
    ];
    palette[(h as usize) % palette.len()]
}

/// Font family for headings and emphasised labels.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

fn load_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let dir = std::env::var_os("WINDIR").map(|w| std::path::PathBuf::from(w).join("Fonts"));
    let read = |name: &str| dir.as_ref().and_then(|d| std::fs::read(d.join(name)).ok());
    let mut semibold_fonts = Vec::new();
    if let Some(b) = read("segoeui.ttf") {
        defs.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(b)));
        defs.families.entry(FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    if let Some(b) = read("seguisb.ttf") {
        defs.font_data.insert("segoe-sb".into(), Arc::new(FontData::from_owned(b)));
        semibold_fonts.push("segoe-sb".to_string());
    }
    if let Some(b) = read("consola.ttf") {
        defs.font_data.insert("consolas".into(), Arc::new(FontData::from_owned(b)));
        defs.families.entry(FontFamily::Monospace).or_default().insert(0, "consolas".into());
    }
    // macOS: the shortcut symbols ⌘ ⇧ ⌥ ⌃ (i18n::keys) are not in egui's own fonts; Apple
    // Symbols has them. Last in the chains, so it only fills in missing characters.
    if cfg!(target_os = "macos") {
        if let Ok(b) = std::fs::read("/System/Library/Fonts/Apple Symbols.ttf") {
            defs.font_data.insert("apple-symbols".into(), Arc::new(FontData::from_owned(b)));
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                defs.families.entry(family).or_default().push("apple-symbols".into());
            }
        }
    }
    // Semibold falls back to the regular chain (keeps emoji/symbol coverage).
    let mut chain = semibold_fonts;
    chain.extend(defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    defs.families.insert(semibold(), chain);
    ctx.set_fonts(defs);
}

pub fn apply(ctx: &egui::Context) {
    load_fonts(ctx);
    apply_visuals(ctx);
}

/// Sets egui's colours for the current theme (call again after switching).
pub fn apply_visuals(ctx: &egui::Context) {
    let light = is_light();
    let theme = if light { egui::Theme::Light } else { egui::Theme::Dark };
    ctx.set_theme(theme);
    let g = |d: u8, l: u8| if light { Color32::from_gray(l) } else { Color32::from_gray(d) };
    ctx.style_mut_of(theme, |s| {
        let v = &mut s.visuals;
        v.panel_fill = PANEL();
        v.window_fill = g(0x2d, 0xfa);
        v.extreme_bg_color = g(0x1c, 0xff);
        v.faint_bg_color = g(0x2f, 0xf4);
        v.code_bg_color = g(0x1c, 0xf4);
        v.selection.bg_fill = if light { Color32::from_rgb(0xb8, 0xd4, 0xf6) } else { Color32::from_rgb(0x2a, 0x5a, 0x8e) };
        v.selection.stroke = Stroke::new(1.0, if light { Color32::BLACK } else { Color32::WHITE });
        v.hyperlink_color = ACCENT();
        v.override_text_color = Some(TEXT());
        v.window_corner_radius = CornerRadius::same(6);
        v.window_stroke = Stroke::new(1.0, g(0x45, 0xcc));
        v.window_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(if light { 50 } else { 110 }) };
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(if light { 40 } else { 90 }) };
        v.menu_corner_radius = CornerRadius::same(5);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE());
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT());
        v.widgets.inactive.weak_bg_fill = g(0x38, 0xe8);
        v.widgets.inactive.bg_fill = g(0x38, 0xe8);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, g(0x46, 0xcc));
        v.widgets.hovered.weak_bg_fill = g(0x43, 0xdd);
        v.widgets.hovered.bg_fill = g(0x43, 0xdd);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, g(0x58, 0xb8));
        v.widgets.active.weak_bg_fill = g(0x4c, 0xd2);
        v.widgets.open.weak_bg_fill = g(0x3c, 0xe2);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open, &mut v.widgets.noninteractive] {
            w.corner_radius = CornerRadius::same(4);
        }
        s.spacing.item_spacing = egui::vec2(8.0, 5.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        s.spacing.menu_margin = egui::Margin::same(6);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(13.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
        s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.5));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::new(16.0, semibold()));
    });
}
