//! Calm dark look: near-black chrome, dark grey panels, a single blue accent, the platform UI
//! font (Segoe UI on Windows) and line icons.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};

pub const TOOLBAR: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
pub const TABS: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
pub const PANEL: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x23, 0x23, 0x23);
pub const LINE: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
pub const ROW_SEL: Color32 = Color32::from_rgb(0x3c, 0x3f, 0x44);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x32, 0x33, 0x36);
pub const ACCENT: Color32 = Color32::from_rgb(0x3d, 0x8e, 0xe6);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(0x2a, 0x6c, 0xb8);
pub const TEXT: Color32 = Color32::from_rgb(0xdc, 0xdc, 0xdc);
pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x95, 0x95, 0x95);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x70, 0x70, 0x70);
pub const GREEN: Color32 = Color32::from_rgb(0x5f, 0xb8, 0x6a);
pub const RED: Color32 = Color32::from_rgb(0xe0, 0x5f, 0x5f);
pub const YELLOW: Color32 = Color32::from_rgb(0xe2, 0xb1, 0x3c);
pub const ORANGE: Color32 = Color32::from_rgb(0xf0, 0x8c, 0x1a);
pub const DIFF_ADD: Color32 = Color32::from_rgb(0x22, 0x38, 0x28);
pub const DIFF_DEL: Color32 = Color32::from_rgb(0x42, 0x24, 0x26);
pub const DIFF_HUNK: Color32 = Color32::from_rgb(0x2a, 0x30, 0x3a);

pub const LANES: [Color32; 8] = [
    ORANGE,
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
    // Semibold falls back to the regular chain (keeps emoji/symbol coverage).
    let mut chain = semibold_fonts;
    chain.extend(defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    defs.families.insert(semibold(), chain);
    ctx.set_fonts(defs);
}

pub fn apply(ctx: &egui::Context) {
    load_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let v = &mut s.visuals;
        v.panel_fill = PANEL;
        v.window_fill = Color32::from_rgb(0x2d, 0x2d, 0x2f);
        v.extreme_bg_color = Color32::from_rgb(0x1c, 0x1c, 0x1c);
        v.faint_bg_color = Color32::from_rgb(0x2f, 0x2f, 0x2f);
        v.code_bg_color = Color32::from_rgb(0x1c, 0x1c, 0x1c);
        v.selection.bg_fill = Color32::from_rgb(0x2a, 0x5a, 0x8e);
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.hyperlink_color = ACCENT;
        v.override_text_color = Some(TEXT);
        v.window_corner_radius = CornerRadius::same(6);
        v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0x45, 0x45, 0x48));
        v.window_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(110) };
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(90) };
        v.menu_corner_radius = CornerRadius::same(5);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x38, 0x38, 0x3b);
        v.widgets.inactive.bg_fill = Color32::from_rgb(0x38, 0x38, 0x3b);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x46, 0x46, 0x4a));
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x43, 0x43, 0x47);
        v.widgets.hovered.bg_fill = Color32::from_rgb(0x43, 0x43, 0x47);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x58, 0x58, 0x5d));
        v.widgets.active.weak_bg_fill = Color32::from_rgb(0x4c, 0x4c, 0x51);
        v.widgets.open.weak_bg_fill = Color32::from_rgb(0x3c, 0x3c, 0x40);
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
