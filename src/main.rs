#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod appicon;
mod catalog;
mod catalog_en;
mod git;
mod graph;
mod i18n;
mod icons;
mod theme;
mod winicon;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut screenshot: Option<PathBuf> = None;
    let mut repo: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--repo" => repo = args.next().map(PathBuf::from),
            other if !other.starts_with("--") && repo.is_none() => repo = Some(PathBuf::from(other)),
            _ => {}
        }
    }

    winicon::set_app_id();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("YAGUI")
        .with_app_id("YAGUI.GitClient")
        .with_icon(std::sync::Arc::new(eframe::egui::IconData { rgba: appicon::icon_pixels(64), width: 64, height: 64 }))
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 560.0]);
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(eframe::egui::WindowLevel::AlwaysOnBottom);
        }
    }

    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        options.event_loop_builder = Some(Box::new(|b| {
            use eframe::winit::platform::macos::EventLoopBuilderExtMacOS;
            b.with_activate_ignoring_other_apps(false);
        }));
    }

    eframe::run_native(
        "YAGUI",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, repo, screenshot)))),
    )
}
