//! Help > Check for Updates, the daily automatic check, and the setup/uninstall dialogs.

use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, RichText, Vec2};

use super::App;
use crate::i18n::{tr, trs};
use crate::updater::{self, Release};
use crate::{installer, theme, version};

/// How YAGUI was started: normally, as setup, or to uninstall (from Settings > Apps).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StartMode {
    Normal,
    Install,
    Uninstall,
}

pub static START_MODE: OnceLock<StartMode> = OnceLock::new();

pub enum Msg {
    Checked(Result<Option<Release>, String>, bool),
    Installed(Result<std::path::PathBuf, String>),
}

#[derive(Default)]
pub enum UState {
    #[default]
    Idle,
    Checking,
    UpToDate(Option<Release>),
    Available(Release),
    Failed(String),
    Downloading(Release),
    Ready(Release),
}

#[derive(Default)]
pub struct UpdateUi {
    pub open: bool,
    pub state: UState,
    rx: Option<Receiver<Msg>>,
    /// Found by the automatic check; shown in the status bar.
    pub notice: Option<Release>,
}

pub enum SetupMode {
    Install,
    Uninstall,
}

pub struct SetupDialog {
    mode: SetupMode,
    opts: installer::Options,
    remove_settings: bool,
    result: Option<Result<std::path::PathBuf, String>>,
}

