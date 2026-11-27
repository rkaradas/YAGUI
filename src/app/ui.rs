//! The main window: menu, toolbar, repository tabs, sidebar, history with details and the
//! local-changes view. Commands are not shown in the layout itself; they appear subtly in
//! tooltips, in the dialog footer and in the activity window.

use std::path::PathBuf;

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, RichText, Sense, Stroke, Vec2};

use super::diffmodel::Diff;
use super::tree::{self, Row};
use super::{diffview, Act, App, DetailTab, Repo, View, ViewOpt};
use crate::catalog::{self, CATS};
use crate::git::{self, FileStatus, JobState};
use crate::i18n::{Lang, c, keys, tr, trs};
use crate::icons::{self, Icon};
use crate::theme;

// ---------------------------------------------------------------------------------------------
// Small building blocks

pub(super) fn icon(ui: &mut egui::Ui, i: Icon, size: f32, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    icons::draw(ui.painter(), rect, i, color);
    resp
}

/// A flat icon button (no frame until hovered).
pub(super) fn icon_button(ui: &mut egui::Ui, i: Icon, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4, theme::dk(0x3a, 0x3a, 0x3d));
    }
    icons::draw(ui.painter(), rect.shrink(4.0), i, if resp.hovered() { theme::TEXT_STRONG() } else { theme::TEXT_DIM() });
    resp.on_hover_text(tip)
}

/// Tooltip with a plain description and, below it, the git command in small grey type.
pub(super) fn tip_with_cmd(resp: egui::Response, what: &str, cmd: &str) -> egui::Response {
    let what = what.to_string();
    let cmd = cmd.to_string();
    resp.on_hover_ui(move |ui| {
        ui.set_max_width(320.0);
        ui.label(&what);
        if !cmd.is_empty() {
            ui.add_space(2.0);
            ui.label(RichText::new(&cmd).monospace().size(11.0).color(theme::TEXT_DIM()));
        }
    })
}

fn tool_button(ui: &mut egui::Ui, i: Icon, label: &str, what: &str, cmd: &str, enabled: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(11.5), theme::TEXT());
    let w = (galley.size().x + 16.0).max(46.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 46.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter_at(rect);
    if enabled && resp.hovered() {
        p.rect_filled(rect.shrink2(Vec2::new(1.0, 3.0)), 5, theme::dk(0x30, 0x30, 0x33));
    }
    let col = if !enabled { theme::TEXT_FAINT() } else if resp.hovered() { theme::TEXT_STRONG() } else { theme::TEXT() };
    let ir = egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 15.0), Vec2::splat(18.0));
    icons::draw(&p, ir, i, col);
    p.text(rect.center_bottom() - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, label, FontId::proportional(11.5), if enabled { theme::TEXT_DIM() } else { theme::TEXT_FAINT() });
    tip_with_cmd(resp, what, cmd)
}

/// A toolbar button that opens a menu: like `tool_button`, with a small arrow next to the
/// icon so it is clear there is more behind it.
fn tool_button_menu(ui: &mut egui::Ui, i: Icon, label: &str, what: &str, cmd: &str, enabled: bool) -> egui::Response {
    let resp = tool_button(ui, i, label, what, cmd, enabled);
    let p = ui.painter_at(resp.rect);
    let c = resp.rect.center_top() + Vec2::new(15.0, 16.0);
    icons::draw(&p, egui::Rect::from_center_size(c, Vec2::splat(8.0)), Icon::ChevronDown, if enabled { theme::TEXT_DIM() } else { theme::TEXT_FAINT() });
    resp
}

/// A small text button with an arrow, for menus inside panels (e.g. "View options").
fn menu_text_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.0), theme::TEXT_DIM());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 26.0, 22.0), Sense::click());
    let p = ui.painter_at(rect);
    if resp.hovered() {
        p.rect_filled(rect, 4, theme::ROW_HOVER());
    }
    p.galley(egui::pos2(rect.left() + 7.0, rect.center().y - g.size().y / 2.0), g, theme::TEXT_DIM());
    icons::draw(&p, egui::Rect::from_center_size(egui::pos2(rect.right() - 10.0, rect.center().y), Vec2::splat(9.0)), Icon::ChevronDown, theme::TEXT_DIM());
    resp
}

fn separator_v(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(13.0, 30.0), Sense::hover());
    ui.painter().line_segment([rect.center_top(), rect.center_bottom()], Stroke::new(1.0, theme::LINE()));
}

pub(super) fn status_color(c: char) -> Color32 {
    match c {
        'A' | '?' => theme::GREEN(),
        'D' => theme::RED(),
        'R' | 'C' => theme::ACCENT(),
        'U' => theme::ORANGE(),
        _ => theme::YELLOW(),
    }
}

pub(super) fn status_badge(ui: &mut egui::Ui, c: char) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
    let col = status_color(c);
    let r = rect.shrink(1.5);
    ui.painter().rect_filled(r, 3, col.gamma_multiply(0.22));
    ui.painter().rect_stroke(r, 3, Stroke::new(1.0, col.gamma_multiply(0.85)), egui::StrokeKind::Inside);
    let letter = match c {
        '?' => 'A',
        x => x,
    };
    ui.painter().text(r.center(), Align2::CENTER_CENTER, letter, FontId::new(9.5, theme::semibold()), col);
}

pub(super) fn status_word(c: char) -> &'static str {
    match c {
        'M' => tr("Modified", "Geändert"),
        'A' => tr("Added", "Hinzugefügt"),
        'D' => tr("Deleted", "Gelöscht"),
        'R' => tr("Renamed", "Umbenannt"),
        'C' => tr("Copied", "Kopiert"),
        '?' => tr("New (untracked)", "Neu (nicht verfolgt)"),
        'U' => tr("Conflict", "Konflikt"),
        'T' => tr("Type changed", "Typ geändert"),
        _ => tr("Modified", "Geändert"),
    }
}

fn initials(name: &str) -> String {
    let mut s: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    if s.is_empty() {
        s = "?".into();
    }
    s.to_uppercase()
}

fn split_path(p: &str) -> (&str, &str) {
    match p.rfind('/') {
        Some(i) => (&p[i + 1..], &p[..i]),
        None => (p, ""),
    }
}

/// A full-width row in a list: optional icon, text, optional right-side text; highlight on
/// hover/selection. Returns the click response.
fn list_row(ui: &mut egui::Ui, ic: Option<Icon>, text: &str, strong: bool, right: &str, selected: bool, indent: f32) -> egui::Response {
    let h = 24.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect, 4, theme::ROW_SEL());
    } else if resp.hovered() {
        p.rect_filled(rect, 4, theme::ROW_HOVER());
    }
    let mut x = rect.left() + 8.0 + indent;
    if let Some(i) = ic {
        icons::draw(&p, egui::Rect::from_center_size(egui::pos2(x + 7.0, rect.center().y), Vec2::splat(14.0)), i, if selected { theme::TEXT_STRONG() } else { theme::TEXT_DIM() });
        x += 22.0;
    }
    let font = if strong { FontId::new(13.0, theme::semibold()) } else { FontId::proportional(13.0) };
    let right_w = if right.is_empty() { 0.0 } else { p.layout_no_wrap(right.to_string(), FontId::proportional(11.5), theme::TEXT_DIM()).size().x + 16.0 };
    let clip = egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(rect.right() - right_w, rect.bottom()));
    ui.painter_at(clip).text(egui::pos2(x, rect.center().y), Align2::LEFT_CENTER, text, font, if selected || strong { theme::TEXT_STRONG() } else { theme::TEXT() });
    if !right.is_empty() {
        p.text(egui::pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, right, FontId::proportional(11.5), theme::TEXT_DIM());
    }
    resp
}

/// A collapsible sidebar section header, open state kept in egui memory.
fn section(ui: &mut egui::Ui, id: &str, title: &str, count: usize, default_open: bool) -> bool {
    section_r(ui, id, title, count, default_open).0
}

fn section_r(ui: &mut egui::Ui, id: &str, title: &str, count: usize, default_open: bool) -> (bool, egui::Response) {
    let key = Id::unique(("sec", id));
    let open = ui.ctx().data_mut(|d| *d.get_temp_mut_or_insert_with(key, || default_open));
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    let p = ui.painter_at(rect);
    let ir = egui::Rect::from_center_size(egui::pos2(rect.left() + 10.0, rect.center().y), Vec2::splat(12.0));
    icons::draw(&p, ir, if open { Icon::ChevronDown } else { Icon::ChevronRight }, theme::TEXT_DIM());
    p.text(egui::pos2(rect.left() + 22.0, rect.center().y), Align2::LEFT_CENTER, title, FontId::new(13.0, theme::semibold()), if resp.hovered() { theme::TEXT_STRONG() } else { theme::TEXT() });
    if count > 0 {
        p.text(egui::pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(11.5), theme::TEXT_FAINT());
    }
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(key, !open));
    }
    (open, resp)
}

pub(super) fn pill(ui: &mut egui::Ui, text: &str, fill: Color32, fg: Color32) {
    egui::Frame::new().fill(fill).corner_radius(9).inner_margin(egui::Margin::symmetric(7, 1)).show(ui, |ui| {
        ui.label(RichText::new(text).size(11.0).color(fg));
    });
}

fn menu_item(ui: &mut egui::Ui, acts: &mut Vec<Act>, label: &str, act: Act) {
    if ui.button(label).clicked() {
        acts.push(act);
        ui.close();
    }
}

/// Opens a catalogue command's dialog from a menu; the label is the command's own title.
fn menu_cmd(ui: &mut egui::Ui, cmds: &[catalog::Cmd], acts: &mut Vec<Act>, id: &'static str, label: Option<&str>, fills: Vec<(&'static str, String)>) {
    let cmd = &cmds[catalog::find(cmds, id)];
    let text = label.map(|s| s.to_string()).unwrap_or_else(|| format!("{}…", c(cmd.title)));
    let resp = ui.button(text);
    let resp = tip_with_cmd(resp, c(cmd.summary), &format!("git {}", cmd.base.trim_start_matches('!')));
    if resp.clicked() {
        acts.push(Act::Open(id, fills));
        ui.close();
    }
}

// ---------------------------------------------------------------------------------------------
// Diffs (drawn by `diffview`)

/// Shown while a diff is being loaded in the background.
fn loading_hint(ui: &mut egui::Ui) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(tr("Loading…", "Lädt …")).color(theme::TEXT_FAINT()));
    });
}

/// The facts are produced in German by `diffmodel::parse`; translate them for English.
pub(super) fn fact_en(f: &str) -> String {
    let pairs = [
        ("Neue Datei (Modus ", "New file (mode "),
        ("Datei gelöscht (Modus ", "Deleted file (mode "),
        ("Dateimodus vorher ", "Old mode "),
        ("Dateimodus jetzt ", "New mode "),
        ("Ähnlichkeit ", "Similarity "),
        ("Umbenannt von ", "Renamed from "),
        ("Umbenannt nach ", "Renamed to "),
        ("Kopiert von ", "Copied from "),
        ("Kopiert nach ", "Copied to "),
        ("Binärdatei – Inhalt kann nicht zeilenweise gezeigt werden", "Binary file – no line-by-line view"),
    ];
    for (de, en) in pairs {
        if let Some(rest) = f.strip_prefix(de) {
            return format!("{en}{rest}");
        }
    }
    f.to_string()
}

/// A file's diff with its header (status, name, diff toolbar), for the Changes tab of a commit.
fn diff_panel(ui: &mut egui::Ui, acts: &mut Vec<Act>, bar: &diffview::Bar, code: char, path: &str, d: Option<&Diff>, id: Id) {
    let split_possible = !d.is_some_and(|d| d.combined) && ui.available_width() >= diffview::SPLIT_MIN_WIDTH;
    egui::Frame::new().fill(theme::PANEL()).inner_margin(egui::Margin::symmetric(12, 5)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            status_badge(ui, code);
            let (file, dir) = split_path(path);
            ui.label(RichText::new(file).font(FontId::new(13.0, theme::semibold())).color(theme::TEXT_STRONG()));
            if !dir.is_empty() {
                ui.label(RichText::new(dir).size(12.0).color(theme::TEXT_DIM()));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(o) = diffview::toolbar(ui, bar, split_possible) {
                    acts.push(Act::DiffOpt(o));
                }
            });
        });
    });
    let Some(d) = d else {
        loading_hint(ui);
        return;
    };
    egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 0 }).show(ui, |ui| {
        diffview::facts(ui, d);
    });
    if d.unified.is_empty() {
        diffview::empty_hint(ui, d, &bar.opts);
        return;
    }
    egui::ScrollArea::vertical().id_salt(id).auto_shrink([false; 2]).show(ui, |ui| {
        diffview::draw(ui, id, d, bar.look, None);
    });
}

// ---------------------------------------------------------------------------------------------

