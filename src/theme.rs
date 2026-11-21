//! Dark look: near-black toolbar, dark grey panels, blue accent, orange first graph lane.

use eframe::egui::{self, Color32, CornerRadius, Stroke};

pub const TOOLBAR: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x25, 0x25, 0x26);
pub const PANEL: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
pub const ROW_SEL: Color32 = Color32::from_rgb(0x3a, 0x3d, 0x41);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x33, 0x35, 0x38);
pub const ACCENT: Color32 = Color32::from_rgb(0x3d, 0x9b, 0xf0);
pub const TEXT: Color32 = Color32::from_rgb(0xe4, 0xe4, 0xe4);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);
pub const GREEN: Color32 = Color32::from_rgb(0x5f, 0xc1, 0x6a);
pub const RED: Color32 = Color32::from_rgb(0xe5, 0x5b, 0x5b);
pub const YELLOW: Color32 = Color32::from_rgb(0xe8, 0xb3, 0x3a);
pub const ORANGE: Color32 = Color32::from_rgb(0xf0, 0x8c, 0x1a);
pub const DIFF_ADD: Color32 = Color32::from_rgb(0x23, 0x3d, 0x2a);
pub const DIFF_DEL: Color32 = Color32::from_rgb(0x48, 0x25, 0x27);
pub const DIFF_HUNK: Color32 = Color32::from_rgb(0x26, 0x34, 0x45);

pub const LANES: [Color32; 8] = [
    ORANGE,
    Color32::from_rgb(0x3d, 0x9b, 0xf0),
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
        Color32::from_rgb(0xc0, 0x7c, 0xe8),
        Color32::from_rgb(0x5b, 0x9b, 0xe8),
        Color32::from_rgb(0x52, 0xb7, 0x8a),
        Color32::from_rgb(0xe0, 0x8b, 0x4f),
        Color32::from_rgb(0xd9, 0x5f, 0x7f),
        Color32::from_rgb(0x4f, 0xb8, 0xc9),
    ];
    palette[(h as usize) % palette.len()]
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let v = &mut s.visuals;
        v.panel_fill = PANEL;
        v.window_fill = Color32::from_rgb(0x2e, 0x2e, 0x30);
        v.extreme_bg_color = Color32::from_rgb(0x1b, 0x1b, 0x1b);
        v.faint_bg_color = Color32::from_rgb(0x30, 0x30, 0x30);
        v.selection.bg_fill = Color32::from_rgb(0x2c, 0x5d, 0x93);
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.hyperlink_color = ACCENT;
        v.override_text_color = Some(TEXT);
        v.window_corner_radius = CornerRadius::same(8);
        v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0x44, 0x44, 0x48));
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3a, 0x3a));
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x3d);
        v.widgets.inactive.bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x3d);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x48, 0x48, 0x4c);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(0x2c, 0x5d, 0x93);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = CornerRadius::same(4);
        }
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.5));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
        s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.5));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(18.0));
    });
}