impl SetupDialog {
    pub fn install() -> Self {
        SetupDialog { mode: SetupMode::Install, opts: installer::Options::default(), remove_settings: false, result: None }
    }
    pub fn uninstall() -> Self {
        SetupDialog { mode: SetupMode::Uninstall, opts: installer::Options::default(), remove_settings: false, result: None }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl App {
    /// Starts a check in the background. `silent`: only tell when there is something new.
    pub(super) fn check_updates(&mut self, silent: bool, ctx: &egui::Context) {
        if matches!(self.upd.state, UState::Checking | UState::Downloading(_)) {
            return;
        }
        let (tx, rx) = channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Checked(updater::latest(), silent));
            c.request_repaint();
        });
        self.upd.rx = Some(rx);
        if !silent {
            self.upd.state = UState::Checking;
        }
        self.p.last_update_check = now_secs();
        self.save();
    }

    /// Daily automatic check (Preferences > Updates).
    pub(super) fn auto_update_check(&mut self, ctx: &egui::Context) {
        if self.screenshot.is_some() || !self.p.auto_update || self.upd.rx.is_some() {
            return;
        }
        if now_secs().saturating_sub(self.p.last_update_check) > 24 * 3600 {
            self.check_updates(true, ctx);
        }
    }

    pub(super) fn poll_updates(&mut self) {
        let Some(rx) = &self.upd.rx else { return };
        let Ok(m) = rx.try_recv() else { return };
        self.upd.rx = None;
        match m {
            Msg::Checked(Ok(Some(r)), silent) => {
                if version::is_newer(&r.tag) {
                    if silent {
                        self.upd.notice = Some(r.clone());
                    }
                    if !silent || self.upd.open {
                        self.upd.state = UState::Available(r);
                    }
                } else if !silent {
                    self.upd.state = UState::UpToDate(Some(r));
                }
            }
            Msg::Checked(Ok(None), silent) => {
                if !silent {
                    self.upd.state = UState::UpToDate(None);
                }
            }
            Msg::Checked(Err(e), silent) => {
                if !silent {
                    self.upd.state = UState::Failed(e);
                }
            }
            Msg::Installed(res) => {
                let rel = match std::mem::take(&mut self.upd.state) {
                    UState::Downloading(r) => r,
                    _ => return,
                };
                self.upd.state = match res {
                    Ok(_) => UState::Ready(rel),
                    Err(e) => UState::Failed(e),
                };
            }
        }
    }

    pub(super) fn update_window(&mut self, ctx: &egui::Context) {
        self.poll_updates();
        if !self.upd.open {
            return;
        }
        let mut open = true;
        let mut check = false;
        let mut install: Option<Release> = None;
        let mut restart = false;
        egui::Window::new(RichText::new(tr("Updates", "Updates")).font(FontId::new(14.5, theme::semibold())))
            .id(Id::unique("updates"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.set_width(520.0);
                egui::Grid::new("upd_grid").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new(tr("Installed version", "Installierte Version")).color(theme::TEXT_DIM()));
                    ui.label(RichText::new(version::long()).monospace().size(12.0));
                    ui.end_row();
                    ui.label(RichText::new(tr("Releases", "Veröffentlichungen")).color(theme::TEXT_DIM()));
                    let url = format!("https://github.com/{}/releases", version::REPO);
                    if ui.link(&url).clicked() {
                        super::auth::open_url(&url);
                    }
                    ui.end_row();
                });
                ui.add_space(8.0);
                match &self.upd.state {
                    UState::Idle => {
                        ui.label(tr("Look for a newer version of YAGUI.", "Nach einer neueren Version von YAGUI suchen."));
                    }
                    UState::Checking => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr("Checking GitHub…", "Frage GitHub…"));
                        });
                    }
                    UState::UpToDate(r) => {
                        ui.horizontal(|ui| {
                            super::ui::icon(ui, crate::icons::Icon::Check, 15.0, theme::GREEN());
                            ui.label(match r {
                                Some(r) => trs(format!("YAGUI is up to date (newest release: {}).", r.tag), format!("YAGUI ist aktuell (neueste Version: {}).", r.tag)),
                                None => tr("No release has been published yet.", "Es wurde noch keine Version veröffentlicht.").to_string(),
                            });
                        });
                    }
                    UState::Failed(e) => {
                        ui.label(RichText::new(trs(format!("Could not check for updates: {e}"), format!("Konnte nicht nach Updates suchen: {e}"))).color(theme::RED()));
                        ui.label(RichText::new(tr("For a private repository, sign in to GitHub under File > Accounts.", "Bei einem privaten Repository unter Datei > Konten bei GitHub anmelden.")).size(12.0).color(theme::TEXT_FAINT()));
                    }
                    UState::Available(r) | UState::Downloading(r) | UState::Ready(r) => {
                        ui.label(RichText::new(trs(format!("Version {} is available", r.tag), format!("Version {} ist verfügbar", r.tag))).font(FontId::new(14.0, theme::semibold())));
                        if !r.published.is_empty() {
                            ui.label(RichText::new(trs(format!("published {}", r.published), format!("veröffentlicht am {}", r.published))).size(12.0).color(theme::TEXT_DIM()));
                        }
                        if !r.notes.trim().is_empty() {
                            ui.add_space(4.0);
                            egui::Frame::new().fill(theme::PANEL_DARK()).corner_radius(4).inner_margin(8).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                                    ui.add(egui::Label::new(RichText::new(r.notes.trim()).size(12.5)).wrap());
                                });
                            });
                        }
                        if r.asset_for_this_computer().is_none() {
                            ui.label(RichText::new(trs(format!("This release has no file for {}. Download it from the release page.", version::platform()), format!("Diese Version hat keine Datei für {}. Lade sie von der Release-Seite.", version::platform()))).size(12.0).color(theme::ORANGE()));
                        }
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.p.auto_update, tr("Check automatically once a day", "Einmal am Tag automatisch suchen"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let primary = |ui: &mut egui::Ui, l: &str, en: bool| ui.add_enabled(en, egui::Button::new(RichText::new(l).color(Color32::WHITE)).fill(theme::ACCENT_DARK()).min_size(Vec2::new(90.0, 26.0))).clicked();
                        match &self.upd.state {
                            UState::Available(r) => {
                                if primary(ui, tr("Download and install", "Herunterladen und installieren"), r.asset_for_this_computer().is_some()) {
                                    install = Some(r.clone());
                                }
                                if ui.button(tr("Release page", "Release-Seite")).clicked() {
                                    super::auth::open_url(&r.url);
                                }
                            }
                            UState::Downloading(_) => {
                                ui.spinner();
                                ui.label(tr("Downloading…", "Lade herunter…"));
                            }
                            UState::Ready(_) => {
                                if primary(ui, tr("Restart YAGUI", "YAGUI neu starten"), true) {
                                    restart = true;
                                }
                                ui.label(RichText::new(tr("Installed. Restart to use the new version.", "Installiert. Neu starten, um die neue Version zu nutzen.")).color(theme::GREEN()));
                            }
                            UState::Checking => {}
                            _ => {
                                if primary(ui, tr("Check now", "Jetzt suchen"), true) {
                                    check = true;
                                }
                            }
                        }
                    });
                });
            });
        self.upd.open = open;
        if check {
            self.check_updates(false, ctx);
        }
        if let Some(r) = install {
            if let Some(a) = r.asset_for_this_computer().cloned() {
                let (tx, rx) = channel();
                let c = ctx.clone();
                std::thread::spawn(move || {
                    let res = updater::download(&a).and_then(|f| updater::install(&f));
                    let _ = tx.send(Msg::Installed(res));
                    c.request_repaint();
                });
                self.upd.rx = Some(rx);
                self.upd.state = UState::Downloading(r);
                self.upd.notice = None;
            }
        }
        if restart {
            self.save();
            updater::restart();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    // ------------------------------------------------------------------ setup / uninstall

    pub(super) fn setup_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.setup.take() else { return };
        let mut keep = true;
        let mut run = false;
        let mut launch = false;
        let mut quit = false;
        let title = match d.mode {
            SetupMode::Install => tr("Install YAGUI", "YAGUI installieren"),
            SetupMode::Uninstall => tr("Uninstall YAGUI", "YAGUI deinstallieren"),
        };
        egui::Window::new(RichText::new(title).font(FontId::new(15.0, theme::semibold())))
            .id(Id::unique("setup"))
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(18)))
            .show(ctx, |ui| {
                ui.set_width(500.0);
                ui.horizontal(|ui| {
                    let tex = ui.ctx().load_texture("setup_icon", egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::appicon::icon_pixels(64)), egui::TextureOptions::LINEAR);
                    ui.image((tex.id(), Vec2::splat(48.0)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("YAGUI").font(FontId::new(18.0, theme::semibold())));
                        ui.label(RichText::new(format!("Yet Another Git User Interface · {}", version::VERSION)).color(theme::TEXT_DIM()));
                    });
                });
                ui.add_space(10.0);
                match (&d.mode, &d.result) {
                    (SetupMode::Install, None) => {
                        ui.label(tr("YAGUI is installed for you only; no administrator rights are needed.", "YAGUI wird nur für dich installiert; Administratorrechte sind nicht nötig."));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tr("Folder", "Ordner")).color(theme::TEXT_DIM()));
                            let mut s = d.opts.dir.display().to_string();
                            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(330.0)).changed() {
                                d.opts.dir = s.into();
                            }
                            if ui.button("…").clicked() {
                                if let Some(f) = rfd::FileDialog::new().pick_folder() {
                                    d.opts.dir = f.join("YAGUI");
                                }
                            }
                        });
                        if cfg!(windows) {
                            ui.checkbox(&mut d.opts.start_menu, tr("Start menu entry", "Eintrag im Startmenü"));
                            ui.checkbox(&mut d.opts.desktop, tr("Desktop shortcut", "Verknüpfung auf dem Desktop"));
                            ui.checkbox(&mut d.opts.context_menu, tr("\"Open in YAGUI\" on folders in Explorer", "„In YAGUI öffnen“ bei Ordnern im Explorer"));
                        } else {
                            ui.checkbox(&mut d.opts.start_menu, tr("Application menu entry and `yagui` command", "Eintrag im Anwendungsmenü und Befehl `yagui`"));
                        }
                        ui.label(RichText::new(tr("Uninstall any time under Settings > Apps or Help > Uninstall YAGUI.", "Deinstallieren jederzeit unter Einstellungen > Apps oder Hilfe > YAGUI deinstallieren.")).size(12.0).color(theme::TEXT_FAINT()));
                    }
                    (SetupMode::Uninstall, None) => {
                        ui.label(trs(format!("Remove YAGUI from {}?", installer::default_dir().display()), format!("YAGUI aus {} entfernen?", installer::default_dir().display())));
                        ui.label(RichText::new(tr("Shortcuts, Explorer entries and the Apps entry are removed too. Your repositories are not touched.", "Verknüpfungen, Explorer-Einträge und der Apps-Eintrag werden mit entfernt. Deine Repositories bleiben unberührt.")).size(12.0).color(theme::TEXT_DIM()));
                        ui.checkbox(&mut d.remove_settings, tr("Also delete my YAGUI settings", "Auch meine YAGUI-Einstellungen löschen"));
                    }
                    (_, Some(Ok(p))) => {
                        ui.horizontal(|ui| {
                            super::ui::icon(ui, crate::icons::Icon::Check, 16.0, theme::GREEN());
                            ui.label(match d.mode {
                                SetupMode::Install => trs(format!("Installed to {}", p.display()), format!("Installiert nach {}", p.display())),
                                SetupMode::Uninstall => tr("YAGUI has been removed.", "YAGUI wurde entfernt.").to_string(),
                            });
                        });
                    }
                    (_, Some(Err(e))) => {
                        ui.label(RichText::new(e).color(theme::RED()));
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let primary = |ui: &mut egui::Ui, l: &str| ui.add(egui::Button::new(RichText::new(l).color(Color32::WHITE)).fill(theme::ACCENT_DARK()).min_size(Vec2::new(100.0, 26.0))).clicked();
                    match (&d.mode, &d.result) {
                        (SetupMode::Install, None) => {
                            if primary(ui, tr("Install", "Installieren")) {
                                run = true;
                            }
                            if ui.button(tr("Cancel", "Abbrechen")).clicked() {
                                keep = false;
                            }
                        }
                        (SetupMode::Uninstall, None) => {
                            if primary(ui, tr("Uninstall", "Deinstallieren")) {
                                run = true;
                            }
                            if ui.button(tr("Cancel", "Abbrechen")).clicked() {
                                keep = false;
                                if *START_MODE.get().unwrap_or(&StartMode::Normal) == StartMode::Uninstall {
                                    quit = true;
                                }
                            }
                        }
                        (SetupMode::Install, Some(Ok(_))) => {
                            if primary(ui, tr("Start YAGUI", "YAGUI starten")) {
                                launch = true;
                            }
                            if ui.button(tr("Close", "Schließen")).clicked() {
                                keep = false;
                            }
                        }
                        (SetupMode::Uninstall, Some(Ok(_))) => {
                            if primary(ui, tr("Close", "Schließen")) {
                                quit = true;
                            }
                        }
                        (_, Some(Err(_))) => {
                            if ui.button(tr("Close", "Schließen")).clicked() {
                                keep = false;
                            }
                        }
                    }
                });
            });
        if run {
            d.result = Some(match d.mode {
                SetupMode::Install => installer::install(&d.opts),
                SetupMode::Uninstall => installer::uninstall(&installer::default_dir(), d.remove_settings, true).map(|_| installer::default_dir()),
            });
        }
        if launch {
            if let Some(Ok(p)) = &d.result {
                let _ = std::process::Command::new(p).spawn();
            }
            quit = true;
        }
        if quit {
            self.no_save = matches!(d.mode, SetupMode::Uninstall);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if keep {
            self.setup = Some(d);
        }
    }
}