impl App {
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let has = !self.tabs.is_empty();
        egui::Panel::top("menu").frame(egui::Frame::new().fill(theme::TOOLBAR()).inner_margin(egui::Margin::symmetric(6, 2))).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                // File – repositories, tabs, sign-in and preferences.
                ui.menu_button(tr("File", "Datei"), |ui| {
                    menu_cmd(ui, &self.cmds, acts, "init", Some(tr("Init New Repository…", "Neues Repository anlegen…")), vec![]);
                    menu_cmd(ui, &self.cmds, acts, "clone", Some(tr("Clone…", "Klonen…")), vec![]);
                    ui.separator();
                    menu_item(ui, acts, &*keys(tr("New Tab   Ctrl+T", "Neuer Tab   Strg+T")), Act::Manager);
                    menu_item(ui, acts, &*keys(tr("Open Repository…   Ctrl+O", "Repository öffnen…   Strg+O")), Act::OpenRepoDialog);
                    menu_item(ui, acts, &*keys(tr("Quick Launch…   Ctrl+P", "Schnellstart…   Strg+P")), Act::QuickLaunch);
                    ui.menu_button(tr("Recent", "Zuletzt geöffnet"), |ui| {
                        if self.recent.is_empty() {
                            ui.label(RichText::new(tr("Nothing yet", "Noch nichts")).color(theme::TEXT_DIM()));
                        }
                        for r in self.recent.clone() {
                            if ui.button(&r).clicked() {
                                acts.push(Act::OpenRepo(PathBuf::from(&r)));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    ui.add_enabled_ui(has, |ui| menu_item(ui, acts, &*keys(tr("Close Tab   Ctrl+W", "Tab schließen   Strg+W")), Act::CloseTab(self.active)));
                    ui.add_enabled_ui(!self.closed_tabs.is_empty(), |ui| menu_item(ui, acts, &*keys(tr("Restore Closed Tab   Ctrl+Shift+T", "Geschlossenen Tab wiederherstellen   Strg+Umschalt+T")), Act::RestoreTab));
                    ui.separator();
                    menu_item(ui, acts, tr("Configure SSH Keys…", "SSH-Schlüssel einrichten…"), Act::Ssh);
                    menu_item(ui, acts, tr("Accounts…", "Konten…"), Act::Accounts);
                    ui.separator();
                    menu_item(ui, acts, tr("Check for Updates…", "Nach Updates suchen…"), Act::CheckUpdates);
                    menu_item(ui, acts, &*keys(tr("Preferences…   Ctrl+,", "Einstellungen…   Strg+,")), Act::Prefs);
                    ui.separator();
                    if ui.button(tr("Exit", "Beenden")).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                // View – what the history shows.
                ui.menu_button(tr("View", "Ansicht"), |ui| {
                    ui.add_enabled_ui(has, |ui| {
                        menu_item(ui, acts, &*keys(tr("Show Uncommitted Changes   Ctrl+1", "Lokale Änderungen zeigen   Strg+1")), Act::View(View::Changes));
                        menu_item(ui, acts, &*keys(tr("Show All Commits   Ctrl+2", "Alle Commits zeigen   Strg+2")), Act::View(View::Commits));
                        ui.separator();
                        let o = self.tabs.get(self.active).map(|r| r.log_opts.clone()).unwrap_or_default();
                        let check = |ui: &mut egui::Ui, on: bool, label: &str, help: &str, opt: ViewOpt, acts: &mut Vec<Act>| {
                            let mut v = on;
                            if ui.checkbox(&mut v, label).on_hover_text(help).clicked() {
                                acts.push(Act::ViewOpt(opt));
                                ui.close();
                            }
                        };
                        check(ui, self.hide_details, tr("Hide Commit Details", "Commit-Details ausblenden"), tr("More room for the commit list", "Mehr Platz für die Commit-Liste"), ViewOpt::Details, acts);
                        if ui.button(&*keys(tr("Show HEAD   Ctrl+H", "HEAD zeigen   Strg+H"))).on_hover_text(tr("Jump to the commit you are on", "Springt zum aktuellen Commit")).clicked() {
                            acts.push(Act::ViewOpt(ViewOpt::Head));
                            ui.close();
                        }
                        check(ui, o.hide_tags, tr("Hide Tags", "Tags ausblenden"), tr("No tag labels in the commit list", "Keine Tag-Beschriftungen in der Liste"), ViewOpt::Tags, acts);
                        check(ui, o.hide_stashes, tr("Hide Stashes in Commit List", "Stashes in der Commit-Liste ausblenden"), "--exclude=refs/stash", ViewOpt::Stashes, acts);
                        check(ui, o.reflog, tr("Show Lost Commits (Reflog)", "Verlorene Commits zeigen (Reflog)"), "--reflog", ViewOpt::Reflog, acts);
                        check(ui, o.first_parent, tr("Collapse All Merges (Show First Parent)", "Merges einklappen (nur erster Elternteil)"), "--first-parent", ViewOpt::FirstParent, acts);
                        check(ui, o.only.as_deref() == Some("HEAD"), tr("Filter by Active Branch", "Nur aktiven Branch zeigen"), tr("Only commits reachable from the current branch", "Nur Commits des aktuellen Branches"), ViewOpt::ActiveBranch, acts);
                    });
                    ui.separator();
                    menu_item(ui, acts, tr("Git Commands", "Git-Befehle"), Act::Commands(true));
                    menu_item(ui, acts, tr("Activity", "Aktivität"), Act::ToggleActivity);
                    menu_item(ui, acts, tr("Git Console (type commands)", "Git-Konsole (Befehle eintippen)"), Act::ToggleConsole);
                    menu_item(ui, acts, tr("Tab List Sidebar", "Tab-Liste an der Seite"), Act::ToggleTabList);
                    ui.separator();
                    ui.menu_button(tr("Language", "Sprache"), |ui| {
                        for (l, name) in [(Lang::En, "English"), (Lang::De, "Deutsch")] {
                            if ui.selectable_label(self.lang == l, name).clicked() {
                                acts.push(Act::Lang(l));
                                ui.close();
                            }
                        }
                    });
                });
                ui.menu_button(tr("Repository", "Repository"), |ui| {
                    ui.add_enabled_ui(has, |ui| {
                        menu_item(ui, acts, &*keys(tr("Refresh   F5", "Neu laden   F5")), Act::Refresh);
                        ui.separator();
                        menu_cmd(ui, &self.cmds, acts, "fetch", Some(&*keys(tr("Fetch…   Ctrl+Shift+L", "Fetch…   Strg+Umschalt+L"))), vec![]);
                        menu_cmd(ui, &self.cmds, acts, "pull", Some(tr("Pull…", "Pull…")), vec![]);
                        menu_cmd(ui, &self.cmds, acts, "push", Some(&*keys(tr("Push…   Ctrl+Shift+U", "Push…   Strg+Umschalt+U"))), vec![]);
                        ui.separator();
                        menu_cmd(ui, &self.cmds, acts, "stash_push", Some(&*keys(tr("Save Stash…   Ctrl+Shift+S", "Stash speichern…   Strg+Umschalt+S"))), vec![]);
                        menu_item(ui, acts, &*keys(tr("New Branch…   Ctrl+Shift+B", "Neuer Branch…   Strg+Umschalt+B")), Act::NewBranch(None));
                        menu_item(ui, acts, tr("New Tag…", "Neuer Tag…"), Act::NewTag(None));
                        menu_cmd(ui, &self.cmds, acts, "worktree_add", Some(tr("New Worktree…", "Neuer Worktree…")), vec![]);
                        ui.separator();
                        ui.menu_button("Git Flow", |ui| {
                            for id in ["flow_init", "flow_feature_start", "flow_feature_finish", "flow_feature_publish", "flow_release_start", "flow_release_finish", "flow_hotfix_start", "flow_hotfix_finish"] {
                                menu_cmd(ui, &self.cmds, acts, id, None, vec![]);
                            }
                        });
                        ui.menu_button("Git LFS", |ui| {
                            for id in ["lfs_install", "lfs_track", "lfs_untrack", "lfs_ls_files", "lfs_status", "lfs_fetch", "lfs_pull", "lfs_push", "lfs_locks", "lfs_lock", "lfs_unlock", "lfs_prune", "lfs_migrate"] {
                                menu_cmd(ui, &self.cmds, acts, id, None, vec![]);
                            }
                        });
                        menu_cmd(ui, &self.cmds, acts, "apply", Some(tr("Apply Patch…", "Patch anwenden…")), vec![]);
                        ui.menu_button("Bisect", |ui| {
                            for id in ["bisect_start", "bisect_good", "bisect_bad", "bisect_skip", "bisect_run", "bisect_log", "bisect_reset"] {
                                menu_cmd(ui, &self.cmds, acts, id, None, vec![]);
                            }
                        });
                        ui.separator();
                        if let Some(r) = self.tabs.get(self.active) {
                            let p = r.path.clone();
                            menu_item(ui, acts, &*keys(tr("Open In File Explorer   Ctrl+Alt+O", "Im Datei-Explorer öffnen   Strg+Alt+O")), Act::Explorer(p));
                        }
                        menu_item(ui, acts, &*keys(tr("Open In Console   Ctrl+Alt+T", "In Konsole öffnen   Strg+Alt+T")), Act::Terminal);
                        ui.separator();
                        menu_item(ui, acts, tr("Repository Statistics…", "Repository-Statistik…"), Act::Stats);
                        menu_item(ui, acts, tr("Repository Treemap…", "Repository-Treemap…"), Act::Treemap);
                        menu_item(ui, acts, tr("Performance Benchmark…", "Leistungstest…"), Act::Bench);
                        ui.separator();
                        menu_item(ui, acts, tr("Settings for This Repository…", "Einstellungen für dieses Repository…"), Act::RepoSettings);
                    });
                });
                // Every git command, grouped: the full set stays one click away.
                ui.menu_button(tr("Commands", "Befehle"), |ui| {
                    menu_item(ui, acts, &*keys(tr("Find a command…   Ctrl+Shift+P", "Befehl suchen…   Strg+Umschalt+P")), Act::QuickLaunch);
                    menu_item(ui, acts, tr("Browse all commands…", "Alle Befehle durchsuchen…"), Act::Commands(true));
                    if !self.custom_cmds.is_empty() {
                        ui.menu_button(tr("Custom Commands", "Eigene Befehle"), |ui| {
                            for (i, cc) in self.custom_cmds.iter().enumerate() {
                                if ui.button(&cc.name).on_hover_text(&cc.command).clicked() {
                                    acts.push(Act::Custom(i, String::new()));
                                    ui.close();
                                }
                            }
                        });
                    }
                    ui.separator();
                    for (cat, name, _) in CATS {
                        ui.menu_button(c(name), |ui| {
                            ui.set_max_height(520.0);
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                for cmd in self.cmds.iter().filter(|x| x.cat == *cat) {
                                    let resp = ui.button(c(cmd.title));
                                    let resp = tip_with_cmd(resp, c(cmd.summary), &format!("git {}", cmd.base.trim_start_matches('!')));
                                    if resp.clicked() {
                                        acts.push(Act::Open(cmd.id, vec![]));
                                        ui.close();
                                    }
                                }
                            });
                        });
                    }
                });
                ui.menu_button(tr("Window", "Fenster"), |ui| {
                    menu_item(ui, acts, &*keys(tr("Select Previous Tab   Ctrl+Shift+Tab", "Vorheriger Tab   Strg+Umschalt+Tab")), Act::TabStep(-1));
                    menu_item(ui, acts, &*keys(tr("Select Next Tab   Ctrl+Tab", "Nächster Tab   Strg+Tab")), Act::TabStep(1));
                    ui.separator();
                    menu_item(ui, acts, tr("Switch Theme", "Hell/Dunkel wechseln"), Act::Theme(!theme::is_light()));
                    menu_item(ui, acts, &*keys(tr("Zoom In   Ctrl++", "Vergrößern   Strg++")), Act::Zoom(0.1));
                    menu_item(ui, acts, &*keys(tr("Zoom Out   Ctrl+-", "Verkleinern   Strg+-")), Act::Zoom(-0.1));
                    menu_item(ui, acts, &*keys(tr("Actual Size   Ctrl+0", "Originalgröße   Strg+0")), Act::Zoom(0.0));
                });
                ui.menu_button(tr("Help", "Hilfe"), |ui| {
                    menu_item(ui, acts, tr("Keyboard Shortcuts", "Tastenkürzel"), Act::Shortcuts);
                    ui.separator();
                    for (label, url) in [
                        (tr("Git Documentation", "Git-Dokumentation"), "https://git-scm.com/docs"),
                        (tr("Pro Git Book (free)", "Pro-Git-Buch (kostenlos)"), "https://git-scm.com/book/de/v2"),
                        (tr("Git Cheat Sheet", "Git-Spickzettel"), "https://education.github.com/git-cheat-sheet-education.pdf"),
                    ] {
                        if ui.button(label).clicked() {
                            super::auth::open_url(url);
                            ui.close();
                        }
                    }
                    ui.separator();
                    menu_cmd(ui, &self.cmds, acts, "version", Some(tr("Git Version…", "Git-Version…")), vec![]);
                    ui.separator();
                    menu_item(ui, acts, tr("Check for Updates…", "Nach Updates suchen…"), Act::CheckUpdates);
                    if crate::installer::running_installed() {
                        menu_item(ui, acts, tr("Uninstall YAGUI…", "YAGUI deinstallieren…"), Act::Uninstall);
                    } else if !cfg!(target_os = "macos") {
                        menu_item(ui, acts, tr("Install YAGUI…", "YAGUI installieren…"), Act::Install);
                    }
                    ui.separator();
                    menu_item(ui, acts, tr("About YAGUI", "Über YAGUI"), Act::About);
                });
            });
        });
    }

    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let has = !self.tabs.is_empty();
        let (rname, head) = self.tabs.get(self.active).map(|r| (r.name.clone(), r.head.clone())).unwrap_or_default();
        let locals: Vec<String> = self.tabs.get(self.active).map(|r| r.local.iter().map(|b| b.name.clone()).collect()).unwrap_or_default();
        egui::Panel::top("toolbar").exact_size(52.0).frame(egui::Frame::new().fill(theme::TOOLBAR()).inner_margin(egui::Margin::symmetric(8, 3))).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(34.0, 40.0), Sense::click());
                    let p = ui.painter_at(rect);
                    if resp.hovered() || self.show_tablist {
                        p.rect_filled(rect.shrink2(Vec2::new(2.0, 6.0)), 4, theme::ROW_HOVER());
                    }
                    icons::draw(&p, egui::Rect::from_center_size(rect.center(), Vec2::splat(17.0)), Icon::Sidebar, if self.show_tablist { theme::ACCENT() } else { theme::TEXT() });
                    if resp.on_hover_text(tr("Show open repositories and workspaces; + opens the Repository Manager", "Offene Repositories und Workspaces zeigen; + öffnet den Repository Manager")).clicked() {
                        acts.push(Act::ToggleTabList);
                    }
                }
                if tool_button(ui, Icon::Search, tr("Quick Launch", "Schnellstart"), &*keys(tr("Find any Git command (Ctrl+Shift+P)", "Jeden Git-Befehl finden (Strg+Umschalt+P)")), "", true).clicked() {
                    acts.push(Act::QuickLaunch);
                }
                separator_v(ui);
                if tool_button(ui, Icon::Fetch, "Fetch", tr("Download new commits from the server", "Neue Commits vom Server laden"), "git fetch", has).clicked() {
                    acts.push(Act::Open("fetch", vec![]));
                }
                if tool_button(ui, Icon::Pull, "Pull", tr("Download and integrate new commits", "Neue Commits laden und einarbeiten"), "git pull", has).clicked() {
                    acts.push(Act::Open("pull", vec![]));
                }
                if tool_button(ui, Icon::Push, "Push", tr("Upload your commits to the server", "Deine Commits hochladen"), "git push", has).clicked() {
                    acts.push(Act::Open("push", vec![]));
                }
                separator_v(ui);
                if tool_button(ui, Icon::Stash, "Stash", tr("Put local changes aside", "Lokale Änderungen zwischenlagern"), "git stash push", has).clicked() {
                    acts.push(Act::Open("stash_push", vec![]));
                }
                let stashes: Vec<(String, String)> = self.tabs.get(self.active).map(|r| r.stashes.clone()).unwrap_or_default();
                let dd = dropdown_arrow(ui, has, tr("More stash actions", "Weitere Stash-Aktionen"));
                egui::Popup::menu(&dd).show(|ui| {
                    ui.set_min_width(260.0);
                    ui.label(RichText::new(tr("Recent Stashes", "Letzte Stashes")).size(11.5).color(theme::TEXT_FAINT()));
                    if stashes.is_empty() {
                        ui.label(RichText::new(tr("none", "keine")).color(theme::TEXT_DIM()));
                    }
                    for (name, msg) in stashes.iter().take(8) {
                        ui.menu_button(format!("{name}  {}", msg.chars().take(40).collect::<String>()), |ui| {
                            menu_cmd(ui, &self.cmds, acts, "stash_apply", Some(tr("Apply…", "Anwenden…")), vec![("stash", name.clone())]);
                            menu_cmd(ui, &self.cmds, acts, "stash_pop", Some(tr("Pop (apply and delete)…", "Zurückholen (anwenden und löschen)…")), vec![("stash", name.clone())]);
                            menu_cmd(ui, &self.cmds, acts, "stash_drop", Some(tr("Delete…", "Löschen…")), vec![("stash", name.clone())]);
                        });
                    }
                    ui.separator();
                    let b = ui.button(tr("Save Snapshot…", "Schnappschuss speichern…"));
                    if tip_with_cmd(b, tr("Store the current changes as a stash and keep working on them", "Speichert die Änderungen als Stash und lässt sie im Arbeitsordner"), "git stash store $(git stash create)").clicked() {
                        acts.push(Act::Snapshot);
                        ui.close();
                    }
                });
                if tool_button(ui, Icon::Pop, "Pop", tr("Bring back the latest stash", "Letzten Stash zurückholen"), "git stash pop", has).clicked() {
                    acts.push(Act::Open("stash_pop", vec![]));
                }

                // Centre: repository and current branch – click to switch branch.
                let right_w = 7.0 * 62.0;
                let centre_w = 250.0;
                let pad = ((ui.available_width() - right_w - centre_w - 120.0) / 2.0).max(8.0);
                ui.add_space(pad);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(centre_w, 38.0), if has { Sense::click() } else { Sense::hover() });
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 5, if resp.hovered() && has { theme::dk(0x33, 0x33, 0x36) } else { theme::dk(0x2a, 0x2a, 0x2c) });
                if has {
                    p.text(rect.center_top() + Vec2::new(0.0, 11.0), Align2::CENTER_CENTER, &rname, FontId::new(13.0, theme::semibold()), theme::TEXT_STRONG());
                    let g = p.layout_no_wrap(head.clone(), FontId::proportional(11.5), theme::TEXT_DIM());
                    let bx = rect.center().x - g.size().x / 2.0 + 7.0;
                    icons::draw(&p, egui::Rect::from_center_size(egui::pos2(bx - 10.0, rect.bottom() - 11.0), Vec2::splat(11.0)), Icon::Branch, theme::TEXT_DIM());
                    p.galley(egui::pos2(bx, rect.bottom() - 11.0 - g.size().y / 2.0), g, theme::TEXT_DIM());
                    icons::draw(&p, egui::Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(10.0)), Icon::ChevronDown, theme::TEXT_DIM());
                } else {
                    p.text(rect.center(), Align2::CENTER_CENTER, tr("No repository", "Kein Repository"), FontId::proportional(12.5), theme::TEXT_DIM());
                }
                let resp = resp.on_hover_text(tr("Current branch – click to switch", "Aktueller Branch – klicken zum Wechseln"));
                if has {
                    egui::Popup::menu(&resp).show(|ui| {
                        ui.set_min_width(220.0);
                        ui.label(RichText::new(tr("Switch to", "Wechseln zu")).size(11.5).color(theme::TEXT_DIM()));
                        for b in &locals {
                            let resp = ui.selectable_label(*b == head, b);
                            if resp.clicked() && *b != head {
                                acts.push(Act::Open("switch", vec![("branch", b.clone())]));
                                ui.close();
                            }
                        }
                    });
                }
                ui.add_space(6.0);
                if tool_button(ui, Icon::Branch, "Branch", tr("Create a branch and switch to it", "Branch anlegen und wechseln"), "git switch -c", has).clicked() {
                    acts.push(Act::NewBranch(None));
                }
                let dd = dropdown_arrow(ui, has, tr("More branch actions", "Weitere Branch-Aktionen"));
                egui::Popup::menu(&dd).show(|ui| {
                    menu_item(ui, acts, tr("New Branch…", "Neuer Branch…"), Act::NewBranch(None));
                    menu_cmd(ui, &self.cmds, acts, "worktree_add", Some(tr("New Worktree…", "Neuer Worktree…")), vec![]);
                    menu_item(ui, acts, tr("New Tag…", "Neuer Tag…"), Act::NewTag(None));
                });
                if tool_button(ui, Icon::Merge, "Merge", tr("Merge another branch into this one", "Anderen Branch zusammenführen"), "git merge", has).clicked() {
                    acts.push(Act::Open("merge", vec![]));
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Workspaces
                    let ws_name = self.workspaces.get(self.ws).map(|w| w.name.clone()).unwrap_or_default();
                    let b = tool_button_menu(ui, Icon::Cube, &short_label(&ws_name), tr("Workspace: each one keeps its own open repositories", "Workspace: jeder hat seine eigenen offenen Repositories"), "", true);
                    egui::Popup::menu(&b).show(|ui| self.workspace_menu(ui, acts));
                    // Appearance
                    let b = tool_button_menu(ui, Icon::Theme, tr("Appearance", "Darstellung"), tr("Theme and commit list layout", "Farbschema und Anordnung der Commit-Liste"), "", true);
                    egui::Popup::menu(&b).show(|ui| {
                        ui.label(RichText::new(tr("Theme", "Farbschema")).size(11.5).color(theme::TEXT_FAINT()));
                        if ui.radio(theme::is_light(), tr("Light", "Hell")).clicked() {
                            acts.push(Act::Theme(true));
                            ui.close();
                        }
                        if ui.radio(!theme::is_light(), tr("Dark", "Dunkel")).clicked() {
                            acts.push(Act::Theme(false));
                            ui.close();
                        }
                        ui.separator();
                        ui.label(RichText::new(tr("Commit List Layout", "Anordnung der Commit-Liste")).size(11.5).color(theme::TEXT_FAINT()));
                        if ui.radio(!self.layout_vertical, tr("Horizontal (details below)", "Horizontal (Details unten)")).clicked() {
                            acts.push(Act::Layout(false));
                            ui.close();
                        }
                        if ui.radio(self.layout_vertical, tr("Vertical (details on the right)", "Vertikal (Details rechts)")).clicked() {
                            acts.push(Act::Layout(true));
                            ui.close();
                        }
                    });
                    separator_v(ui);
                    if tool_button(ui, Icon::Activity, tr("Activity", "Aktivität"), tr("Commands run so far and their output", "Ausgeführte Befehle und ihre Ausgabe"), "", true).clicked() {
                        acts.push(Act::ToggleActivity);
                    }
                    let cmds_on = self.commands_view;
                    let b = tool_button(ui, Icon::Cube, tr("Commands", "Befehle"), tr("All Git commands, explained and runnable", "Alle Git-Befehle, erklärt und ausführbar"), "", true);
                    if cmds_on {
                        ui.painter().line_segment([b.rect.left_bottom() + Vec2::new(14.0, -2.0), b.rect.right_bottom() + Vec2::new(-14.0, -2.0)], egui::Stroke::new(2.0, theme::ACCENT()));
                    }
                    if b.clicked() {
                        acts.push(Act::Commands(!cmds_on));
                    }
                    separator_v(ui);
                    if tool_button(ui, Icon::Terminal, tr("Console", "Konsole"), &*keys(tr("Open a terminal in this repository (Ctrl+Alt+T)", "Terminal in diesem Repository öffnen (Strg+Alt+T)")), "", has).clicked() {
                        acts.push(Act::Terminal);
                    }
                    let b = tool_button_menu(ui, Icon::Folder, tr("Open in", "Öffnen in"), tr("Open the folder in another program", "Ordner in einem anderen Programm öffnen"), "", has);
                    if has {
                        egui::Popup::menu(&b).show(|ui| self.open_in_menu(ui));
                    }
                });
            });
        });
    }

    pub(super) fn tab_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        egui::Panel::top("tabs").exact_size(26.0).frame(egui::Frame::new().fill(theme::TABS())).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let n = self.tabs.len().max(1);
                let w = ((ui.available_width() - 28.0) / n as f32).clamp(110.0, 320.0);
                let mut menus: Vec<(egui::Response, usize)> = Vec::new();
                for (i, t) in self.tabs.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
                    let active = i == self.active && !self.manager_active;
                    let p = ui.painter_at(rect);
                    let bg = if active { theme::PANEL() } else if resp.hovered() { theme::dk(0x22, 0x22, 0x22) } else { theme::TABS() };
                    p.rect_filled(rect, 0, bg);
                    p.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, theme::dk(0x2c, 0x2c, 0x2c)));
                    p.text(rect.center(), Align2::CENTER_CENTER, &t.name, FontId::proportional(12.5), if active { theme::TEXT_STRONG() } else { theme::TEXT_DIM() });
                    let close = egui::Rect::from_center_size(rect.right_center() - Vec2::new(14.0, 0.0), Vec2::splat(14.0));
                    let hover_close = resp.hover_pos().map(|p| close.contains(p)).unwrap_or(false);
                    if resp.hovered() {
                        if hover_close {
                            p.rect_filled(close.expand(2.0), 3, theme::dk(0x44, 0x44, 0x48));
                        }
                        icons::draw(&p, close.shrink(2.0), Icon::Close, theme::TEXT_DIM());
                    }
                    if resp.clicked() {
                        acts.push(if hover_close { Act::CloseTab(i) } else { Act::SwitchTab(i) });
                    }
                    if resp.middle_clicked() {
                        acts.push(Act::CloseTab(i));
                    }
                    let resp = resp.on_hover_text(t.path.display().to_string());
                    menus.push((resp, i));
                }
                if self.manager_tab {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w.min(220.0), 26.0), Sense::click());
                    let p = ui.painter_at(rect);
                    let active = self.manager_active;
                    p.rect_filled(rect, 0, if active { theme::PANEL() } else if resp.hovered() { theme::ROW_HOVER() } else { theme::TABS() });
                    p.text(rect.center(), Align2::CENTER_CENTER, tr("Repository Manager", "Repository Manager"), FontId::proportional(12.5), if active { theme::TEXT_STRONG() } else { theme::TEXT_DIM() });
                    let close = egui::Rect::from_center_size(rect.right_center() - Vec2::new(14.0, 0.0), Vec2::splat(14.0));
                    let hover_close = resp.hover_pos().map(|p| close.contains(p)).unwrap_or(false);
                    if resp.hovered() {
                        icons::draw(&p, close.shrink(2.0), Icon::Close, theme::TEXT_DIM());
                    }
                    if resp.clicked() {
                        if hover_close {
                            acts.push(Act::CloseManager);
                        } else {
                            acts.push(Act::Manager);
                        }
                    }
                }
                ui.add_space(4.0);
                if icon_button(ui, Icon::Plus, &*keys(tr("New tab: Repository Manager (Ctrl+T)", "Neuer Tab: Repository Manager (Strg+T)"))).clicked() {
                    acts.push(Act::Manager);
                }
                for (resp, i) in menus {
                    resp.context_menu(|ui| self.tab_menu(ui, acts, i));
                }
            });
        });
    }

    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let now = ui.ctx().input(|i| i.time);
        egui::Panel::bottom("statusbar").exact_size(24.0).frame(egui::Frame::new().fill(theme::TOOLBAR()).inner_margin(egui::Margin::symmetric(10, 0))).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let running: Vec<&git::Job> = self.jobs.iter().filter(|j| j.state() == JobState::Running).collect();
                if let Some(j) = running.last() {
                    ui.spinner();
                    ui.label(RichText::new(&j.title).size(12.0));
                    ui.label(RichText::new(j.display.lines().next().unwrap_or("")).monospace().size(11.0).color(theme::TEXT_FAINT()));
                } else if let Some(j) = self.jobs.iter().rev().find(|j| !j.automatic) {
                    let ok = matches!(j.state(), JobState::Done(0) | JobState::Started);
                    icon(ui, if ok { Icon::Check } else { Icon::Warning }, 13.0, if ok { theme::GREEN() } else { theme::RED() });
                    ui.label(RichText::new(&j.title).size(12.0).color(theme::TEXT_DIM()));
                    ui.label(RichText::new(j.display.lines().next().unwrap_or("")).monospace().size(11.0).color(theme::TEXT_FAINT()));
                }
                if let Some((msg, t)) = &self.toast {
                    if now - t < 6.0 {
                        ui.label(RichText::new(msg).size(12.0).color(theme::YELLOW()));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(r) = &self.upd.notice {
                        let t = trs(format!("Update {} available", r.tag), format!("Update {} verfügbar", r.tag));
                        if ui.add(egui::Button::new(RichText::new(t).size(11.5).color(Color32::WHITE)).fill(theme::ACCENT_DARK())).clicked() {
                            acts.push(Act::CheckUpdates);
                        }
                        ui.add_space(8.0);
                    }
                    for (l, name) in [(Lang::De, "DE"), (Lang::En, "EN")] {
                        let sel = self.lang == l;
                        let r = ui.add(egui::Button::new(RichText::new(name).size(11.0).color(if sel { theme::TEXT_STRONG() } else { theme::TEXT_FAINT() })).frame(false));
                        if r.on_hover_text(tr("Interface language", "Sprache der Oberfläche")).clicked() && !sel {
                            acts.push(Act::Lang(l));
                        }
                    }
                    ui.add_space(8.0);
                    let label = format!("{} ({})", tr("Activity", "Aktivität"), self.jobs.iter().filter(|j| !j.automatic).count());
                    if ui.add(egui::Button::new(RichText::new(label).size(11.5).color(theme::TEXT_DIM())).frame(false)).clicked() {
                        acts.push(Act::ToggleActivity);
                    }
                    // The repository is being read in the background; said only when it takes a
                    // while, so quick reloads don't make the bar flicker.
                    if let Some(d) = self.tabs.get(self.active).and_then(|r| r.loading_for()) {
                        let wait = std::time::Duration::from_millis(400);
                        if d >= wait {
                            ui.add_space(8.0);
                            ui.label(RichText::new(tr("Reading repository…", "Lese Repository …")).size(11.5).color(theme::TEXT_FAINT()));
                        } else {
                            ui.ctx().request_repaint_after(wait - d);
                        }
                    }
                });
            });
        });
    }

    /// A red bar when the last command failed, with its first error line.
    pub(super) fn notice_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let Some(id) = self.notice else { return };
        let Some(j) = self.jobs.iter().find(|j| j.id == id) else {
            self.notice = None;
            return;
        };
        let out = j.output();
        let first = out.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
        let title = j.title.clone();
        let mut close = false;
        egui::Frame::new().fill(theme::dk(0x4a, 0x25, 0x27)).inner_margin(egui::Margin::symmetric(12, 7)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icon(ui, Icon::Warning, 15.0, Color32::from_rgb(0xf2, 0x9a, 0x9a));
                ui.label(RichText::new(trs(format!("{title} failed"), format!("{title} fehlgeschlagen"))).color(theme::TEXT_STRONG()));
                ui.label(RichText::new(&first).color(Color32::from_rgb(0xe8, 0xb8, 0xb8)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, tr("Dismiss", "Ausblenden")).clicked() {
                        close = true;
                    }
                    if ui.button(tr("Details", "Details")).clicked() {
                        acts.push(Act::ToggleActivity);
                        close = true;
                    }
                });
            });
        });
        if close {
            self.notice = None;
        }
    }

    pub(super) fn state_banner(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let r = &self.tabs[self.active];
        let bar = |ui: &mut egui::Ui, fill: Color32, add: &mut dyn FnMut(&mut egui::Ui)| {
            egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(12, 7)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| add(ui));
            });
        };
        if let Some(e) = &r.error {
            let e = e.clone();
            bar(ui, theme::dk(0x4a, 0x25, 0x27), &mut |ui| {
                ui.label(RichText::new(&e).color(theme::TEXT_STRONG()));
            });
        }
        if let Some(lock) = r.locked {
            bar(ui, theme::dk(0x3a, 0x30, 0x22), &mut |ui| {
                icon(ui, Icon::Lock, 14.0, theme::YELLOW());
                ui.label(trs(
                    format!("Another Git process is working on this repository (.git/{lock}). The view updates by itself when it is done."),
                    format!("Ein anderer Git-Prozess arbeitet gerade am Repository (.git/{lock}). Die Anzeige aktualisiert sich danach von selbst."),
                ));
            });
        }
        let Some(st) = r.state else { return };
        let (text, ids): (&str, Vec<&'static str>) = match st {
            "merge" => (tr("Merge in progress. Resolve the conflicts, stage the files, then continue.", "Ein Merge läuft. Konflikte lösen, Dateien vormerken, dann abschließen."), vec!["merge_continue", "merge_abort"]),
            "rebase" => (tr("Rebase paused. Resolve conflicts or finish editing, then continue.", "Rebase angehalten. Konflikte lösen oder Bearbeitung abschließen, dann fortsetzen."), vec!["rebase_continue", "rebase_skip", "rebase_abort"]),
            "cherry-pick" => (tr("Cherry-pick in progress.", "Ein Cherry-Pick läuft."), vec!["cherry_pick_continue", "cherry_pick_skip", "cherry_pick_abort"]),
            "revert" => (tr("Revert in progress.", "Ein Revert läuft."), vec!["revert_continue", "revert_abort"]),
            "am" => (tr("Applying patches.", "Patches werden angewendet."), vec!["am_continue", "am_skip", "am_abort"]),
            "bisect" => (tr("Bisect in progress: test this revision and mark it.", "Bisect läuft: diesen Stand testen und markieren."), vec!["bisect_good", "bisect_bad", "bisect_reset"]),
            "squash" => (tr("A squash merge prepared changes. Review them and commit.", "Ein Squash-Merge hat Änderungen vorbereitet. Prüfen und committen."), vec![]),
            _ => return,
        };
        let cmds = &self.cmds;
        let squash = st == "squash";
        bar(ui, theme::dk(0x3d, 0x33, 0x1c), &mut |ui| {
            icon(ui, Icon::Warning, 14.0, theme::YELLOW());
            ui.label(RichText::new(text).color(theme::TEXT_STRONG()));
            ui.add_space(8.0);
            for id in &ids {
                let cmd = &cmds[catalog::find(cmds, id)];
                let resp = ui.button(c(cmd.title));
                if tip_with_cmd(resp, c(cmd.summary), &format!("git {}", cmd.base)).clicked() {
                    acts.push(Act::Open(cmd.id, vec![]));
                }
            }
            if squash && ui.button(tr("Go to commit", "Zum Commit")).clicked() {
                acts.push(Act::View(View::Changes));
            }
        });
    }

    // -----------------------------------------------------------------------------------------
    // Sidebar

    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let cmds_on = self.commands_view;
        let pinned: Vec<String> = {
            let key = format!("{}|", self.tabs[self.active].path.display());
            self.pinned.iter().filter_map(|p| p.strip_prefix(&key).map(String::from)).collect()
        };
        let sort = self.branch_sort;
        let branch_customs = self.customs_for(2);
        let mut new_sort: Option<u8> = None;
        let r = &mut self.tabs[self.active];
        egui::Panel::left("sidebar").resizable(true).default_size(260.0).min_size(200.0).frame(egui::Frame::new().fill(theme::SIDEBAR()).inner_margin(egui::Margin { left: 6, right: 6, top: 8, bottom: 6 })).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(&r.name).font(FontId::new(14.0, theme::semibold())).color(theme::TEXT_STRONG()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let g = icon_button(ui, Icon::Settings, tr("Repository settings", "Repository-Einstellungen"));
                    let path = r.path.clone();
                    egui::Popup::menu(&g).show(|ui| {
                        if ui.button(tr("Rename Repository…", "Repository umbenennen…")).clicked() {
                            acts.push(Act::RenameRepo(path.clone()));
                            ui.close();
                        }
                        if ui.button(tr("Copy Repository Path", "Repository-Pfad kopieren")).clicked() {
                            acts.push(Act::Copy(super::auth::native(&path)));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(tr("Settings for This Repository…", "Einstellungen für dieses Repository…")).clicked() {
                            acts.push(Act::RepoSettings);
                            ui.close();
                        }
                    });
                    if icon_button(ui, Icon::Refresh, tr("Refresh (F5)", "Neu laden (F5)")).clicked() {
                        acts.push(Act::Refresh);
                    }
                });
            });
            ui.add_space(6.0);
            let n = r.status.len();
            let changes_label = tr("Local Changes", "Lokale Änderungen");
            if list_row(ui, Some(Icon::Changes), changes_label, false, &if n > 0 { n.to_string() } else { String::new() }, r.view == View::Changes && !cmds_on, 0.0).clicked() {
                acts.push(Act::View(View::Changes));
            }
            if list_row(ui, Some(Icon::History), tr("All Commits", "Alle Commits"), false, "", r.view == View::Commits && !cmds_on, 0.0).clicked() {
                acts.push(Act::View(View::Commits));
            }
            if list_row(ui, Some(Icon::Cube), tr("Git Commands", "Git-Befehle"), false, "", cmds_on, 0.0)
                .on_hover_text(tr("Every Git command, explained, with options and a preview", "Jeder Git-Befehl mit Erklärung, Optionen und Vorschau"))
                .clicked()
            {
                acts.push(Act::Commands(true));
            }
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(&mut r.filter).hint_text(tr("Filter", "Filter")).desired_width(f32::INFINITY));
            ui.add_space(6.0);
            let filter = r.filter.to_lowercase();
            let matches = |s: &str| filter.is_empty() || s.to_lowercase().contains(&filter);
            let head = r.head.clone();

            egui::ScrollArea::vertical().id_salt("side_scroll").auto_shrink([false; 2]).show(ui, |ui| {
                // Branches: pinned first, then a folder tree ("feature/x" lives in "feature").
                let (open_b, hdr) = section_r(ui, "branches", tr("Branches", "Branches"), r.local.len(), true);
                hdr.context_menu(|ui| {
                    menu_item(ui, acts, tr("New Branch…", "Neuer Branch…"), Act::NewBranch(None));
                    menu_item(ui, acts, tr("Select Stale Local Branches…", "Veraltete lokale Branches auswählen…"), Act::StaleBranches);
                    ui.separator();
                    ui.label(RichText::new(tr("Sort Branches:", "Branches sortieren:")).size(11.5).color(theme::TEXT_FAINT()));
                    for (i, l) in [tr("Alphabetically", "Alphabetisch"), tr("Alphabetically backward", "Alphabetisch rückwärts"), tr("Recently used", "Zuletzt verwendet")].iter().enumerate() {
                        if ui.radio(sort == i as u8, *l).clicked() {
                            new_sort = Some(i as u8);
                            ui.close();
                        }
                    }
                });
                if open_b {
                    // Runs every frame: keep it linear in the number of branches (lookups by
                    // map, sort keys computed once), whatever the repository's size.
                    let mut names: Vec<String> = r.local.iter().map(|b| b.name.clone()).filter(|n| matches(n)).collect();
                    match sort {
                        1 => {
                            names.sort_by_cached_key(|n| n.to_lowercase());
                            names.reverse();
                        }
                        2 => {
                            let recency: std::collections::HashMap<&str, usize> = r.branch_recency.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
                            names.sort_by_key(|n| recency.get(n.as_str()).copied().unwrap_or(usize::MAX));
                        }
                        _ => names.sort_by_cached_key(|n| n.to_lowercase()),
                    }
                    let pins: Vec<String> = names.iter().filter(|n| pinned.contains(n)).cloned().collect();
                    let rest: Vec<String> = names.iter().filter(|n| !pinned.contains(n)).cloned().collect();
                    let by_name: std::collections::HashMap<&str, &git::RefInfo> = r.local.iter().map(|b| (b.name.as_str(), b)).collect();
                    let ctx = ui.ctx().clone();
                    let draw_branch = |ui: &mut egui::Ui, name: &str, label: &str, indent: f32, acts: &mut Vec<Act>| {
                        let Some(b) = by_name.get(name).copied() else { return };
                        let (a, be, gone) = Repo::ahead_behind(&b.track);
                        let mut right = String::new();
                        if a > 0 {
                            right.push_str(&format!("↑{a} "));
                        }
                        if be > 0 {
                            right.push_str(&format!("↓{be}"));
                        }
                        if gone {
                            right = tr("gone", "entfernt").into();
                        }
                        let is_pinned = pinned.iter().any(|p| p == name);
                        let filtered = r.log_opts.only.as_deref() == Some(name);
                        let hidden = r.log_opts.hidden.iter().any(|h| h == name);
                        let resp = list_row(ui, Some(if b.is_head { Icon::Check } else { Icon::Branch }), label, b.is_head, right.trim(), filtered, indent);
                        // Hover buttons: pin, show only this branch, hide it.
                        let mut hit_button = false;
                        if resp.hovered() || is_pinned || filtered || hidden {
                            let p = ui.painter_at(resp.rect);
                            let rr = resp.rect;
                            let right_x = rr.right() - 8.0 - if right.trim().is_empty() { 0.0 } else { 42.0 };
                            for (k, (ic, on, act)) in [(Icon::Eye, hidden, Act::HideBranch(name.to_string())), (Icon::Filter, filtered, Act::FilterBranch(name.to_string())), (Icon::Pin, is_pinned, Act::PinBranch(name.to_string()))].into_iter().enumerate() {
                                if !(resp.hovered() || on) {
                                    continue;
                                }
                                let c = egui::pos2(right_x - 8.0 - k as f32 * 18.0, rr.center().y);
                                let br = egui::Rect::from_center_size(c, Vec2::splat(16.0));
                                let hov = resp.hover_pos().map(|hp| br.contains(hp)).unwrap_or(false);
                                if hov {
                                    p.rect_filled(br, 3, theme::ROW_SEL());
                                }
                                icons::draw(&p, br.shrink(2.0), ic, if on { theme::ACCENT() } else { theme::TEXT_DIM() });
                                if hov && resp.clicked() {
                                    acts.push(act);
                                    hit_button = true;
                                }
                            }
                        }
                        // Tooltip and menu data are built only when shown, not in every frame.
                        let resp = resp.on_hover_ui(|ui| {
                            let up = if b.upstream.is_empty() { String::new() } else { format!(" → {}", b.upstream) };
                            ui.label(trs(
                                format!("{}{up}\nDouble-click to switch · right-click for actions", b.name),
                                format!("{}{up}\nDoppelklick zum Wechseln · Rechtsklick für Aktionen", b.name),
                            ));
                        });
                        if hit_button {
                        } else if resp.double_clicked() && !b.is_head {
                            acts.push(Act::Open("switch", vec![("branch", b.name.clone())]));
                        } else if resp.clicked() {
                            acts.push(Act::JumpToRef(format!("refs/heads/{}", b.name)));
                        }
                        let cmds = &self.cmds;
                        resp.context_menu(|ui| {
                            let extra = BranchExtra {
                                pr: r.remotes.first().and_then(|(rn, url)| super::manager::web_url(url).map(|w| (rn.clone(), pr_url(&w, &b.name)))),
                                remote_branches: r.remote_branches.iter().map(|b| b.name.clone()).collect(),
                                customs: branch_customs.clone(),
                                pinned: is_pinned,
                                filtered,
                                hidden,
                            };
                            branch_menu(ui, cmds, acts, &b.name, &b.upstream, &head, false, &extra)
                        });
                    };
                    for n in &pins {
                        draw_branch(ui, n, n, 8.0, acts);
                    }
                    let refs: Vec<&str> = rest.iter().map(|s| s.as_str()).collect();
                    if sort == 0 || sort == 1 {
                        for row in super::tree::rows(&ctx, "side-branches", &refs, true) {
                            match row {
                                super::tree::Row::Dir { path, label, depth, count, open } => {
                                    if super::tree::dir_row(ui, &label, depth, count, open, None).0.clicked() {
                                        super::tree::toggle(&ctx, "side-branches", &path, true);
                                    }
                                }
                                super::tree::Row::File { idx, name, depth } => {
                                    draw_branch(ui, refs[idx], &name, 8.0 + depth as f32 * super::tree::INDENT, acts);
                                }
                            }
                        }
                    } else {
                        for n in &rest {
                            draw_branch(ui, n, n, 8.0, acts);
                        }
                    }
                }
                ui.add_space(2.0);
                if section(ui, "remotes", tr("Remotes", "Remotes"), r.remotes.len(), true) {
                    for (rn, url) in &r.remotes {
                        let key = Id::unique(("remote-open", rn));
                        let open = ui.ctx().data_mut(|d| *d.get_temp_mut_or_insert_with(key, || false));
                        let n_br = r.remote_branches.iter().filter(|b| b.name.starts_with(&format!("{rn}/")) && !b.name.ends_with("/HEAD")).count();
                        let resp = list_row(ui, Some(Icon::Cloud), rn, false, &n_br.to_string(), false, 20.0).on_hover_text(url);
                        // Arrow like the section headers: the remote opens to show its branches.
                        icons::draw(&ui.painter_at(resp.rect), egui::Rect::from_center_size(egui::pos2(resp.rect.left() + 16.0, resp.rect.center().y), Vec2::splat(10.0)), if open { Icon::ChevronDown } else { Icon::ChevronRight }, theme::TEXT_DIM());
                        if resp.clicked() {
                            ui.ctx().data_mut(|d| d.insert_temp(key, !open));
                        }
                        let rn2 = rn.clone();
                        let url2 = url.clone();
                        let cmds = &self.cmds;
                        resp.context_menu(|ui| {
                            menu_cmd(ui, cmds, acts, "fetch", Some(&format!("{} {rn2}", tr("Fetch from", "Fetch von"))), vec![("remote", rn2.clone())]);
                            menu_cmd(ui, cmds, acts, "remote_set_url", None, vec![("remote", rn2.clone())]);
                            if let Some(other) = super::auth::switch_remote_url(&url2) {
                                let label = if other.starts_with("git@") { tr("Switch to SSH address…", "Auf SSH-Adresse umstellen…") } else { tr("Switch to HTTPS address…", "Auf HTTPS-Adresse umstellen…") };
                                menu_cmd(ui, cmds, acts, "remote_set_url", Some(label), vec![("remote", rn2.clone()), ("url", other)]);
                            }
                            menu_cmd(ui, cmds, acts, "remote_rename", None, vec![("remote", rn2.clone())]);
                            menu_cmd(ui, cmds, acts, "remote_prune", None, vec![("remote", rn2.clone())]);
                            if let Some(w) = super::manager::web_url(&url2) {
                                let host = if w.contains("github") { "GitHub" } else if w.contains("gitlab") { "GitLab" } else if w.contains("bitbucket") { "Bitbucket" } else { tr("the web", "im Web") };
                                if ui.button(trs(format!("View on {host}"), format!("Auf {host} ansehen"))).clicked() {
                                    super::auth::open_url(&w);
                                    ui.close();
                                }
                            }
                            menu_cmd(ui, cmds, acts, "remote_add", Some(tr("Add New Remote…", "Neues Remote hinzufügen…")), vec![]);
                            menu_item(ui, acts, tr("Copy Remote Address", "Remote-Adresse kopieren"), Act::Copy(url2.clone()));
                            ui.separator();
                            menu_cmd(ui, cmds, acts, "remote_remove", None, vec![("remote", rn2.clone())]);
                        });
                        if open {
                            // Remote branches as a folder tree too ("feature/x" lives in "feature").
                            let rbs: Vec<&git::RefInfo> = r.remote_branches.iter().filter(|b| b.name.starts_with(&format!("{rn}/")) && matches(&b.name)).collect();
                            let shorts: Vec<String> = rbs.iter().map(|b| b.name.trim_start_matches(&format!("{rn}/")).to_string()).collect();
                            let refs: Vec<&str> = shorts.iter().map(|s| s.as_str()).collect();
                            let ctx2 = ui.ctx().clone();
                            let salt = format!("remote-tree-{rn}");
                            let mut visible: Vec<(usize, f32)> = Vec::new();
                            for row in super::tree::rows(&ctx2, &salt, &refs, true) {
                                match row {
                                    super::tree::Row::Dir { path, label, depth, count, open } => {
                                        if super::tree::dir_row(ui, &label, depth + 2, count, open, None).0.clicked() {
                                            super::tree::toggle(&ctx2, &salt, &path, true);
                                        }
                                    }
                                    super::tree::Row::File { idx, depth, .. } => visible.push((idx, 30.0 + depth as f32 * super::tree::INDENT)),
                                }
                                // draw the branches of this row right away so the order stays
                                for (idx, indent) in visible.drain(..) {
                                let b = rbs[idx];
                                let short = shorts[idx].rsplit('/').next().unwrap_or(&shorts[idx]).to_string();
                                let resp = list_row(ui, Some(Icon::Branch), &short, false, "", false, indent).on_hover_text(&b.name);
                                if resp.double_clicked() {
                                    acts.push(Act::Open("checkout_track", vec![("branch", b.name.clone())]));
                                } else if resp.clicked() {
                                    acts.push(Act::JumpToRef(format!("refs/remotes/{}", b.name)));
                                }
                                let name = &b.name;
                                let cmds = &self.cmds;
                                resp.context_menu(|ui| {
                                    let extra = BranchExtra {
                                        pr: r.remotes.iter().find(|(rn, _)| name.starts_with(&format!("{rn}/"))).and_then(|(rn, url)| super::manager::web_url(url).map(|w| (rn.clone(), pr_url(&w, name.split_once('/').map(|x| x.1).unwrap_or(name))))),
                                        remote_branches: vec![],
                                        customs: branch_customs.clone(),
                                        pinned: false,
                                        filtered: r.log_opts.only.as_deref() == Some(name.as_str()),
                                        hidden: false,
                                    };
                                    branch_menu(ui, cmds, acts, name, "", &head, true, &extra)
                                });
                                }
                            }
                        }
                    }
                }
                ui.add_space(2.0);
                let (open_t, hdr) = section_r(ui, "tags", tr("Tags", "Tags"), r.tags.len(), false);
                hdr.context_menu(|ui| {
                    menu_item(ui, acts, tr("New Tag…", "Neuer Tag…"), Act::NewTag(None));
                    menu_cmd(ui, &self.cmds, acts, "push_tags", Some(tr("Push All Tags…", "Alle Tags pushen…")), vec![]);
                });
                if open_t {
                    for t in r.tags.iter().rev().filter(|t| matches(&t.name)) {
                        let resp = list_row(ui, Some(Icon::Tag), &t.name, false, "", false, 8.0);
                        if resp.clicked() {
                            acts.push(Act::JumpToRef(format!("refs/tags/{}", t.name)));
                        }
                        let name = t.name.clone();
                        let cmds = &self.cmds;
                        resp.context_menu(|ui| {
                            menu_cmd(ui, cmds, acts, "checkout", Some(tr("Checkout…", "Auschecken…")), vec![("branch", name.clone())]);
                            menu_cmd(ui, cmds, acts, "push_tag", None, vec![("tag", name.clone())]);
                            ui.separator();
                            menu_cmd(ui, cmds, acts, "tag_delete", None, vec![("tag", name.clone())]);
                            menu_cmd(ui, cmds, acts, "push_tag_delete", None, vec![("tag", name.clone())]);
                            ui.separator();
                            menu_item(ui, acts, tr("Copy name", "Namen kopieren"), Act::Copy(name.clone()));
                        });
                    }
                }
                ui.add_space(2.0);
                if section(ui, "stashes", tr("Stashes", "Stashes"), r.stashes.len(), false) {
                    for (name, msg) in r.stashes.iter().filter(|s| matches(&s.1)) {
                        let sel = r.stash_sel.as_deref() == Some(name.as_str());
                        let resp = list_row(ui, Some(Icon::Stash), msg, false, "", sel, 8.0).on_hover_text(trs(format!("{name}\nClick to see its changes · double-click to apply"), format!("{name}\nKlicken zeigt die Änderungen · Doppelklick wendet ihn an")));
                        let name = name.clone();
                        let cmds = &self.cmds;
                        if resp.double_clicked() {
                            acts.push(Act::Open("stash_apply", vec![("stash", name.clone())]));
                        } else if resp.clicked() {
                            acts.push(Act::SelectStash(name.clone()));
                        }
                        resp.context_menu(|ui| {
                            menu_cmd(ui, cmds, acts, "stash_apply", None, vec![("stash", name.clone())]);
                            menu_cmd(ui, cmds, acts, "stash_pop", None, vec![("stash", name.clone())]);
                            menu_cmd(ui, cmds, acts, "stash_branch", None, vec![("stash", name.clone())]);
                            menu_item(ui, acts, tr("Rename…", "Umbenennen…"), Act::RenameStash(name.clone()));
                            menu_item(ui, acts, tr("Save as Patch…", "Als Patch speichern…"), Act::StashPatch(name.clone()));
                            ui.separator();
                            menu_cmd(ui, cmds, acts, "stash_drop", None, vec![("stash", name.clone())]);
                        });
                    }
                }
                ui.add_space(2.0);
                let (open_s, hdr) = section_r(ui, "submodules", tr("Submodules", "Submodule"), r.submodules.len(), false);
                hdr.context_menu(|ui| {
                    menu_cmd(ui, &self.cmds, acts, "submodule_add", Some(tr("Add New Submodule…", "Neues Submodul hinzufügen…")), vec![]);
                    menu_cmd(ui, &self.cmds, acts, "submodule_update", Some(tr("Update Submodules…", "Submodule aktualisieren…")), vec![]);
                });
                if open_s {
                    for s in &r.submodules {
                        let full = r.path.join(s);
                        let resp = list_row(ui, Some(Icon::Cube), s, false, "", false, 8.0);
                        if resp.double_clicked() {
                            acts.push(Act::OpenRepo(full.clone()));
                        }
                        let s2 = s.clone();
                        let cmds = &self.cmds;
                        resp.context_menu(|ui| {
                            menu_item(ui, acts, tr("Open", "Öffnen"), Act::OpenRepo(full.clone()));
                            menu_cmd(ui, cmds, acts, "submodule_update", None, vec![("paths", s2.clone())]);
                            menu_cmd(ui, cmds, acts, "submodule_deinit", None, vec![("paths", s2.clone())]);
                        });
                    }
                }
                if r.worktrees.len() > 1 {
                    ui.add_space(2.0);
                    if section(ui, "worktrees", tr("Worktrees", "Worktrees"), r.worktrees.len() - 1, false) {
                        for (p, b) in r.worktrees.iter().skip(1) {
                            let resp = list_row(ui, Some(Icon::Worktree), b, false, "", false, 8.0).on_hover_text(p);
                            let pb = PathBuf::from(p);
                            let p2 = p.clone();
                            if resp.double_clicked() {
                                acts.push(Act::OpenRepo(pb.clone()));
                            }
                            let cmds = &self.cmds;
                            resp.context_menu(|ui| {
                                menu_item(ui, acts, tr("Open", "Öffnen"), Act::OpenRepo(pb.clone()));
                                menu_cmd(ui, cmds, acts, "worktree_remove", None, vec![("path", p2.clone())]);
                            });
                        }
                    }
                }
            });
        });
        if let Some(s) = new_sort {
            self.branch_sort = s;
            self.save();
        }
    }

    // -----------------------------------------------------------------------------------------
    // History

    pub(super) fn commits_view(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let (hide_details, vertical) = (self.hide_details, self.layout_vertical);
        let commit_customs = self.customs_for(1);
        let compact_labels = self.p.compact_labels;
        let rules = if self.p.issue_links { self.repo_settings().issue_rules } else { vec![] };
        let bar = self.diff_bar();
        let cmds = &self.cmds;
        let r = &mut self.tabs[self.active];
        if !hide_details {
            let frame = egui::Frame::new().fill(theme::PANEL()).inner_margin(0);
            if vertical {
                egui::Panel::right("detail_v").resizable(true).default_size(560.0).min_size(260.0).frame(frame).show(ui, |ui| {
                    detail_pane(ui, r, acts, &rules, &bar);
                });
            } else {
                egui::Panel::bottom("detail").resizable(true).default_size(340.0).min_size(140.0).frame(frame).show(ui, |ui| {
                    detail_pane(ui, r, acts, &rules, &bar);
                });
            }
        }
        // "Filtered by" bar above the list.
        if r.log_opts.only.is_some() || !r.log_opts.hidden.is_empty() || r.log_opts.first_parent {
            egui::Frame::new().fill(theme::PANEL_DARK()).inner_margin(egui::Margin::symmetric(12, 4)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    icon(ui, Icon::Filter, 13.0, theme::ACCENT());
                    let mut parts = Vec::new();
                    if let Some(o) = &r.log_opts.only {
                        parts.push(trs(format!("only '{o}'"), format!("nur '{o}'")));
                    }
                    if !r.log_opts.hidden.is_empty() {
                        parts.push(trs(format!("hidden: {}", r.log_opts.hidden.join(", ")), format!("ausgeblendet: {}", r.log_opts.hidden.join(", "))));
                    }
                    if r.log_opts.first_parent {
                        parts.push(tr("first parent only", "nur erster Elternteil").to_string());
                    }
                    ui.label(RichText::new(trs(format!("Filtered by: {}", parts.join(" · ")), format!("Gefiltert: {}", parts.join(" · ")))).size(12.0));
                    if ui.small_button(tr("Clear filter", "Filter aufheben")).clicked() {
                        r.log_opts.only = None;
                        r.log_opts.hidden.clear();
                        r.log_opts.first_parent = false;
                        r.reload(git::RELOAD_REFS);
                    }
                });
            });
        }

        egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 4 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                icon(ui, Icon::Search, 14.0, theme::TEXT_DIM());
                let modes = [tr("Message, author, SHA", "Beschreibung, Autor, SHA"), tr("Path", "Pfad"), tr("Diff content", "Inhalt der Änderungen")];
                let mode_before = r.search_mode;
                egui::ComboBox::from_id_salt("search_mode").width(150.0).selected_text(modes[r.search_mode]).show_ui(ui, |ui| {
                    for (i, m) in modes.iter().enumerate() {
                        ui.selectable_value(&mut r.search_mode, i, *m);
                    }
                });
                let hint = match r.search_mode {
                    1 => tr("File or folder, e.g. src/app", "Datei oder Ordner, z. B. src/app"),
                    2 => tr("Text added or removed, then Enter", "Hinzugefügter oder entfernter Text, dann Enter"),
                    _ => tr("Search message, author or SHA", "Beschreibung, Autor oder SHA suchen"),
                };
                let te = ui.add(egui::TextEdit::singleline(&mut r.commit_search).hint_text(hint).desired_width(280.0).frame(egui::Frame::NONE));
                // Path and diff-content searches ask git (git log -- <path> / git log -G <text>),
                // in the background; while typing a path only after a short pause.
                let now = ui.input(|i| i.time);
                if r.search_mode > 0 && (mode_before != r.search_mode || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) {
                    r.search_due = Some(now);
                } else if r.search_mode == 1 && te.changed() {
                    r.search_due = Some(now + 0.35);
                }
                if r.search_mode == 0 {
                    r.search_hits = None;
                    r.search_due = None;
                }
                if let Some(due) = r.search_due {
                    if now < due {
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(due - now));
                    } else {
                        r.search_due = None;
                        let q = r.commit_search.trim().to_string();
                        if q.is_empty() {
                            r.search_hits = None;
                            r.stop_search();
                        } else {
                            let mut args: Vec<String> = vec!["log".into(), "--format=%H".into(), "-n3000".into()];
                            args.extend(git::log_tip_args(&r.log_opts));
                            if r.search_mode == 1 {
                                args.push("--".into());
                                args.push(format!("*{q}*"));
                            } else {
                                args.insert(1, format!("-G{}", regex_escape(&q)));
                            }
                            r.start_search(args);
                        }
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let before = (r.log_opts.hide_stashes, r.log_opts.reflog, r.log_opts.first_parent, r.log_opts.only.clone());
                    let vb = menu_text_button(ui, tr("View options", "Anzeige"));
                    egui::Popup::menu(&vb).show(|ui| {
                        ui.checkbox(&mut r.log_opts.hide_stashes, tr("Hide stashes", "Stashes ausblenden"));
                        ui.checkbox(&mut r.log_opts.reflog, tr("Show reflog commits", "Reflog-Commits zeigen"));
                        ui.checkbox(&mut r.log_opts.hide_tags, tr("Hide tags", "Tags ausblenden"));
                        ui.checkbox(&mut r.log_opts.first_parent, tr("Collapse merges (first parent)", "Merges einklappen (erster Elternteil)"));
                        let mut active_only = r.log_opts.only.as_deref() == Some("HEAD");
                        if ui.checkbox(&mut active_only, tr("Only the active branch", "Nur aktiver Branch")).changed() {
                            r.log_opts.only = if active_only { Some("HEAD".into()) } else { None };
                        }
                        ui.separator();
                        let tips = git::log_tip_args(&r.log_opts).join(" ");
                        ui.label(RichText::new(format!("git log --date-order {tips}")).monospace().size(10.5).color(theme::TEXT_FAINT()));
                    });
                    if before != (r.log_opts.hide_stashes, r.log_opts.reflog, r.log_opts.first_parent, r.log_opts.only.clone()) {
                        r.reload(git::RELOAD_REFS);
                    }
                    ui.label(RichText::new(trs(format!("{} commits", r.commits.len()), format!("{} Commits", r.commits.len()))).size(12.0).color(theme::TEXT_FAINT()));
                });
            });
        });
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(rect, 0, theme::LINE());

        let q = r.commit_search.to_lowercase();
        let visible: Vec<usize> = if let Some(hits) = &r.search_hits {
            r.commits.iter().enumerate().filter(|(_, c)| hits.contains(&c.sha)).map(|(i, _)| i).collect()
        } else if q.is_empty() || r.search_mode > 0 {
            (0..r.commits.len()).collect()
        } else {
            r.commits.iter().enumerate().filter(|(_, c)| contains_ci(&c.subject, &q) || contains_ci(&c.author, &q) || c.sha.starts_with(&q)).map(|(i, _)| i).collect()
        };
        let row_h = 24.0;
        let mut scroll = egui::ScrollArea::vertical().id_salt("commits").auto_shrink([false; 2]);
        if let Some(i) = r.scroll_to.take() {
            if let Some(pos) = visible.iter().position(|v| *v == i) {
                scroll = scroll.vertical_scroll_offset((pos as f32 * row_h - 120.0).max(0.0));
            }
        }
        let graph_on = q.is_empty() || (r.search_mode > 0 && r.search_hits.is_none());
        let hide_tags = r.log_opts.hide_tags;
        let compact = compact_labels;
        let lane_w = 14.0;
        let graph_w = if graph_on { r.lanes as f32 * lane_w + 14.0 } else { 8.0 };
        let head = r.head.clone();
        scroll.show_rows(ui, row_h, visible.len(), |ui, range| {
            for vi in range {
                let i = visible[vi];
                let c = &r.commits[i];
                let w = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, row_h), Sense::click());
                let p = ui.painter_at(rect);
                let sel = r.selected == Some(i);
                if sel {
                    p.rect_filled(rect, 0, theme::ROW_SEL());
                } else if resp.hovered() {
                    p.rect_filled(rect, 0, theme::ROW_HOVER());
                }
                if graph_on {
                    if let Some(row) = r.rows.get(i) {
                        let x = |l: usize| rect.left() + 12.0 + l as f32 * lane_w;
                        let (top, mid, bot) = (rect.top(), rect.center().y, rect.bottom());
                        for &l in &row.through {
                            p.line_segment([egui::pos2(x(l), top), egui::pos2(x(l), bot)], Stroke::new(2.0, theme::lane_color(l)));
                        }
                        for &l in &row.incoming {
                            p.line_segment([egui::pos2(x(l), top), egui::pos2(x(row.lane), mid)], Stroke::new(2.0, theme::lane_color(l)));
                        }
                        for &l in &row.outgoing {
                            p.line_segment([egui::pos2(x(row.lane), mid), egui::pos2(x(l), bot)], Stroke::new(2.0, theme::lane_color(l)));
                        }
                        let col = theme::lane_color(row.lane);
                        p.circle_filled(egui::pos2(x(row.lane), mid), 4.5, col);
                        if c.parents.len() <= 1 {
                            p.circle_filled(egui::pos2(x(row.lane), mid), 2.0, if sel { theme::ROW_SEL() } else { theme::PANEL() });
                        }
                    }
                }
                let date_w = 118.0;
                let sha_w = 68.0;
                let author_w = 170.0;
                let right = rect.right() - 10.0;
                let subj_right = right - date_w - sha_w - author_w - 14.0;
                let mut x = rect.left() + graph_w + 4.0;
                for rf in &c.refs {
                    if hide_tags && rf.starts_with("tag: ") {
                        continue;
                    }
                    // Compact labels: a server branch on the same commit as its local branch
                    // is not shown separately.
                    if compact && !rf.starts_with("tag: ") && !rf.starts_with("HEAD") && !r.local.iter().any(|b| &b.name == rf) {
                        if let Some((_, short)) = rf.split_once('/') {
                            if c.refs.iter().any(|x| x == short || x.strip_prefix("HEAD -> ") == Some(short)) {
                                continue;
                            }
                        }
                    }
                    let (label, ic, fill, stroke, fg) = if let Some(b) = rf.strip_prefix("HEAD -> ") {
                        (b.to_string(), Some(Icon::Check), theme::dk(0x3a, 0x3a, 0x3d), theme::ORANGE(), theme::TEXT_STRONG())
                    } else if let Some(t) = rf.strip_prefix("tag: ") {
                        (t.to_string(), Some(Icon::Tag), theme::dk(0x3a, 0x34, 0x22), Color32::from_rgb(0x6a, 0x5a, 0x2a), theme::YELLOW())
                    } else if rf.starts_with("stash@") {
                        (rf.clone(), Some(Icon::Stash), theme::dk(0x33, 0x30, 0x3d), theme::dk(0x55, 0x4c, 0x6a), theme::TEXT())
                    } else if rf == "HEAD" {
                        ("HEAD".to_string(), None, theme::dk(0x3a, 0x3a, 0x3d), theme::ORANGE(), theme::TEXT_STRONG())
                    } else if r.local.iter().any(|b| &b.name == rf) {
                        (rf.clone(), None, theme::dk(0x2b, 0x3d, 0x55), Color32::from_rgb(0x3e, 0x5a, 0x80), theme::TEXT_STRONG())
                    } else {
                        (rf.clone(), Some(Icon::Cloud), theme::dk(0x33, 0x33, 0x35), theme::dk(0x48, 0x48, 0x4c), theme::TEXT_DIM())
                    };
                    let galley = p.layout_no_wrap(label, FontId::proportional(11.5), fg);
                    let iw = if ic.is_some() { 14.0 } else { 0.0 };
                    let bw = galley.size().x + 12.0 + iw;
                    if x + bw > subj_right - 60.0 {
                        break;
                    }
                    let br = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 4.0), Vec2::new(bw, row_h - 8.0));
                    p.rect_filled(br, 4, fill);
                    p.rect_stroke(br, 4, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
                    if let Some(ic) = ic {
                        icons::draw(&p, egui::Rect::from_center_size(egui::pos2(br.left() + 11.0, br.center().y), Vec2::splat(10.0)), ic, fg);
                    }
                    p.galley(egui::pos2(br.left() + 6.0 + iw, br.center().y - galley.size().y / 2.0), galley, fg);
                    x += bw + 5.0;
                }
                let subj_rect = egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(subj_right, rect.bottom()));
                ui.painter_at(subj_rect).text(egui::pos2(x + 2.0, rect.center().y), Align2::LEFT_CENTER, &c.subject, FontId::proportional(13.0), theme::TEXT());
                let ax = right - date_w - sha_w - author_w;
                let av = egui::Rect::from_center_size(egui::pos2(ax + 8.0, rect.center().y), Vec2::splat(16.0));
                p.rect_filled(av, 3, theme::avatar_color(&c.author));
                p.text(av.center(), Align2::CENTER_CENTER, initials(&c.author), FontId::new(8.5, theme::semibold()), Color32::WHITE);
                ui.painter_at(egui::Rect::from_min_max(egui::pos2(ax + 20.0, rect.top()), egui::pos2(ax + author_w - 6.0, rect.bottom())))
                    .text(egui::pos2(ax + 22.0, rect.center().y), Align2::LEFT_CENTER, &c.author, FontId::proportional(12.5), theme::TEXT());
                p.text(egui::pos2(right - date_w - sha_w, rect.center().y), Align2::LEFT_CENTER, &c.short, FontId::monospace(12.0), theme::TEXT_DIM());
                p.text(egui::pos2(right - date_w, rect.center().y), Align2::LEFT_CENTER, &c.date, FontId::proportional(12.5), theme::TEXT_DIM());
                if resp.clicked() {
                    acts.push(Act::SelectCommit(i));
                }
                let sha = c.sha.clone();
                let short = c.short.clone();
                let subject = c.subject.clone();
                let parents = c.parents.len();
                let head = head.clone();
                resp.context_menu(|ui| commit_menu(ui, cmds, acts, &sha, &short, &subject, parents, &head, &commit_customs));
            }
        });
    }

    // -----------------------------------------------------------------------------------------
    // Local changes

    pub(super) fn changes_view(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>, ctx: &egui::Context) {
        let mut hunk: Option<(String, bool, bool)> = None;
        let mut do_commit = false;
        let mut flat = self.flat_files;
        let (low, high, guide) = (self.p.subject_low, self.p.subject_high, self.p.page_guide);
        let bar = self.diff_bar();
        {
            let cmds = &self.cmds;
            let r = &mut self.tabs[self.active];
            let unstaged: Vec<FileStatus> = r.status.iter().filter(|f| f.unstaged()).cloned().collect();
            let staged: Vec<FileStatus> = r.status.iter().filter(|f| f.staged()).cloned().collect();
            let has_head = !r.commits.is_empty();
            let root = r.path.clone();

            egui::Panel::left("changes_files").resizable(true).default_size(380.0).min_size(280.0).frame(egui::Frame::new().fill(theme::PANEL()).inner_margin(0)).show(ui, |ui| {
                // Commit composer at the bottom
                egui::Panel::bottom("commit_box").resizable(false).frame(egui::Frame::new().fill(theme::PANEL_DARK()).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
                    let subj = ui.add(egui::TextEdit::singleline(&mut r.commit_msg).hint_text(tr("Commit subject", "Commit-Titel")).desired_width(f32::INFINITY));
                    // Length indicator (Preferences > Commit): fine up to the low limit, warn up to the high one.
                    let len = r.commit_msg.chars().count();
                    if len > 0 {
                        let col = if len <= low { theme::GREEN() } else if len <= high { theme::ORANGE() } else { theme::RED() };
                        ui.painter().text(subj.rect.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, len.to_string(), FontId::proportional(11.0), col);
                    }
                    ui.add_space(2.0);
                    let desc = ui.add(egui::TextEdit::multiline(&mut r.commit_desc).hint_text(tr("Description (optional)", "Beschreibung (optional)")).desired_rows(3).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
                    // Page guide at the configured column.
                    let cw = ui.fonts_mut(|f| f.glyph_width(&egui::TextStyle::Monospace.resolve(ui.style()), 'm'));
                    let gx = desc.rect.left() + 4.0 + cw * guide as f32;
                    if gx < desc.rect.right() - 4.0 {
                        ui.painter().line_segment([egui::pos2(gx, desc.rect.top() + 3.0), egui::pos2(gx, desc.rect.bottom() - 3.0)], Stroke::new(1.0, theme::LINE()));
                    }
                    let ctrl_enter = (subj.has_focus() || desc.has_focus()) && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut r.amend, tr("Amend last commit", "Letzten Commit ändern")).on_hover_text(tr(
                            "Replace the last commit instead of adding a new one. Don't use it on commits you already pushed.",
                            "Ersetzt den letzten Commit statt einen neuen anzulegen. Nicht für bereits gepushte Commits verwenden.",
                        ));
                    });
                    ui.add_space(4.0);
                    let can = (!r.commit_msg.trim().is_empty() || r.amend) && (!staged.is_empty() || r.amend);
                    let label = if r.amend {
                        tr("Amend Commit", "Commit ändern").to_string()
                    } else if staged.is_empty() {
                        tr("Commit", "Commit").to_string()
                    } else {
                        trs(format!("Commit {} file{}", staged.len(), if staged.len() == 1 { "" } else { "s" }), format!("{} Datei{} committen", staged.len(), if staged.len() == 1 { "" } else { "en" }))
                    };
                    let btn = egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(if can { theme::ACCENT_DARK() } else { theme::dk(0x36, 0x36, 0x39) }).min_size(Vec2::new(ui.available_width(), 28.0));
                    let resp = ui.add_enabled(can, btn).on_disabled_hover_text(tr("Stage files and enter a subject first", "Erst Dateien vormerken und einen Titel eingeben"));
                    if resp.clicked() || (ctrl_enter && can) {
                        do_commit = true;
                    }
                    // Subtle: what will run
                    let preview = commit_preview(&r.commit_msg, &r.commit_desc, r.amend);
                    ui.add_space(3.0);
                    ui.label(RichText::new(preview).monospace().size(10.5).color(theme::TEXT_FAINT()));
                });

                // Tree / list switch.
                egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 8, top: 6, bottom: 2 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Show as", "Anzeigen als")).size(11.5).color(theme::TEXT_FAINT()));
                        if ui.selectable_label(!flat, RichText::new(tr("Tree", "Baum")).size(11.5)).on_hover_text(tr("Files grouped by folder; act on a whole folder", "Dateien nach Ordnern; Aktionen für ganze Ordner")).clicked() {
                            flat = false;
                        }
                        if ui.selectable_label(flat, RichText::new(tr("List", "Liste")).size(11.5)).on_hover_text(tr("All files in one list", "Alle Dateien in einer Liste")).clicked() {
                            flat = true;
                        }
                        if !flat {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let all: Vec<&str> = r.status.iter().map(|f| f.path.as_str()).collect();
                                if ui.add(egui::Button::new(RichText::new(tr("Collapse all", "Alle zuklappen")).size(11.0).color(theme::TEXT_DIM())).frame(false)).clicked() {
                                    tree::set_all(ui.ctx(), "unstaged", &all, false);
                                    tree::set_all(ui.ctx(), "staged", &all, false);
                                }
                                if ui.add(egui::Button::new(RichText::new(tr("Expand all", "Alle aufklappen")).size(11.0).color(theme::TEXT_DIM())).frame(false)).clicked() {
                                    tree::set_all(ui.ctx(), "unstaged", &all, true);
                                    tree::set_all(ui.ctx(), "staged", &all, true);
                                }
                            });
                        }
                    });
                });
                egui::ScrollArea::vertical().id_salt("files").auto_shrink([false; 2]).show(ui, |ui| {
                    ui.add_space(4.0);
                    file_section_header(ui, tr("Unstaged", "Nicht vorgemerkt"), unstaged.len(), if unstaged.is_empty() { None } else { Some((tr("Stage All", "Alle vormerken"), "git add -A")) }, || {
                        acts.push(Act::Quick { title: tr("Stage all", "Alles vormerken").into(), args: vec!["add".into(), "-A".into()], explain: String::new() });
                    });
                    if unstaged.is_empty() {
                        empty_hint(ui, tr("No changes", "Keine Änderungen"));
                    }
                    file_list(ui, cmds, acts, &unstaged, false, r.sel_file.as_ref(), has_head, flat, &root);
                    ui.add_space(10.0);
                    let unstage_args: Vec<String> = if has_head { vec!["reset".into(), "-q".into()] } else { vec!["rm".into(), "--cached".into(), "-r".into(), "-q".into(), ".".into()] };
                    file_section_header(ui, tr("Staged", "Vorgemerkt"), staged.len(), if staged.is_empty() { None } else { Some((tr("Unstage All", "Alle zurücknehmen"), "git reset")) }, || {
                        acts.push(Act::Quick { title: tr("Unstage all", "Alles zurücknehmen").into(), args: unstage_args.clone(), explain: String::new() });
                    });
                    if staged.is_empty() {
                        empty_hint(ui, tr("Nothing staged yet", "Noch nichts vorgemerkt"));
                    }
                    file_list(ui, cmds, acts, &staged, true, r.sel_file.as_ref(), has_head, flat, &root);
                });
            });

            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK()).inner_margin(0)).show(ui, |ui| {
                let Some((path, staged_sel)) = r.sel_file.clone() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new(tr("Select a file to see its changes", "Wähle eine Datei, um ihre Änderungen zu sehen")).color(theme::TEXT_DIM()));
                    });
                    return;
                };
                // The diff on screen belongs to this file (a newly picked one may still be loading).
                let ready = r.file_diff_key.as_ref().is_some_and(|(p, s)| *p == path && *s == staged_sel);
                let d = r.file_diff.clone();
                let untracked = r.status.iter().any(|f| f.path == path && f.index == '?');
                let code = r.status.iter().find(|f| f.path == path).map(|f| if staged_sel { f.index } else if f.index == '?' { '?' } else { f.worktree }).unwrap_or('M');
                let split_possible = !(ready && d.combined) && ui.available_width() >= diffview::SPLIT_MIN_WIDTH;
                // File header bar with the diff toolbar on the right.
                egui::Frame::new().fill(theme::PANEL()).inner_margin(egui::Margin::symmetric(12, 5)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        status_badge(ui, code);
                        let (file, dir) = split_path(&path);
                        ui.label(RichText::new(file).font(FontId::new(13.0, theme::semibold())).color(theme::TEXT_STRONG()));
                        if !dir.is_empty() {
                            ui.label(RichText::new(dir).size(12.0).color(theme::TEXT_DIM()));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(o) = diffview::toolbar(ui, &bar, split_possible) {
                                acts.push(Act::DiffOpt(o));
                            }
                            ui.add_space(10.0);
                            ui.label(RichText::new(if staged_sel { tr("Staged", "Vorgemerkt") } else { tr("Unstaged", "Nicht vorgemerkt") }).size(11.5).color(theme::TEXT_FAINT()));
                        });
                    });
                });
                if !ready {
                    loading_hint(ui);
                    return;
                }
                egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 0 }).show(ui, |ui| {
                    diffview::facts(ui, &d);
                });
                if d.unified.is_empty() {
                    diffview::empty_hint(ui, &d, &bar.opts);
                    return;
                }
                // Hunk actions (at the right of each hunk's header), each offered only
                // when the patch built from the diff on screen does exactly what it says.
                let o = r.file_diff_opts;
                let busy = !r.jobs_pending.is_empty() || r.file_diff_req < r.hunk_ok_from;
                // Discard Hunk also needs the check whether git apply would change the file
                // elsewhere (line ends, encoding, filters); it runs right after the diff loads.
                let discard_block = match r.discard_checked {
                    Some((req, false)) if req == r.file_diff_req => None,
                    Some((req, true)) if req == r.file_diff_req => Some(tr("Not for this file: Git would rewrite it outside the hunk (its line ends don't match core.autocrlf or .gitattributes, or it has an encoding, ident or filter attribute). Discard the whole file instead.", "Nicht für diese Datei: Git würde sie auch außerhalb des Abschnitts umschreiben (ihre Zeilenenden passen nicht zu core.autocrlf oder .gitattributes, oder sie hat ein encoding-, ident- oder filter-Attribut). Stattdessen die ganze Datei verwerfen.")),
                    _ => Some(tr("One moment: checking the file's line ends", "Einen Moment: die Zeilenenden der Datei werden geprüft")),
                };
                let mut hunk_bar = |ui: &mut egui::Ui, h: usize| {
                    let why_not = if busy {
                        // The diff on screen is from before the last hunk action.
                        Some(tr("One moment: the last change is being applied and the diff reloaded", "Einen Moment: die letzte Änderung wird angewendet und der Diff neu geladen"))
                    } else if o.ignore_ws {
                        Some(tr("Not while “Ignore whitespace” is on: the hunk then doesn't match the file", "Nicht, solange „Leerzeichen ignorieren“ an ist: der Abschnitt passt dann nicht zur Datei"))
                    } else if o.context == 0 && !o.full {
                        Some(tr("Needs at least one line around the change (“More lines”): without it git can't check where the hunk belongs", "Braucht mindestens eine Zeile um die Änderung („Mehr Zeilen“): ohne sie kann git nicht prüfen, wohin der Abschnitt gehört"))
                    } else if !d.hunk_safe(h) {
                        Some(tr("Not for this hunk: it has characters that aren't UTF-8 (e.g. Windows-1252), which would change. Stage or discard the whole file instead.", "Nicht für diesen Abschnitt: er enthält Zeichen, die kein UTF-8 sind (z. B. Windows-1252) und sich ändern würden. Stattdessen die ganze Datei vormerken oder verwerfen."))
                    } else {
                        None
                    };
                    let ok = why_not.is_none();
                    let off = why_not.unwrap_or("");
                    let small = |t: &str| egui::Button::new(RichText::new(t).size(11.5)).small();
                    if staged_sel {
                        let b = ui.add_enabled(ok, small(tr("Unstage Hunk", "Abschnitt zurücknehmen"))).on_disabled_hover_text(off);
                        if tip_with_cmd(b, tr("Remove only this hunk from staging", "Nur diesen Abschnitt aus der Vormerkung nehmen"), "git apply --cached --reverse").clicked() {
                            hunk = Some((d.hunk_patch(h, true), true, true));
                        }
                    } else {
                        // Discarding writes the file through git's conversions.
                        let (dok, doff) = match discard_block {
                            Some(why) if ok => (false, why),
                            _ => (ok, off),
                        };
                        let b = ui.add_enabled(dok, small(tr("Discard Hunk", "Abschnitt verwerfen"))).on_disabled_hover_text(doff);
                        if tip_with_cmd(b, tr("Throw away only this change in the file. Can't be undone.", "Verwirft nur diese Änderung in der Datei. Nicht rückgängig zu machen."), "git apply --reverse").clicked() {
                            hunk = Some((d.hunk_patch(h, true), true, false));
                        }
                        let b = ui.add_enabled(ok, small(tr("Stage Hunk", "Abschnitt vormerken"))).on_disabled_hover_text(off);
                        if tip_with_cmd(b, tr("Stage only this hunk", "Nur diesen Abschnitt vormerken"), "git apply --cached").clicked() {
                            hunk = Some((d.hunk_patch(h, false), false, true));
                        }
                    }
                };
                let with_bar = !untracked && d.hunks_applicable();
                egui::ScrollArea::vertical().id_salt("wdiff").auto_shrink([false; 2]).show(ui, |ui| {
                    let bar_fn: Option<&mut dyn FnMut(&mut egui::Ui, usize)> = if with_bar { Some(&mut hunk_bar) } else { None };
                    diffview::draw(ui, Id::unique(("wdiff", &path, staged_sel)), &d, bar.look, bar_fn);
                });
            });
        }
        if flat != self.flat_files {
            self.flat_files = flat;
            self.save();
        }
        if let Some((patch, reverse, cached)) = hunk {
            let mut args = vec!["apply".to_string()];
            if cached {
                args.push("--cached".into());
            }
            if reverse {
                args.push("--reverse".into());
            }
            // Exactly the hunk: apply.whitespace=fix in a user's config would change its
            // trailing spaces.
            args.push("--whitespace=nowarn".into());
            args.push("--unidiff-zero".into());
            args.push("-".into());
            let title = match (reverse, cached) {
                (false, true) => tr("Stage hunk", "Abschnitt vormerken"),
                (true, true) => tr("Unstage hunk", "Abschnitt zurücknehmen"),
                _ => tr("Discard hunk", "Abschnitt verwerfen"),
            };
            // (The hunk buttons wait until this job ended and the diff was loaded again, see
            // `App::spawn`.)
            self.quick_stdin(title.into(), args, patch, ctx);
        }
        if do_commit {
            let r = &mut self.tabs[self.active];
            let mut args = vec!["commit".to_string()];
            if r.amend {
                args.push("--amend".into());
                if r.commit_msg.trim().is_empty() {
                    args.push("--no-edit".into());
                }
            }
            if !r.commit_msg.trim().is_empty() {
                args.push("-m".into());
                args.push(r.commit_msg.trim().to_string());
                if !r.commit_desc.trim().is_empty() {
                    args.push("-m".into());
                    args.push(r.commit_desc.trim().to_string());
                }
            }
            r.commit_msg.clear();
            r.commit_desc.clear();
            r.amend = false;
            if self.repo_settings().signoff {
                args.push("--signoff".into());
            }
            if self.p.push_on_commit {
                self.quick_sequence(tr("Commit and push", "Committen und pushen").into(), vec![args, vec!["push".into()]], ctx);
            } else {
                self.quick(tr("Commit", "Commit").into(), args, ctx);
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Welcome

    pub(super) fn welcome(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let w = 560.0f32;
        ui.add_space((ui.available_height() * 0.16).max(30.0));
        ui.vertical_centered(|ui| {
            ui.set_max_width(w);
            ui.label(RichText::new("YAGUI").font(FontId::new(30.0, theme::semibold())).color(theme::TEXT_STRONG()));
            ui.label(RichText::new(tr("A friendly Git client", "Ein freundlicher Git-Client")).size(14.0).color(theme::TEXT_DIM()));
            ui.add_space(26.0);
        });
        ui.horizontal(|ui| {
            let total = 3.0 * 176.0 + 2.0 * 12.0;
            ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
            ui.spacing_mut().item_spacing.x = 12.0;
            for (ic, title, desc, a) in [
                (Icon::Folder, tr("Open", "Öffnen"), tr("A repository on this computer", "Ein Repository auf diesem PC"), 0),
                (Icon::Pull, tr("Clone", "Klonen"), tr("Download from GitHub & co.", "Von GitHub & Co. laden"), 1),
                (Icon::Plus, tr("New", "Neu"), tr("Start a new repository", "Neues Repository beginnen"), 2),
            ] {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(176.0, 104.0), Sense::click());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 6, if resp.hovered() { theme::dk(0x36, 0x36, 0x39) } else { theme::dk(0x30, 0x30, 0x32) });
                p.rect_stroke(rect, 6, Stroke::new(1.0, if resp.hovered() { theme::ACCENT_DARK() } else { theme::LINE() }), egui::StrokeKind::Inside);
                icons::draw(&p, egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 30.0), Vec2::splat(24.0)), ic, theme::ACCENT());
                p.text(rect.center() + Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, title, FontId::new(14.0, theme::semibold()), theme::TEXT_STRONG());
                p.text(rect.center() + Vec2::new(0.0, 32.0), Align2::CENTER_CENTER, desc, FontId::proportional(11.5), theme::TEXT_DIM());
                if resp.clicked() {
                    acts.push(match a {
                        0 => Act::OpenRepoDialog,
                        1 => Act::Open("clone", vec![]),
                        _ => Act::Open("init", vec![]),
                    });
                }
            }
        });
        ui.add_space(14.0);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                let links = [
                    (tr("Browse all Git commands", "Alle Git-Befehle ansehen"), Act::Commands(true)),
                    (tr("Set up SSH keys", "SSH-Schlüssel einrichten"), Act::Ssh),
                    (tr("Accounts", "Konten"), Act::Accounts),
                ];
                let widths: f32 = 3.0 * 150.0;
                ui.add_space(((ui.available_width() - widths) / 2.0).max(0.0));
                for (i, (label, act)) in links.into_iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new("·").color(theme::TEXT_FAINT()));
                    }
                    if ui.link(RichText::new(label).size(12.5)).clicked() {
                        acts.push(act);
                    }
                }
            });
        });
        ui.add_space(24.0);
        if !self.recent.is_empty() {
            ui.vertical_centered(|ui| {
                ui.set_max_width(w);
                ui.label(RichText::new(tr("Recent repositories", "Zuletzt geöffnet")).font(FontId::new(12.5, theme::semibold())).color(theme::TEXT_DIM()));
                ui.add_space(4.0);
                for r in self.recent.clone().into_iter().take(8) {
                    let name = std::path::Path::new(&r).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| r.clone());
                    if list_row(ui, Some(Icon::Folder), &name, false, &r, false, 0.0).clicked() {
                        acts.push(Act::OpenRepo(PathBuf::from(&r)));
                    }
                }
            });
        }
    }
}

fn commit_preview(msg: &str, desc: &str, amend: bool) -> String {
    let mut s = String::from("git commit");
    if amend {
        s.push_str(" --amend");
        if msg.trim().is_empty() {
            s.push_str(" --no-edit");
        }
    }
    if !msg.trim().is_empty() || !amend {
        let m = if msg.trim().is_empty() { "…".to_string() } else { msg.trim().to_string() };
        s.push_str(&format!(" -m {}", catalog::quote_for_display(&m)));
        if !desc.trim().is_empty() {
            s.push_str(" -m …");
        }
    }
    s
}

fn empty_hint(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(RichText::new(text).size(12.0).color(theme::TEXT_FAINT()));
    });
}

fn file_section_header(ui: &mut egui::Ui, title: &str, count: usize, action: Option<(&str, &str)>, mut on_click: impl FnMut()) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(title).font(FontId::new(12.5, theme::semibold())).color(theme::TEXT()));
        ui.label(RichText::new(count.to_string()).size(11.5).color(theme::TEXT_FAINT()));
        if let Some((label, cmd)) = action {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                let b = ui.small_button(label);
                if tip_with_cmd(b, label, cmd).clicked() {
                    on_click();
                }
            });
        }
    });
    ui.add_space(2.0);
}

/// The changed files of one section (unstaged or staged), as a folder tree or a flat list.
/// Folders can be staged, unstaged, stashed or discarded as a whole.
#[allow(clippy::too_many_arguments)]
fn file_list(ui: &mut egui::Ui, cmds: &[catalog::Cmd], acts: &mut Vec<Act>, files: &[FileStatus], staged: bool, sel: Option<&(String, bool)>, has_head: bool, flat: bool, root: &std::path::Path) {
    let is_sel = |f: &FileStatus| sel.map(|(p, s)| *p == f.path && *s == staged).unwrap_or(false);
    if flat {
        for f in files {
            file_row(ui, cmds, acts, f, staged, is_sel(f), has_head, 0.0, true);
        }
        return;
    }
    let salt = if staged { "staged" } else { "unstaged" };
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let ctx = ui.ctx().clone();
    for row in tree::rows(&ctx, salt, &paths, true) {
        match row {
            Row::Dir { path, label, depth, count, open } => {
                let (resp, action) = tree::dir_row(ui, &label, depth, count, open, Some(if staged { Icon::Close } else { Icon::Plus }));
                let dir = format!("{path}/");
                let inside: Vec<&FileStatus> = files.iter().filter(|f| f.path.starts_with(&dir)).collect();
                let (title, args) = folder_stage_args(&dir, staged, has_head);
                if action {
                    acts.push(Act::Quick { title, args: args.clone(), explain: String::new() });
                } else if resp.clicked() {
                    tree::toggle(&ctx, salt, &path, true);
                }
                let what = if staged {
                    trs(format!("Folder {path} · × unstages all {count} files in it"), format!("Ordner {path} · × nimmt alle {count} Dateien darin zurück"))
                } else {
                    trs(format!("Folder {path} · + stages all {count} files in it"), format!("Ordner {path} · + merkt alle {count} Dateien darin vor"))
                };
                let resp = tip_with_cmd(resp, &what, &format!("git {}", args.join(" ")));
                let any_untracked = inside.iter().any(|f| f.index == '?');
                let any_tracked = inside.iter().any(|f| f.index != '?');
                resp.context_menu(|ui| folder_menu(ui, cmds, acts, &dir, count, staged, has_head, any_tracked, any_untracked, root, salt, &paths));
            }
            Row::File { idx, depth, .. } => {
                let f = &files[idx];
                file_row(ui, cmds, acts, f, staged, is_sel(f), has_head, 8.0 + depth as f32 * tree::INDENT, false);
            }
        }
    }
}

fn folder_stage_args(dir: &str, staged: bool, has_head: bool) -> (String, Vec<String>) {
    if !staged {
        (trs(format!("Stage folder {dir}"), format!("Ordner {dir} vormerken")), vec!["add".into(), "--".into(), dir.into()])
    } else if has_head {
        (trs(format!("Unstage folder {dir}"), format!("Ordner {dir} zurücknehmen")), vec!["restore".into(), "--staged".into(), "--".into(), dir.into()])
    } else {
        (trs(format!("Unstage folder {dir}"), format!("Ordner {dir} zurücknehmen")), vec!["rm".into(), "--cached".into(), "-r".into(), "-q".into(), "--".into(), dir.into()])
    }
}

#[allow(clippy::too_many_arguments)]
fn folder_menu(ui: &mut egui::Ui, cmds: &[catalog::Cmd], acts: &mut Vec<Act>, dir: &str, count: usize, staged: bool, has_head: bool, any_tracked: bool, any_untracked: bool, root: &std::path::Path, salt: &str, paths: &[&str]) {
    ui.set_min_width(250.0);
    ui.label(RichText::new(trs(format!("{dir}  ·  {count} files"), format!("{dir}  ·  {count} Dateien"))).size(11.5).color(theme::TEXT_FAINT()));
    ui.separator();
    let (title, args) = folder_stage_args(dir, staged, has_head);
    let label = if staged { tr("Unstage folder", "Ordner zurücknehmen") } else { tr("Stage folder", "Ordner vormerken") };
    let b = ui.button(label);
    if tip_with_cmd(b, &title, &format!("git {}", args.join(" "))).clicked() {
        acts.push(Act::Quick { title, args, explain: String::new() });
        ui.close();
    }
    let mut fills = vec![("paths", dir.to_string())];
    if any_untracked {
        fills.push(("flag:-u", "1".into()));
    }
    menu_cmd(ui, cmds, acts, "stash_push", Some(tr("Stash folder…", "Ordner zwischenlagern (Stash)…")), fills);
    if !staged {
        if any_tracked {
            menu_cmd(ui, cmds, acts, "restore", Some(tr("Discard changes in folder…", "Änderungen im Ordner verwerfen…")), vec![("paths", dir.to_string())]);
        }
        if any_untracked {
            menu_cmd(ui, cmds, acts, "clean", Some(tr("Delete new files in folder…", "Neue Dateien im Ordner löschen…")), vec![("paths", dir.to_string())]);
        }
        menu_cmd(ui, cmds, acts, "commit", Some(tr("Commit only this folder…", "Nur diesen Ordner committen…")), vec![("paths", dir.to_string())]);
    } else {
        menu_cmd(ui, cmds, acts, "commit", Some(tr("Commit only this folder…", "Nur diesen Ordner committen…")), vec![("paths", dir.to_string())]);
    }
    ui.separator();
    menu_cmd(ui, cmds, acts, "log", Some(tr("Folder history…", "Ordnerverlauf…")), vec![("paths", dir.to_string())]);
    menu_item(ui, acts, tr("Show in Explorer", "Im Explorer zeigen"), Act::Explorer(root.join(dir.trim_end_matches('/'))));
    menu_item(ui, acts, tr("Copy path", "Pfad kopieren"), Act::Copy(dir.to_string()));
    ui.separator();
    if ui.button(tr("Expand all", "Alle aufklappen")).clicked() {
        tree::set_all(ui.ctx(), salt, paths, true);
        ui.close();
    }
    if ui.button(tr("Collapse all", "Alle zuklappen")).clicked() {
        tree::set_all(ui.ctx(), salt, paths, false);
        ui.close();
    }
}

#[allow(clippy::too_many_arguments)]
fn file_row(ui: &mut egui::Ui, cmds: &[catalog::Cmd], acts: &mut Vec<Act>, f: &FileStatus, staged: bool, selected: bool, has_head: bool, indent: f32, show_dir: bool) {
    let code = if f.conflicted() { 'U' } else if staged { f.index } else if f.index == '?' { '?' } else { f.worktree };
    let h = 24.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect, 0, theme::ROW_SEL());
    } else if resp.hovered() {
        p.rect_filled(rect, 0, theme::ROW_HOVER());
    }
    // status badge
    let br = egui::Rect::from_center_size(egui::pos2(rect.left() + 22.0 + indent, rect.center().y), Vec2::splat(13.0));
    let col = status_color(code);
    p.rect_filled(br, 3, col.gamma_multiply(0.22));
    p.rect_stroke(br, 3, Stroke::new(1.0, col.gamma_multiply(0.85)), egui::StrokeKind::Inside);
    p.text(br.center(), Align2::CENTER_CENTER, if code == '?' { 'A' } else { code }, FontId::new(9.0, theme::semibold()), col);
    // name + folder
    let shown = match &f.orig {
        Some(o) => format!("{o} → {}", f.path),
        None => f.path.clone(),
    };
    let (file, dir) = split_path(&shown);
    let dir = if show_dir { dir } else { "" };
    let x = rect.left() + 36.0 + indent;
    let action_w = if resp.hovered() || selected { 30.0 } else { 0.0 };
    let clip = egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(rect.right() - 8.0 - action_w, rect.bottom()));
    let pc = ui.painter_at(clip);
    let g = pc.layout_no_wrap(file.to_string(), FontId::proportional(13.0), theme::TEXT());
    let gw = g.size().x;
    pc.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, theme::TEXT());
    if !dir.is_empty() {
        pc.text(egui::pos2(x + gw + 8.0, rect.center().y), Align2::LEFT_CENTER, dir, FontId::proportional(11.5), theme::TEXT_FAINT());
    }
    // hover action: stage / unstage
    let mut action_clicked = false;
    if action_w > 0.0 {
        let ar = egui::Rect::from_center_size(egui::pos2(rect.right() - 20.0, rect.center().y), Vec2::new(22.0, 18.0));
        let hovered = resp.hover_pos().map(|p| ar.contains(p)).unwrap_or(false);
        p.rect_filled(ar, 4, if hovered { theme::dk(0x4a, 0x4a, 0x4f) } else { theme::dk(0x3a, 0x3a, 0x3d) });
        icons::draw(&p, ar.shrink(4.0), if staged { Icon::Close } else { Icon::Plus }, theme::TEXT_STRONG());
        if resp.clicked() && hovered {
            action_clicked = true;
        }
    }
    let (tip, args): (String, Vec<String>) = if staged {
        if has_head {
            (format!("git restore --staged -- {}", f.path), vec!["restore".into(), "--staged".into(), "--".into(), f.path.clone()])
        } else {
            (format!("git rm --cached -- {}", f.path), vec!["rm".into(), "--cached".into(), "-q".into(), "--".into(), f.path.clone()])
        }
    } else {
        (format!("git add -- {}", f.path), vec!["add".into(), "--".into(), f.path.clone()])
    };
    if action_clicked {
        acts.push(Act::Quick { title: if staged { trs(format!("Unstage {file}"), format!("{file} zurücknehmen")) } else { trs(format!("Stage {file}"), format!("{file} vormerken")) }, args, explain: String::new() });
    } else if resp.clicked() {
        acts.push(Act::SelectFile(f.path.clone(), staged));
    }
    if resp.double_clicked() {
        // double-click toggles staging
        let (_, args2) = (tip.clone(), if staged { vec!["restore".into(), "--staged".into(), "--".into(), f.path.clone()] } else { vec!["add".into(), "--".into(), f.path.clone()] });
        acts.push(Act::Quick { title: if staged { trs(format!("Unstage {file}"), format!("{file} zurücknehmen")) } else { trs(format!("Stage {file}"), format!("{file} vormerken")) }, args: args2, explain: String::new() });
    }
    let what = format!("{} · {}", status_word(code), if staged { tr("click × to unstage", "× zum Zurücknehmen") } else { tr("click + to stage", "+ zum Vormerken") });
    let resp = tip_with_cmd(resp, &what, &tip);
    let p2 = f.path.clone();
    let untracked = f.index == '?';
    let conflict = f.conflicted();
    resp.context_menu(|ui| {
        if conflict {
            menu_item(ui, acts, tr("Resolve using mine", "Meine Version nehmen"), Act::Quick { title: tr("Resolve using mine", "Meine Version nehmen").into(), args: vec!["checkout".into(), "--ours".into(), "--".into(), p2.clone()], explain: String::new() });
            menu_item(ui, acts, tr("Resolve using theirs", "Ihre Version nehmen"), Act::Quick { title: tr("Resolve using theirs", "Ihre Version nehmen").into(), args: vec!["checkout".into(), "--theirs".into(), "--".into(), p2.clone()], explain: String::new() });
            menu_item(ui, acts, tr("Mark as resolved", "Als gelöst markieren"), Act::Quick { title: tr("Mark as resolved", "Als gelöst markieren").into(), args: vec!["add".into(), "--".into(), p2.clone()], explain: String::new() });
            menu_cmd(ui, cmds, acts, "mergetool", Some(tr("Open in merge tool…", "Im Merge-Programm öffnen…")), vec![("paths", p2.clone())]);
            ui.separator();
        }
        if staged {
            menu_item(ui, acts, tr("Unstage", "Zurücknehmen"), Act::Quick { title: tr("Unstage", "Zurücknehmen").into(), args: vec!["restore".into(), "--staged".into(), "--".into(), p2.clone()], explain: String::new() });
        } else {
            menu_item(ui, acts, tr("Stage", "Vormerken"), Act::Quick { title: tr("Stage", "Vormerken").into(), args: vec!["add".into(), "--".into(), p2.clone()], explain: String::new() });
            if untracked {
                menu_cmd(ui, cmds, acts, "clean", Some(tr("Delete file…", "Datei löschen…")), vec![("paths", p2.clone())]);
            } else {
                menu_cmd(ui, cmds, acts, "restore", Some(tr("Discard changes…", "Änderungen verwerfen…")), vec![("paths", p2.clone())]);
            }
        }
        ui.separator();
        if !untracked {
            menu_cmd(ui, cmds, acts, "blame", Some(tr("Blame…", "Blame…")), vec![("file", p2.clone())]);
            menu_cmd(ui, cmds, acts, "log", Some(tr("File history…", "Dateiverlauf…")), vec![("paths", p2.clone()), ("flag:--follow", "1".into())]);
            menu_cmd(ui, cmds, acts, "rm", Some(tr("Stop tracking…", "Nicht mehr verfolgen…")), vec![("paths", p2.clone()), ("flag:--cached", "1".into())]);
            menu_cmd(ui, cmds, acts, "assume_unchanged", Some(tr("Ignore changes temporarily…", "Änderungen vorübergehend ignorieren…")), vec![("paths", p2.clone())]);
        } else {
            menu_cmd(ui, cmds, acts, "check_ignore", Some(tr("Check ignore rules…", "Ignorier-Regeln prüfen…")), vec![("paths", p2.clone())]);
        }
        ui.separator();
        menu_item(ui, acts, tr("Copy path", "Pfad kopieren"), Act::Copy(p2.clone()));
    });
}

/// What a branch's right-click menu needs besides the branch itself.
pub(super) struct BranchExtra {
    /// (remote, URL of a new pull request for this branch)
    pub pr: Option<(String, String)>,
    pub remote_branches: Vec<String>,
    pub customs: Vec<(usize, super::extra::CustomCmd)>,
    pub pinned: bool,
    pub filtered: bool,
    pub hidden: bool,
}

/// The page that starts a pull request for `branch` on a hosting site.
fn pr_url(web: &str, branch: &str) -> String {
    if web.contains("gitlab") {
        format!("{web}/-/merge_requests/new?merge_request[source_branch]={branch}")
    } else if web.contains("bitbucket") {
        format!("{web}/pull-requests/new?source={branch}")
    } else if web.contains("dev.azure.com") || web.contains("visualstudio.com") {
        format!("{web}/pullrequestcreate?sourceRef={branch}")
    } else {
        format!("{web}/compare/{branch}?expand=1")
    }
}

#[allow(clippy::too_many_arguments)]
fn branch_menu(ui: &mut egui::Ui, cmds: &[catalog::Cmd], acts: &mut Vec<Act>, name: &str, upstream: &str, head: &str, remote: bool, extra: &BranchExtra) {
    let n = name.to_string();
    if remote {
        menu_cmd(ui, cmds, acts, "checkout_track", Some(&trs(format!("Checkout {n}…"), format!("{n} auschecken…"))), vec![("branch", n.clone())]);
        menu_cmd(ui, cmds, acts, "worktree_add", Some(tr("Checkout as Worktree…", "Als Worktree auschecken…")), vec![("branch", n.clone())]);
        ui.separator();
        menu_cmd(ui, cmds, acts, "pull", Some(&trs(format!("Pull into {head}…"), format!("In {head} pullen…"))), vec![]);
        menu_cmd(ui, cmds, acts, "merge", Some(&trs(format!("Merge into {head}…"), format!("In {head} zusammenführen…"))), vec![("branch", n.clone())]);
        menu_cmd(ui, cmds, acts, "rebase", Some(&trs(format!("Rebase {head} on {n}…"), format!("{head} auf {n} neu aufsetzen…"))), vec![("branch", n.clone())]);
        menu_item(ui, acts, &trs(format!("Interactively Rebase {head} on {n}…"), format!("{head} interaktiv auf {n} neu aufsetzen…")), Act::Rebase(n.clone()));
        if let Some((rn, url)) = &extra.pr {
            if ui.button(trs(format!("Create Pull Request on '{rn}'"), format!("Pull Request auf '{rn}' erstellen"))).clicked() {
                super::auth::open_url(url);
                ui.close();
            }
        }
        ui.separator();
        menu_item(ui, acts, tr("New Branch…", "Neuer Branch…"), Act::NewBranch(Some(n.clone())));
        menu_item(ui, acts, tr("New Tag…", "Neuer Tag…"), Act::NewTag(Some(n.clone())));
        ui.separator();
        if let Some((r, b)) = n.split_once('/') {
            menu_cmd(ui, cmds, acts, "push_delete", Some(&trs(format!("Delete {n}…"), format!("{n} löschen…"))), vec![("remote", r.to_string()), ("branch", b.to_string())]);
        }
        menu_item(ui, acts, if extra.filtered { tr("Stop filtering", "Filter aufheben") } else { tr("Show Only This Branch", "Nur diesen Branch zeigen") }, Act::FilterBranch(n.clone()));
    } else {
        let is_head = n == head;
        if !is_head {
            menu_cmd(ui, cmds, acts, "switch", Some(&trs(format!("Checkout {n}…"), format!("{n} auschecken…"))), vec![("branch", n.clone())]);
        }
        menu_cmd(ui, cmds, acts, "worktree_add", Some(tr("Checkout as Worktree…", "Als Worktree auschecken…")), vec![("branch", n.clone())]);
        if !upstream.is_empty() {
            if is_head {
                menu_cmd(ui, cmds, acts, "merge", Some(&trs(format!("Fast-Forward to '{upstream}'"), format!("Vorspulen auf '{upstream}'"))), vec![("branch", upstream.to_string()), ("flag:--ff-only", "1".into())]);
                menu_cmd(ui, cmds, acts, "pull", Some(&trs(format!("Pull '{upstream}'…"), format!("'{upstream}' pullen…"))), vec![]);
            } else {
                let b = ui.button(trs(format!("Fast-Forward to '{upstream}'"), format!("Vorspulen auf '{upstream}'")));
                if tip_with_cmd(b, tr("Moves this branch forward without checking it out", "Bewegt diesen Branch vorwärts, ohne ihn auszuchecken"), &format!("git fetch . {upstream}:{n}")).clicked() {
                    acts.push(Act::Quick { title: trs(format!("Fast-forward {n}"), format!("{n} vorspulen")), args: vec!["fetch".into(), ".".into(), format!("{upstream}:{n}")], explain: String::new() });
                    ui.close();
                }
            }
        }
        let remote_name = upstream.split_once('/').map(|x| x.0.to_string()).unwrap_or_else(|| "origin".into());
        menu_cmd(ui, cmds, acts, "push", Some(&trs(format!("Push to '{remote_name}'…"), format!("Nach '{remote_name}' pushen…"))), vec![("branch", n.clone())]);
        if let Some((rn, url)) = &extra.pr {
            if ui.button(trs(format!("Create Pull Request on '{rn}'"), format!("Pull Request auf '{rn}' erstellen"))).clicked() {
                super::auth::open_url(url);
                ui.close();
            }
        }
        if !is_head {
            ui.separator();
            menu_cmd(ui, cmds, acts, "merge", Some(&trs(format!("Merge {n} into {head}…"), format!("{n} in {head} zusammenführen…"))), vec![("branch", n.clone())]);
            menu_cmd(ui, cmds, acts, "rebase", Some(&trs(format!("Rebase {head} on {n}…"), format!("{head} auf {n} neu aufsetzen…"))), vec![("branch", n.clone())]);
            menu_item(ui, acts, &trs(format!("Interactively Rebase {head} on {n}…"), format!("{head} interaktiv auf {n} neu aufsetzen…")), Act::Rebase(n.clone()));
        }
        ui.separator();
        menu_item(ui, acts, tr("New Branch…", "Neuer Branch…"), Act::NewBranch(Some(n.clone())));
        menu_item(ui, acts, tr("New Tag…", "Neuer Tag…"), Act::NewTag(Some(n.clone())));
        ui.menu_button(tr("Tracking", "Verbindung zum Server"), |ui| {
            if !upstream.is_empty() {
                menu_cmd(ui, cmds, acts, "branch_unset_upstream", Some(tr("Remove tracking reference", "Verbindung lösen")), vec![("branch", n.clone())]);
                ui.separator();
            }
            ui.set_max_height(360.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for rb in &extra.remote_branches {
                    if rb.ends_with("/HEAD") {
                        continue;
                    }
                    if ui.radio(rb == upstream, rb).clicked() && rb != upstream {
                        acts.push(Act::Quick { title: trs(format!("Track {rb}"), format!("Mit {rb} verbinden")), args: vec!["branch".into(), format!("--set-upstream-to={rb}"), n.clone()], explain: String::new() });
                        ui.close();
                    }
                }
            });
        });
        menu_cmd(ui, cmds, acts, "branch_rename", Some(tr("Rename…", "Umbenennen…")), vec![("branch", n.clone())]);
        ui.add_enabled_ui(!is_head, |ui| {
            menu_cmd(ui, cmds, acts, "branch_delete", Some(tr("Delete…", "Löschen…")), vec![("branch", n.clone())]);
        });
        ui.separator();
        menu_item(ui, acts, if extra.pinned { tr("Unpin", "Nicht mehr anheften") } else { tr("Pin to Top", "Oben anheften") }, Act::PinBranch(n.clone()));
        menu_item(ui, acts, if extra.filtered { tr("Stop filtering", "Filter aufheben") } else { tr("Show Only This Branch", "Nur diesen Branch zeigen") }, Act::FilterBranch(n.clone()));
        menu_item(ui, acts, if extra.hidden { tr("Show in Commit List", "In der Commit-Liste zeigen") } else { tr("Hide from Commit List", "In der Commit-Liste ausblenden") }, Act::HideBranch(n.clone()));
    }
    if !extra.customs.is_empty() {
        ui.separator();
        for (i, cc) in &extra.customs {
            if ui.button(&cc.name).on_hover_text(&cc.command).clicked() {
                acts.push(Act::Custom(*i, n.clone()));
                ui.close();
            }
        }
    }
    ui.separator();
    menu_item(ui, acts, tr("Copy Branch Name", "Branch-Namen kopieren"), Act::Copy(n));
}

#[allow(clippy::too_many_arguments)]
fn commit_menu(ui: &mut egui::Ui, cmds: &[catalog::Cmd], acts: &mut Vec<Act>, sha: &str, short: &str, subject: &str, parents: usize, head: &str, customs: &[(usize, super::extra::CustomCmd)]) {
    let s = sha.to_string();
    menu_item(ui, acts, tr("New Branch…", "Neuer Branch…"), Act::NewBranch(Some(short.to_string())));
    menu_item(ui, acts, tr("New Tag…", "Neuer Tag…"), Act::NewTag(Some(short.to_string())));
    ui.separator();
    ui.menu_button(tr("Interactive Rebase", "Interaktiver Rebase"), |ui| {
        menu_item(ui, acts, &trs(format!("Interactively Rebase '{head}' to Here…"), format!("'{head}' ab hier interaktiv neu aufsetzen…")), Act::Rebase(s.clone()));
        ui.separator();
        ui.label(RichText::new(tr("Quick Actions", "Schnellaktionen")).size(11.5).color(theme::TEXT_FAINT()));
        ui.add_enabled_ui(parents <= 1, |ui| {
            menu_item(ui, acts, tr("Reword…", "Beschreibung ändern…"), Act::RebaseAction(s.clone(), 1));
            menu_item(ui, acts, tr("Edit…", "Bearbeiten…"), Act::RebaseAction(s.clone(), 2));
            menu_item(ui, acts, tr("Squash into Parent…", "Mit Vorgänger zusammenfassen…"), Act::RebaseAction(s.clone(), 3));
            menu_item(ui, acts, tr("Fixup into Parent…", "In Vorgänger einschmelzen…"), Act::RebaseAction(s.clone(), 4));
            menu_item(ui, acts, tr("Drop…", "Entfernen…"), Act::RebaseAction(s.clone(), 5));
        });
    });
    menu_cmd(ui, cmds, acts, "reset", Some(&trs(format!("Reset '{head}' to Here…"), format!("'{head}' hierher zurücksetzen…"))), vec![("commit", s.clone())]);
    menu_cmd(ui, cmds, acts, "checkout", Some(tr("Checkout Commit…", "Commit auschecken…")), vec![("branch", s.clone())]);
    menu_cmd(ui, cmds, acts, "cherry_pick", Some(tr("Cherry-pick Commit…", "Commit übernehmen (Cherry-pick)…")), vec![("commits", s.clone())]);
    let mut rv: Vec<(&'static str, String)> = vec![("commits", s.clone())];
    if parents > 1 {
        rv.push(("parent", "1".into()));
    }
    menu_cmd(ui, cmds, acts, "revert", Some(tr("Revert Commit…", "Commit rückgängig machen (Revert)…")), rv);
    ui.separator();
    menu_cmd(ui, cmds, acts, "merge", Some(&trs(format!("Merge into '{head}'…"), format!("In '{head}' zusammenführen…"))), vec![("branch", s.clone())]);
    menu_cmd(ui, cmds, acts, "rebase", Some(&trs(format!("Rebase '{head}' onto Here…"), format!("'{head}' hierauf neu aufsetzen…"))), vec![("branch", s.clone())]);
    ui.separator();
    menu_cmd(ui, cmds, acts, "format_patch", Some(tr("Save as Patch…", "Als Patch speichern…")), vec![("range", format!("-1 {short}"))]);
    menu_cmd(ui, cmds, acts, "diff", Some(tr("Compare to Local Changes…", "Mit lokalen Änderungen vergleichen…")), vec![("a", s.clone())]);
    menu_cmd(ui, cmds, acts, "notes_add", Some(tr("Add Note…", "Notiz hinzufügen…")), vec![("commit", s.clone())]);
    if !customs.is_empty() {
        ui.separator();
        for (i, cc) in customs {
            if ui.button(&cc.name).on_hover_text(&cc.command).clicked() {
                acts.push(Act::Custom(*i, s.clone()));
                ui.close();
            }
        }
    }
    ui.separator();
    menu_item(ui, acts, tr("Copy Commit SHA", "Commit-SHA kopieren"), Act::Copy(s.clone()));
    menu_item(ui, acts, tr("Copy Commit Info", "Commit-Info kopieren"), Act::Copy(format!("{short} {subject}")));
}

fn detail_pane(ui: &mut egui::Ui, r: &mut Repo, acts: &mut Vec<Act>, rules: &[super::extra::IssueRule], bar: &diffview::Bar) {
    egui::Frame::new().fill(theme::PANEL()).inner_margin(egui::Margin { left: 14, right: 14, top: 6, bottom: 0 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            for (t, label) in [(DetailTab::Commit, tr("Commit", "Commit")), (DetailTab::Changes, tr("Changes", "Änderungen")), (DetailTab::Tree, tr("File Tree", "Dateibaum"))] {
                let sel = r.detail_tab == t;
                let txt = RichText::new(label).font(FontId::new(13.0, theme::semibold())).color(if sel { theme::ACCENT() } else { theme::TEXT_DIM() });
                let resp = ui.add(egui::Button::new(txt).frame(false));
                if sel {
                    let rr = resp.rect;
                    ui.painter().line_segment([rr.left_bottom() + Vec2::new(0.0, 5.0), rr.right_bottom() + Vec2::new(0.0, 5.0)], Stroke::new(2.0, theme::ACCENT()));
                }
                if resp.clicked() {
                    acts.push(Act::DetailTab(t));
                }
            }
        });
        ui.add_space(5.0);
    });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, theme::LINE());
    let Some(d) = &r.detail else {
        if r.wanted_detail.is_some() && r.bg_detail.busy() {
            loading_hint(ui);
        } else {
            ui.add_space(10.0);
            ui.label(RichText::new(tr("Select a commit", "Wähle einen Commit")).color(theme::TEXT_DIM()));
        }
        return;
    };
    // Diffs of files opened in the Commit tab that still have to be loaded.
    let mut want: Vec<(String, String)> = vec![];
    match r.detail_tab {
        DetailTab::Commit => {
            egui::ScrollArea::vertical().id_salt("detail_commit").auto_shrink([false; 2]).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin { left: 20, right: 20, top: 12, bottom: 12 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
                        ui.painter().rect_filled(rect, 5, theme::avatar_color(&d.author));
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, initials(&d.author), FontId::new(16.0, theme::semibold()), Color32::WHITE);
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&d.author).font(FontId::new(13.5, theme::semibold())).color(theme::TEXT_STRONG()));
                                ui.label(RichText::new(&d.email).size(12.0).color(theme::TEXT_DIM()));
                            });
                            ui.label(RichText::new(&d.date).size(12.0).color(theme::TEXT_DIM()));
                        });
                    });
                    ui.add_space(8.0);
                    egui::Grid::new("meta").num_columns(2).spacing([16.0, 5.0]).show(ui, |ui| {
                        let key = |ui: &mut egui::Ui, t: &str| {
                            ui.label(RichText::new(t).size(11.5).color(theme::TEXT_FAINT()));
                        };
                        if !d.refs.is_empty() {
                            key(ui, tr("Refs", "Refs"));
                            ui.label(RichText::new(&d.refs).size(12.5));
                            ui.end_row();
                        }
                        key(ui, "SHA");
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&d.sha).monospace().size(12.0));
                            if icon_button(ui, Icon::Changes, tr("Copy SHA", "SHA kopieren")).clicked() {
                                acts.push(Act::Copy(d.sha.clone()));
                            }
                        });
                        ui.end_row();
                        key(ui, tr("Parents", "Eltern"));
                        ui.horizontal(|ui| {
                            for p in &d.parents {
                                if ui.link(RichText::new(&p[..p.len().min(8)]).monospace().size(12.0)).clicked() {
                                    acts.push(Act::JumpToRef(p.clone()));
                                }
                            }
                        });
                        ui.end_row();
                        if d.committer != d.author && !d.committer.is_empty() {
                            key(ui, tr("Committer", "Committer"));
                            ui.label(RichText::new(&d.committer).size(12.5));
                            ui.end_row();
                        }
                    });
                    ui.add_space(10.0);
                    let mut lines = d.message.lines();
                    ui.label(RichText::new(lines.next().unwrap_or("")).font(FontId::new(15.0, theme::semibold())).color(theme::TEXT_STRONG()));
                    let rest: String = lines.collect::<Vec<_>>().join("\n");
                    if !rest.trim().is_empty() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(rest.trim()).size(12.5));
                    }
                    let links = super::extra::issue_links(rules, &d.message);
                    if !links.is_empty() {
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(tr("Issues:", "Issues:")).size(12.0).color(theme::TEXT_DIM()));
                            for (t, u) in links {
                                if ui.link(t).on_hover_text(&u).clicked() {
                                    super::auth::open_url(&u);
                                }
                            }
                        });
                    }
                    ui.add_space(12.0);
                    let all_key = Id::unique(("expand-all", &d.sha));
                    let mut expand_all = ui.ctx().data(|m| m.get_temp::<bool>(all_key)).unwrap_or(false);
                    // The opened diffs below get this width less their frame's border (a merge
                    // conflict shows in one column anyway).
                    let split_possible = ui.available_width() - 2.0 >= diffview::SPLIT_MIN_WIDTH;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(trs(format!("{} changed files", d.files.len()), format!("{} geänderte Dateien", d.files.len()))).size(11.5).color(theme::TEXT_FAINT()));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(o) = diffview::toolbar(ui, bar, split_possible) {
                                acts.push(Act::DiffOpt(o));
                            }
                            ui.add_space(12.0);
                            if ui.add(egui::Button::new(RichText::new(if expand_all { tr("Collapse All", "Alle zuklappen") } else { tr("Expand All", "Alle aufklappen") }).size(12.0)).frame(false)).clicked() {
                                expand_all = !expand_all;
                                ui.ctx().data_mut(|m| m.insert_temp(all_key, expand_all));
                                for (_, path) in &d.files {
                                    ui.ctx().data_mut(|m| m.insert_temp(Id::unique(("file-open", &d.sha, path)), expand_all));
                                }
                            }
                        });
                    });
                    ui.add_space(2.0);
                    for (st, path) in &d.files {
                        let code = st.chars().next().unwrap_or('M');
                        let open_key = Id::unique(("file-open", &d.sha, path));
                        let is_open = ui.ctx().data(|m| m.get_temp::<bool>(open_key)).unwrap_or(false);
                        let resp = ui.horizontal(|ui| {
                            if icon_button(ui, if is_open { Icon::ChevronDown } else { Icon::ChevronRight }, tr("Show the changes here", "Änderungen hier zeigen")).clicked() {
                                ui.ctx().data_mut(|m| m.insert_temp(open_key, !is_open));
                            }
                            status_badge(ui, code);
                            let (file, dir) = split_path(path);
                            let r = ui.add(egui::Button::new(RichText::new(file).size(13.0)).frame(false));
                            if !dir.is_empty() {
                                ui.label(RichText::new(dir).size(11.5).color(theme::TEXT_FAINT()));
                            }
                            r
                        }).inner;
                        if resp.on_hover_text(status_word(code)).clicked() {
                            acts.push(Act::DetailFile(path.clone()));
                        }
                        if is_open {
                            let got = r.inline_diff(&d.sha, path);
                            if got.is_none() {
                                want.push((d.sha.clone(), path.clone()));
                            }
                            egui::Frame::new().fill(theme::PANEL_DARK()).stroke(Stroke::new(1.0, theme::LINE())).corner_radius(3).inner_margin(egui::Margin::symmetric(0, 4)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                match &got {
                                    None => loading_hint(ui),
                                    Some(diff) => {
                                        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| diffview::facts(ui, diff));
                                        if diff.unified.is_empty() {
                                            diffview::empty_hint(ui, diff, &bar.opts);
                                        } else {
                                            diffview::draw(ui, Id::unique(("inline", &d.sha, path)), diff, bar.look, None);
                                        }
                                    }
                                }
                            });
                            ui.add_space(4.0);
                        }
                    }
                });
            });
        }
        DetailTab::Changes => {
            egui::Panel::left("detail_files").resizable(true).default_size(260.0).frame(egui::Frame::new().fill(theme::PANEL()).inner_margin(egui::Margin::symmetric(4, 6))).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("dfiles").auto_shrink([false; 2]).show(ui, |ui| {
                    let paths: Vec<&str> = d.files.iter().map(|f| f.1.as_str()).collect();
                    let ctx = ui.ctx().clone();
                    for row in tree::rows(&ctx, "detail", &paths, true) {
                        match row {
                            Row::Dir { path, label, depth, count, open } => {
                                if tree::dir_row(ui, &label, depth, count, open, None).0.clicked() {
                                    tree::toggle(&ctx, "detail", &path, true);
                                }
                            }
                            Row::File { idx, name, depth } => {
                                let (st, path) = &d.files[idx];
                                let indent = 8.0 + depth as f32 * tree::INDENT;
                                let sel = r.detail_file.as_deref() == Some(path.as_str());
                                let resp = list_row(ui, None, &name, false, "", sel, indent + 18.0).on_hover_text(path);
                                let br = egui::Rect::from_center_size(egui::pos2(resp.rect.left() + indent + 14.0, resp.rect.center().y), Vec2::splat(12.0));
                                let code = st.chars().next().unwrap_or('M');
                                let col = status_color(code);
                                ui.painter().rect_filled(br, 3, col.gamma_multiply(0.25));
                                ui.painter().text(br.center(), Align2::CENTER_CENTER, code, FontId::new(8.5, theme::semibold()), col);
                                if resp.clicked() {
                                    acts.push(Act::DetailFile(path.clone()));
                                }
                            }
                        }
                    }
                });
            });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK()).inner_margin(0)).show(ui, |ui| {
                match r.detail_file.as_deref() {
                    Some(f) => {
                        let ready = r.detail_diff_key.as_ref().is_some_and(|(s, p)| *s == d.sha && p == f);
                        let code = d.files.iter().find(|x| x.1 == f).and_then(|x| x.0.chars().next()).unwrap_or('M');
                        diff_panel(ui, acts, bar, code, f, if ready { Some(&*r.detail_diff) } else { None }, Id::unique(("ddiff", f)));
                    }
                    None => {
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.label(RichText::new(tr("Select a file", "Wähle eine Datei")).color(theme::TEXT_DIM()));
                        });
                    }
                }
            });
        }
        DetailTab::Tree => {
            egui::Panel::left("tree_files").resizable(true).default_size(300.0).frame(egui::Frame::new().fill(theme::PANEL()).inner_margin(egui::Margin::symmetric(4, 6))).show(ui, |ui| {
                // The whole tree of the commit: folders start closed, so big projects stay fast.
                let paths: Vec<&str> = r.tree.iter().map(|s| s.as_str()).collect();
                let ctx = ui.ctx().clone();
                let rows = tree::rows(&ctx, "commit-tree", &paths, false);
                egui::ScrollArea::vertical().id_salt("tfiles").auto_shrink([false; 2]).show_rows(ui, 24.0, rows.len(), |ui, range| {
                    for row in &rows[range] {
                        match row {
                            Row::Dir { path, label, depth, count, open } => {
                                if tree::dir_row(ui, label, *depth, *count, *open, None).0.clicked() {
                                    tree::toggle(&ctx, "commit-tree", path, false);
                                }
                            }
                            Row::File { idx, name, depth } => {
                                let p = &r.tree[*idx];
                                let sel = r.tree_file.as_deref() == Some(p.as_str());
                                if list_row(ui, Some(Icon::Changes), name, false, "", sel, 8.0 + *depth as f32 * tree::INDENT + 4.0).on_hover_text(p).clicked() {
                                    acts.push(Act::TreeFile(p.clone()));
                                }
                            }
                        }
                    }
                });
            });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK()).inner_margin(10)).show(ui, |ui| {
                if r.tree_file.is_some() && r.tree_content_for != r.tree_file {
                    loading_hint(ui);
                    return;
                }
                egui::ScrollArea::both().id_salt("tcontent").auto_shrink([false; 2]).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&r.tree_content).monospace().size(12.0)).extend());
                });
            });
        }
    }
    for (sha, file) in want {
        r.ensure_inline(&sha, &file);
    }
}

/// `hay` contains `needle` (given in lower case), ignoring case. Runs for every commit in every
/// frame while the history is filtered, so ASCII text is compared without allocating.
fn contains_ci(hay: &str, needle: &str) -> bool {
    if needle.is_ascii() {
        let (h, n) = (hay.as_bytes(), needle.as_bytes());
        n.is_empty() || h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
    } else {
        hay.to_lowercase().contains(needle)
    }
}

/// Escapes regex characters so `git log -G` looks for the literal text.
fn regex_escape(s: &str) -> String {
    let mut o = String::new();
    for ch in s.chars() {
        if "\\.^$|?*+()[]{}".contains(ch) {
            o.push('\\');
        }
        o.push(ch);
    }
    o
}

/// The small ▾ next to a toolbar button that opens its menu.
fn dropdown_arrow(ui: &mut egui::Ui, enabled: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(14.0, 40.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter_at(rect);
    if resp.hovered() && enabled {
        p.rect_filled(rect.shrink2(Vec2::new(0.0, 6.0)), 3, theme::ROW_HOVER());
    }
    icons::draw(&p, egui::Rect::from_center_size(rect.center() + Vec2::new(0.0, -4.0), Vec2::splat(9.0)), Icon::ChevronDown, if enabled { theme::TEXT_DIM() } else { theme::TEXT_FAINT() });
    resp.on_hover_text(tip)
}

fn short_label(s: &str) -> String {
    if s.chars().count() > 10 { format!("{}…", s.chars().take(9).collect::<String>()) } else { s.to_string() }
}
