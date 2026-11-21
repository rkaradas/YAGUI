//! The main window: menu, toolbar, repository tabs, sidebar, history with details and the
//! local-changes view. Commands are not shown in the layout itself; they appear subtly in
//! tooltips, in the dialog footer and in the activity window.

use std::path::PathBuf;

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, RichText, Sense, Stroke, Vec2};

use super::{Act, App, DetailTab, Repo, View};
use crate::catalog::{self, CATS};
use crate::git::{self, FileStatus, JobState};
use crate::i18n::{self, Lang, c, tr, trs};
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
        ui.painter().rect_filled(rect, 4, Color32::from_rgb(0x3a, 0x3a, 0x3d));
    }
    icons::draw(ui.painter(), rect.shrink(4.0), i, if resp.hovered() { theme::TEXT_STRONG } else { theme::TEXT_DIM });
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
            ui.label(RichText::new(&cmd).monospace().size(11.0).color(theme::TEXT_DIM));
        }
    })
}

fn tool_button(ui: &mut egui::Ui, i: Icon, label: &str, what: &str, cmd: &str, enabled: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(11.5), theme::TEXT);
    let w = (galley.size().x + 16.0).max(46.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 46.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter_at(rect);
    if enabled && resp.hovered() {
        p.rect_filled(rect.shrink2(Vec2::new(1.0, 3.0)), 5, Color32::from_rgb(0x30, 0x30, 0x33));
    }
    let col = if !enabled { theme::TEXT_FAINT } else if resp.hovered() { theme::TEXT_STRONG } else { theme::TEXT };
    let ir = egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 15.0), Vec2::splat(18.0));
    icons::draw(&p, ir, i, col);
    p.text(rect.center_bottom() - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, label, FontId::proportional(11.5), if enabled { theme::TEXT_DIM } else { theme::TEXT_FAINT });
    tip_with_cmd(resp, what, cmd)
}

fn separator_v(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(13.0, 30.0), Sense::hover());
    ui.painter().line_segment([rect.center_top(), rect.center_bottom()], Stroke::new(1.0, theme::LINE));
}

pub(super) fn status_color(c: char) -> Color32 {
    match c {
        'A' | '?' => theme::GREEN,
        'D' => theme::RED,
        'R' | 'C' => theme::ACCENT,
        'U' => theme::ORANGE,
        _ => theme::YELLOW,
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
        p.rect_filled(rect, 4, theme::ROW_SEL);
    } else if resp.hovered() {
        p.rect_filled(rect, 4, theme::ROW_HOVER);
    }
    let mut x = rect.left() + 8.0 + indent;
    if let Some(i) = ic {
        icons::draw(&p, egui::Rect::from_center_size(egui::pos2(x + 7.0, rect.center().y), Vec2::splat(14.0)), i, if selected { theme::TEXT_STRONG } else { theme::TEXT_DIM });
        x += 22.0;
    }
    let font = if strong { FontId::new(13.0, theme::semibold()) } else { FontId::proportional(13.0) };
    let right_w = if right.is_empty() { 0.0 } else { p.layout_no_wrap(right.to_string(), FontId::proportional(11.5), theme::TEXT_DIM).size().x + 16.0 };
    let clip = egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(rect.right() - right_w, rect.bottom()));
    ui.painter_at(clip).text(egui::pos2(x, rect.center().y), Align2::LEFT_CENTER, text, font, if selected || strong { theme::TEXT_STRONG } else { theme::TEXT });
    if !right.is_empty() {
        p.text(egui::pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, right, FontId::proportional(11.5), theme::TEXT_DIM);
    }
    resp
}

/// A collapsible sidebar section header, open state kept in egui memory.
fn section(ui: &mut egui::Ui, id: &str, title: &str, count: usize, default_open: bool) -> bool {
    let key = Id::unique(("sec", id));
    let open = ui.ctx().data_mut(|d| *d.get_temp_mut_or_insert_with(key, || default_open));
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    let p = ui.painter_at(rect);
    let ir = egui::Rect::from_center_size(egui::pos2(rect.left() + 10.0, rect.center().y), Vec2::splat(12.0));
    icons::draw(&p, ir, if open { Icon::ChevronDown } else { Icon::ChevronRight }, theme::TEXT_DIM);
    p.text(egui::pos2(rect.left() + 22.0, rect.center().y), Align2::LEFT_CENTER, title, FontId::new(13.0, theme::semibold()), if resp.hovered() { theme::TEXT_STRONG } else { theme::TEXT });
    if count > 0 {
        p.text(egui::pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(11.5), theme::TEXT_FAINT);
    }
    if resp.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(key, !open));
    }
    open
}

fn pill(ui: &mut egui::Ui, text: &str, fill: Color32, fg: Color32) {
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
// Diff rendering with line numbers

pub(super) fn diff_facts(ui: &mut egui::Ui, pd: &git::ParsedDiff) {
    if pd.facts.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for f in &pd.facts {
            let txt = if i18n::is_de() { f.clone() } else { fact_en(f) };
            pill(ui, &txt, Color32::from_rgb(0x2c, 0x37, 0x46), Color32::from_rgb(0xb4, 0xcf, 0xee));
        }
    });
    ui.add_space(4.0);
}

/// The facts are produced in German by `git::parse_diff`; translate them for English.
fn fact_en(f: &str) -> String {
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

/// Draws diff lines with old/new line numbers (from the hunk headers).
pub(super) fn diff_body(ui: &mut egui::Ui, text: &str) {
    if text.trim().is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new(tr("No textual changes.", "Keine Textänderungen.")).color(theme::TEXT_DIM));
        return;
    }
    let font = FontId::monospace(12.5);
    let w = ui.available_width().max(700.0);
    let num_w = 38.0;
    let (mut old, mut new) = (0u32, 0u32);
    let mut in_hunk = false;
    for line in text.lines().take(8000) {
        if line.starts_with("@@") {
            let (o, n, _) = parse_hunk(line);
            old = o;
            new = n;
            in_hunk = true;
        }
        if !in_hunk {
            continue; // the file header is summarised by diff_facts
        }
        let (bg, fg, on, nn) = if line.starts_with("@@") {
            (theme::DIFF_HUNK, Color32::from_rgb(0x8f, 0xa9, 0xc7), None, None)
        } else if line.starts_with('+') {
            new += 1;
            (theme::DIFF_ADD, Color32::from_rgb(0xc4, 0xe8, 0xc9), None, Some(new - 1))
        } else if line.starts_with('-') {
            old += 1;
            (theme::DIFF_DEL, Color32::from_rgb(0xf0, 0xc0, 0xc0), Some(old - 1), None)
        } else if line.starts_with('\\') {
            (Color32::TRANSPARENT, theme::TEXT_FAINT, None, None)
        } else {
            old += 1;
            new += 1;
            (Color32::TRANSPARENT, theme::TEXT, Some(old - 1), Some(new - 1))
        };
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 18.0), Sense::hover());
        let p = ui.painter();
        if bg != Color32::TRANSPARENT {
            p.rect_filled(rect, 0, bg);
        }
        let numcol = theme::TEXT_FAINT;
        if let Some(o) = on {
            p.text(egui::pos2(rect.left() + num_w - 4.0, rect.center().y), Align2::RIGHT_CENTER, o.to_string(), FontId::monospace(11.0), numcol);
        }
        if let Some(n) = nn {
            p.text(egui::pos2(rect.left() + 2.0 * num_w - 4.0, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), FontId::monospace(11.0), numcol);
        }
        p.line_segment([egui::pos2(rect.left() + 2.0 * num_w + 2.0, rect.top()), egui::pos2(rect.left() + 2.0 * num_w + 2.0, rect.bottom())], Stroke::new(1.0, theme::LINE));
        p.text(egui::pos2(rect.left() + 2.0 * num_w + 10.0, rect.center().y), Align2::LEFT_CENTER, line.replace('\t', "    "), font.clone(), fg);
    }
}

fn parse_hunk(line: &str) -> (u32, u32, String) {
    let pd = git::parse_diff(line);
    pd.hunk_info.into_iter().next().unwrap_or((0, 0, String::new()))
}

pub(super) fn diff_view(ui: &mut egui::Ui, text: &str, id: &str) {
    let pd = git::parse_diff(text);
    diff_facts(ui, &pd);
    egui::ScrollArea::both().id_salt(id).auto_shrink([false; 2]).show(ui, |ui| {
        if !pd.binary {
            diff_body(ui, text);
        }
    });
}

// ---------------------------------------------------------------------------------------------

impl App {
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let has = !self.tabs.is_empty();
        egui::Panel::top("menu").frame(egui::Frame::new().fill(theme::TOOLBAR).inner_margin(egui::Margin::symmetric(6, 2))).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(tr("File", "Datei"), |ui| {
                    menu_item(ui, acts, tr("Open Repository…", "Repository öffnen…"), Act::OpenRepoDialog);
                    menu_cmd(ui, &self.cmds, acts, "clone", Some(tr("Clone Repository…", "Repository klonen…")), vec![]);
                    menu_cmd(ui, &self.cmds, acts, "init", Some(tr("New Repository…", "Neues Repository…")), vec![]);
                    ui.separator();
                    ui.menu_button(tr("Recent", "Zuletzt geöffnet"), |ui| {
                        if self.recent.is_empty() {
                            ui.label(RichText::new(tr("Nothing yet", "Noch nichts")).color(theme::TEXT_DIM));
                        }
                        for r in self.recent.clone() {
                            if ui.button(&r).clicked() {
                                acts.push(Act::OpenRepo(PathBuf::from(&r)));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    menu_item(ui, acts, tr("Configure SSH Keys…", "SSH-Schlüssel einrichten…"), Act::Ssh);
                    menu_item(ui, acts, tr("Accounts…", "Konten…"), Act::Accounts);
                    ui.separator();
                    menu_item(ui, acts, tr("Preferences…", "Einstellungen…"), Act::Prefs);
                    ui.separator();
                    if ui.button(tr("Exit", "Beenden")).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button(tr("Repository", "Repository"), |ui| {
                    ui.add_enabled_ui(has, |ui| {
                        menu_cmd(ui, &self.cmds, acts, "fetch", None, vec![]);
                        menu_cmd(ui, &self.cmds, acts, "pull", None, vec![]);
                        menu_cmd(ui, &self.cmds, acts, "push", None, vec![]);
                        ui.separator();
                        menu_cmd(ui, &self.cmds, acts, "switch_create", None, vec![]);
                        menu_cmd(ui, &self.cmds, acts, "merge", None, vec![]);
                        menu_cmd(ui, &self.cmds, acts, "rebase", None, vec![]);
                        menu_cmd(ui, &self.cmds, acts, "tag_create", None, vec![]);
                        ui.separator();
                        menu_cmd(ui, &self.cmds, acts, "stash_push", None, vec![]);
                        menu_cmd(ui, &self.cmds, acts, "stash_pop", None, vec![]);
                        ui.separator();
                        menu_cmd(ui, &self.cmds, acts, "remote_add", None, vec![]);
                        menu_cmd(ui, &self.cmds, acts, "submodule_add", None, vec![]);
                        menu_cmd(ui, &self.cmds, acts, "worktree_add", None, vec![]);
                        ui.separator();
                        menu_item(ui, acts, tr("Refresh  (F5)", "Neu laden  (F5)"), Act::Refresh);
                        if let Some(r) = self.tabs.get(self.active) {
                            let p = r.path.clone();
                            menu_item(ui, acts, tr("Open in File Explorer   Ctrl+Alt+O", "Im Datei-Explorer öffnen   Strg+Alt+O"), Act::Explorer(p));
                        }
                        menu_item(ui, acts, tr("Open in Console   Ctrl+Alt+T", "In Konsole öffnen   Strg+Alt+T"), Act::Terminal);
                    });
                });
                // Every git command, grouped: the full set stays one click away without
                // taking space in the window.
                ui.menu_button(tr("Commands", "Befehle"), |ui| {
                    menu_item(ui, acts, tr("Find a command…   Ctrl+Shift+P", "Befehl suchen…   Strg+Umschalt+P"), Act::QuickLaunch);
                    menu_item(ui, acts, tr("Browse all commands…", "Alle Befehle durchsuchen…"), Act::Commands(true));
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
                ui.menu_button(tr("View", "Ansicht"), |ui| {
                    ui.add_enabled_ui(has, |ui| {
                        menu_item(ui, acts, tr("Local Changes", "Lokale Änderungen"), Act::View(View::Changes));
                        menu_item(ui, acts, tr("All Commits", "Alle Commits"), Act::View(View::Commits));
                    });
                    menu_item(ui, acts, tr("Git Commands", "Git-Befehle"), Act::Commands(true));
                    ui.separator();
                    menu_item(ui, acts, tr("Activity", "Aktivität"), Act::ToggleActivity);
                    menu_item(ui, acts, tr("Git Console (type commands)", "Git-Konsole (Befehle eintippen)"), Act::ToggleConsole);
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
                ui.menu_button(tr("Help", "Hilfe"), |ui| {
                    ui.label(RichText::new(tr("Hover over any button to see what it does.", "Fahre über einen Knopf, um zu sehen, was er tut.")).color(theme::TEXT_DIM));
                    ui.separator();
                    menu_cmd(ui, &self.cmds, acts, "version", None, vec![]);
                    menu_cmd(ui, &self.cmds, acts, "config_name", None, vec![]);
                    menu_cmd(ui, &self.cmds, acts, "config_email", None, vec![]);
                });
            });
        });
    }

    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let has = !self.tabs.is_empty();
        let (rname, head) = self.tabs.get(self.active).map(|r| (r.name.clone(), r.head.clone())).unwrap_or_default();
        let locals: Vec<String> = self.tabs.get(self.active).map(|r| r.local.iter().map(|b| b.name.clone()).collect()).unwrap_or_default();
        egui::Panel::top("toolbar").exact_size(52.0).frame(egui::Frame::new().fill(theme::TOOLBAR).inner_margin(egui::Margin::symmetric(8, 3))).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if tool_button(ui, Icon::Search, tr("Quick Launch", "Schnellstart"), tr("Find any Git command (Ctrl+Shift+P)", "Jeden Git-Befehl finden (Strg+Umschalt+P)"), "", true).clicked() {
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
                if tool_button(ui, Icon::Pop, "Pop", tr("Bring back the latest stash", "Letzten Stash zurückholen"), "git stash pop", has).clicked() {
                    acts.push(Act::Open("stash_pop", vec![]));
                }

                // Centre: repository and current branch – click to switch branch.
                let right_w = 4.0 * 62.0;
                let centre_w = 250.0;
                let pad = ((ui.available_width() - right_w - centre_w - 120.0) / 2.0).max(8.0);
                ui.add_space(pad);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(centre_w, 38.0), if has { Sense::click() } else { Sense::hover() });
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 5, if resp.hovered() && has { Color32::from_rgb(0x33, 0x33, 0x36) } else { Color32::from_rgb(0x2a, 0x2a, 0x2c) });
                if has {
                    p.text(rect.center_top() + Vec2::new(0.0, 11.0), Align2::CENTER_CENTER, &rname, FontId::new(13.0, theme::semibold()), theme::TEXT_STRONG);
                    let g = p.layout_no_wrap(head.clone(), FontId::proportional(11.5), theme::TEXT_DIM);
                    let bx = rect.center().x - g.size().x / 2.0 + 7.0;
                    icons::draw(&p, egui::Rect::from_center_size(egui::pos2(bx - 10.0, rect.bottom() - 11.0), Vec2::splat(11.0)), Icon::Branch, theme::TEXT_DIM);
                    p.galley(egui::pos2(bx, rect.bottom() - 11.0 - g.size().y / 2.0), g, theme::TEXT_DIM);
                    icons::draw(&p, egui::Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(10.0)), Icon::ChevronDown, theme::TEXT_DIM);
                } else {
                    p.text(rect.center(), Align2::CENTER_CENTER, tr("No repository", "Kein Repository"), FontId::proportional(12.5), theme::TEXT_DIM);
                }
                let resp = resp.on_hover_text(tr("Current branch – click to switch", "Aktueller Branch – klicken zum Wechseln"));
                if has {
                    egui::Popup::menu(&resp).show(|ui| {
                        ui.set_min_width(220.0);
                        ui.label(RichText::new(tr("Switch to", "Wechseln zu")).size(11.5).color(theme::TEXT_DIM));
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
                    acts.push(Act::Open("switch_create", vec![]));
                }
                if tool_button(ui, Icon::Merge, "Merge", tr("Merge another branch into this one", "Anderen Branch zusammenführen"), "git merge", has).clicked() {
                    acts.push(Act::Open("merge", vec![]));
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if tool_button(ui, Icon::Activity, tr("Activity", "Aktivität"), tr("Commands run so far and their output", "Ausgeführte Befehle und ihre Ausgabe"), "", true).clicked() {
                        acts.push(Act::ToggleActivity);
                    }
                    let cmds_on = self.commands_view;
                    let b = tool_button(ui, Icon::Cube, tr("Commands", "Befehle"), tr("All Git commands, explained and runnable", "Alle Git-Befehle, erklärt und ausführbar"), "", true);
                    if cmds_on {
                        ui.painter().line_segment([b.rect.left_bottom() + Vec2::new(14.0, -2.0), b.rect.right_bottom() + Vec2::new(-14.0, -2.0)], egui::Stroke::new(2.0, theme::ACCENT));
                    }
                    if b.clicked() {
                        acts.push(Act::Commands(!cmds_on));
                    }
                    separator_v(ui);
                    if tool_button(ui, Icon::Terminal, tr("Console", "Konsole"), tr("Open a terminal in this repository (Ctrl+Alt+T)", "Terminal in diesem Repository öffnen (Strg+Alt+T)"), "", has).clicked() {
                        acts.push(Act::Terminal);
                    }
                    let b = tool_button(ui, Icon::Folder, tr("Open in", "Öffnen in"), tr("Open the folder in another program", "Ordner in einem anderen Programm öffnen"), "", has);
                    if has {
                        egui::Popup::menu(&b).show(|ui| self.open_in_menu(ui));
                    }
                });
            });
        });
    }

    pub(super) fn tab_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        egui::Panel::top("tabs").exact_size(26.0).frame(egui::Frame::new().fill(theme::TABS)).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let n = self.tabs.len().max(1);
                let w = ((ui.available_width() - 28.0) / n as f32).clamp(110.0, 320.0);
                for (i, t) in self.tabs.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
                    let active = i == self.active;
                    let p = ui.painter_at(rect);
                    let bg = if active { theme::PANEL } else if resp.hovered() { Color32::from_rgb(0x22, 0x22, 0x22) } else { theme::TABS };
                    p.rect_filled(rect, 0, bg);
                    p.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, Color32::from_rgb(0x2c, 0x2c, 0x2c)));
                    p.text(rect.center(), Align2::CENTER_CENTER, &t.name, FontId::proportional(12.5), if active { theme::TEXT_STRONG } else { theme::TEXT_DIM });
                    let close = egui::Rect::from_center_size(rect.right_center() - Vec2::new(14.0, 0.0), Vec2::splat(14.0));
                    let hover_close = resp.hover_pos().map(|p| close.contains(p)).unwrap_or(false);
                    if resp.hovered() {
                        if hover_close {
                            p.rect_filled(close.expand(2.0), 3, Color32::from_rgb(0x44, 0x44, 0x48));
                        }
                        icons::draw(&p, close.shrink(2.0), Icon::Close, theme::TEXT_DIM);
                    }
                    if resp.clicked() {
                        acts.push(if hover_close { Act::CloseTab(i) } else { Act::SwitchTab(i) });
                    }
                    resp.on_hover_text(t.path.display().to_string());
                }
                ui.add_space(4.0);
                if icon_button(ui, Icon::Plus, tr("Open another repository", "Weiteres Repository öffnen")).clicked() {
                    acts.push(Act::OpenRepoDialog);
                }
            });
        });
    }

    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let now = ui.ctx().input(|i| i.time);
        egui::Panel::bottom("statusbar").exact_size(24.0).frame(egui::Frame::new().fill(theme::TOOLBAR).inner_margin(egui::Margin::symmetric(10, 0))).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let running: Vec<&git::Job> = self.jobs.iter().filter(|j| j.state() == JobState::Running).collect();
                if let Some(j) = running.last() {
                    ui.spinner();
                    ui.label(RichText::new(&j.title).size(12.0));
                    ui.label(RichText::new(j.display.lines().next().unwrap_or("")).monospace().size(11.0).color(theme::TEXT_FAINT));
                } else if let Some(j) = self.jobs.iter().rev().find(|j| !j.automatic) {
                    let ok = matches!(j.state(), JobState::Done(0) | JobState::Started);
                    icon(ui, if ok { Icon::Check } else { Icon::Warning }, 13.0, if ok { theme::GREEN } else { theme::RED });
                    ui.label(RichText::new(&j.title).size(12.0).color(theme::TEXT_DIM));
                    ui.label(RichText::new(j.display.lines().next().unwrap_or("")).monospace().size(11.0).color(theme::TEXT_FAINT));
                }
                if let Some((msg, t)) = &self.toast {
                    if now - t < 6.0 {
                        ui.label(RichText::new(msg).size(12.0).color(theme::YELLOW));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    for (l, name) in [(Lang::De, "DE"), (Lang::En, "EN")] {
                        let sel = self.lang == l;
                        let r = ui.add(egui::Button::new(RichText::new(name).size(11.0).color(if sel { theme::TEXT_STRONG } else { theme::TEXT_FAINT })).frame(false));
                        if r.on_hover_text(tr("Interface language", "Sprache der Oberfläche")).clicked() && !sel {
                            acts.push(Act::Lang(l));
                        }
                    }
                    ui.add_space(8.0);
                    let label = format!("{} ({})", tr("Activity", "Aktivität"), self.jobs.iter().filter(|j| !j.automatic).count());
                    if ui.add(egui::Button::new(RichText::new(label).size(11.5).color(theme::TEXT_DIM)).frame(false)).clicked() {
                        acts.push(Act::ToggleActivity);
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
        egui::Frame::new().fill(Color32::from_rgb(0x4a, 0x25, 0x27)).inner_margin(egui::Margin::symmetric(12, 7)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icon(ui, Icon::Warning, 15.0, Color32::from_rgb(0xf2, 0x9a, 0x9a));
                ui.label(RichText::new(trs(format!("{title} failed"), format!("{title} fehlgeschlagen"))).color(theme::TEXT_STRONG));
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
            bar(ui, Color32::from_rgb(0x4a, 0x25, 0x27), &mut |ui| {
                ui.label(RichText::new(&e).color(theme::TEXT_STRONG));
            });
        }
        if let Some(lock) = r.locked {
            bar(ui, Color32::from_rgb(0x3a, 0x30, 0x22), &mut |ui| {
                icon(ui, Icon::Lock, 14.0, theme::YELLOW);
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
        bar(ui, Color32::from_rgb(0x3d, 0x33, 0x1c), &mut |ui| {
            icon(ui, Icon::Warning, 14.0, theme::YELLOW);
            ui.label(RichText::new(text).color(theme::TEXT_STRONG));
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
        let r = &mut self.tabs[self.active];
        egui::Panel::left("sidebar").resizable(true).default_size(260.0).min_size(200.0).frame(egui::Frame::new().fill(theme::SIDEBAR).inner_margin(egui::Margin { left: 6, right: 6, top: 8, bottom: 6 })).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(&r.name).font(FontId::new(14.0, theme::semibold())).color(theme::TEXT_STRONG));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
                // Branches
                if section(ui, "branches", tr("Branches", "Branches"), r.local.len(), true) {
                    for b in r.local.iter().filter(|b| matches(&b.name)) {
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
                        let resp = list_row(ui, Some(if b.is_head { Icon::Check } else { Icon::Branch }), &b.name, b.is_head, right.trim(), false, 8.0);
                        let resp = resp.on_hover_text(trs(
                            format!("{}{}\nDouble-click to switch · right-click for actions", b.name, if b.upstream.is_empty() { String::new() } else { format!(" → {}", b.upstream) }),
                            format!("{}{}\nDoppelklick zum Wechseln · Rechtsklick für Aktionen", b.name, if b.upstream.is_empty() { String::new() } else { format!(" → {}", b.upstream) }),
                        ));
                        if resp.double_clicked() && !b.is_head {
                            acts.push(Act::Open("switch", vec![("branch", b.name.clone())]));
                        } else if resp.clicked() {
                            acts.push(Act::JumpToRef(b.name.clone()));
                        }
                        let name = b.name.clone();
                        let up = b.upstream.clone();
                        let head = head.clone();
                        let cmds = &self.cmds;
                        resp.context_menu(|ui| branch_menu(ui, cmds, acts, &name, &up, &head, false));
                    }
                }
                ui.add_space(2.0);
                if section(ui, "remotes", tr("Remotes", "Remotes"), r.remotes.len(), true) {
                    for (rn, url) in &r.remotes {
                        let key = Id::unique(("remote-open", rn));
                        let open = ui.ctx().data_mut(|d| *d.get_temp_mut_or_insert_with(key, || false));
                        let resp = list_row(ui, Some(Icon::Cloud), rn, false, "", false, 8.0).on_hover_text(url);
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
                            ui.separator();
                            menu_cmd(ui, cmds, acts, "remote_remove", None, vec![("remote", rn2.clone())]);
                        });
                        if open {
                            for b in r.remote_branches.iter().filter(|b| b.name.starts_with(&format!("{rn}/")) && matches(&b.name)) {
                                let short = b.name.trim_start_matches(&format!("{rn}/")).to_string();
                                let resp = list_row(ui, Some(Icon::Branch), &short, false, "", false, 26.0);
                                if resp.double_clicked() {
                                    acts.push(Act::Open("checkout_track", vec![("branch", b.name.clone())]));
                                } else if resp.clicked() {
                                    acts.push(Act::JumpToRef(b.name.clone()));
                                }
                                let name = b.name.clone();
                                let head = head.clone();
                                let cmds = &self.cmds;
                                resp.context_menu(|ui| branch_menu(ui, cmds, acts, &name, "", &head, true));
                            }
                        }
                    }
                }
                ui.add_space(2.0);
                if section(ui, "tags", tr("Tags", "Tags"), r.tags.len(), false) {
                    for t in r.tags.iter().rev().filter(|t| matches(&t.name)) {
                        let resp = list_row(ui, Some(Icon::Tag), &t.name, false, "", false, 8.0);
                        if resp.clicked() {
                            acts.push(Act::JumpToRef(t.name.clone()));
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
                        let resp = list_row(ui, Some(Icon::Stash), msg, false, "", false, 8.0).on_hover_text(name);
                        let name = name.clone();
                        let cmds = &self.cmds;
                        if resp.double_clicked() {
                            acts.push(Act::Open("stash_apply", vec![("stash", name.clone())]));
                        }
                        resp.context_menu(|ui| {
                            menu_cmd(ui, cmds, acts, "stash_apply", None, vec![("stash", name.clone())]);
                            menu_cmd(ui, cmds, acts, "stash_pop", None, vec![("stash", name.clone())]);
                            menu_cmd(ui, cmds, acts, "stash_branch", None, vec![("stash", name.clone())]);
                            ui.separator();
                            menu_cmd(ui, cmds, acts, "stash_drop", None, vec![("stash", name.clone())]);
                        });
                    }
                }
                ui.add_space(2.0);
                if section(ui, "submodules", tr("Submodules", "Submodule"), r.submodules.len(), false) {
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
    }

    // -----------------------------------------------------------------------------------------
    // History

    pub(super) fn commits_view(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let cmds = &self.cmds;
        let r = &mut self.tabs[self.active];
        egui::Panel::bottom("detail").resizable(true).default_size(340.0).min_size(140.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(0)).show(ui, |ui| {
            detail_pane(ui, r, acts);
        });

        egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 4 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                icon(ui, Icon::Search, 14.0, theme::TEXT_DIM);
                ui.add(egui::TextEdit::singleline(&mut r.commit_search).hint_text(tr("Search message, author or SHA", "Beschreibung, Autor oder SHA suchen")).desired_width(300.0).frame(egui::Frame::NONE));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let before = (r.log_opts.hide_stashes, r.log_opts.reflog);
                    ui.menu_button(RichText::new(tr("View options", "Anzeige")).size(12.0).color(theme::TEXT_DIM), |ui| {
                        ui.checkbox(&mut r.log_opts.hide_stashes, tr("Hide stashes", "Stashes ausblenden"));
                        ui.checkbox(&mut r.log_opts.reflog, tr("Show reflog commits", "Reflog-Commits zeigen"));
                        ui.separator();
                        let tips = git::log_tip_args(r.log_opts).join(" ");
                        ui.label(RichText::new(format!("git log --date-order {tips}")).monospace().size(10.5).color(theme::TEXT_FAINT));
                    });
                    if before != (r.log_opts.hide_stashes, r.log_opts.reflog) {
                        r.reload(git::RELOAD_REFS);
                    }
                    ui.label(RichText::new(trs(format!("{} commits", r.commits.len()), format!("{} Commits", r.commits.len()))).size(12.0).color(theme::TEXT_FAINT));
                });
            });
        });
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(rect, 0, theme::LINE);

        let q = r.commit_search.to_lowercase();
        let visible: Vec<usize> = if q.is_empty() {
            (0..r.commits.len()).collect()
        } else {
            r.commits.iter().enumerate().filter(|(_, c)| c.subject.to_lowercase().contains(&q) || c.author.to_lowercase().contains(&q) || c.sha.starts_with(&q)).map(|(i, _)| i).collect()
        };
        let row_h = 24.0;
        let mut scroll = egui::ScrollArea::vertical().id_salt("commits").auto_shrink([false; 2]);
        if let Some(i) = r.scroll_to.take() {
            if let Some(pos) = visible.iter().position(|v| *v == i) {
                scroll = scroll.vertical_scroll_offset((pos as f32 * row_h - 120.0).max(0.0));
            }
        }
        let graph_on = q.is_empty();
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
                    p.rect_filled(rect, 0, theme::ROW_SEL);
                } else if resp.hovered() {
                    p.rect_filled(rect, 0, theme::ROW_HOVER);
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
                            p.circle_filled(egui::pos2(x(row.lane), mid), 2.0, if sel { theme::ROW_SEL } else { theme::PANEL });
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
                    let (label, ic, fill, stroke, fg) = if let Some(b) = rf.strip_prefix("HEAD -> ") {
                        (b.to_string(), Some(Icon::Check), Color32::from_rgb(0x3a, 0x3a, 0x3d), theme::ORANGE, theme::TEXT_STRONG)
                    } else if let Some(t) = rf.strip_prefix("tag: ") {
                        (t.to_string(), Some(Icon::Tag), Color32::from_rgb(0x3a, 0x34, 0x22), Color32::from_rgb(0x6a, 0x5a, 0x2a), theme::YELLOW)
                    } else if rf == "HEAD" {
                        ("HEAD".to_string(), None, Color32::from_rgb(0x3a, 0x3a, 0x3d), theme::ORANGE, theme::TEXT_STRONG)
                    } else if r.local.iter().any(|b| &b.name == rf) {
                        (rf.clone(), None, Color32::from_rgb(0x2b, 0x3d, 0x55), Color32::from_rgb(0x3e, 0x5a, 0x80), theme::TEXT_STRONG)
                    } else {
                        (rf.clone(), Some(Icon::Cloud), Color32::from_rgb(0x33, 0x33, 0x35), Color32::from_rgb(0x48, 0x48, 0x4c), theme::TEXT_DIM)
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
                ui.painter_at(subj_rect).text(egui::pos2(x + 2.0, rect.center().y), Align2::LEFT_CENTER, &c.subject, FontId::proportional(13.0), theme::TEXT);
                let ax = right - date_w - sha_w - author_w;
                let av = egui::Rect::from_center_size(egui::pos2(ax + 8.0, rect.center().y), Vec2::splat(16.0));
                p.rect_filled(av, 3, theme::avatar_color(&c.author));
                p.text(av.center(), Align2::CENTER_CENTER, initials(&c.author), FontId::new(8.5, theme::semibold()), Color32::WHITE);
                ui.painter_at(egui::Rect::from_min_max(egui::pos2(ax + 20.0, rect.top()), egui::pos2(ax + author_w - 6.0, rect.bottom())))
                    .text(egui::pos2(ax + 22.0, rect.center().y), Align2::LEFT_CENTER, &c.author, FontId::proportional(12.5), theme::TEXT);
                p.text(egui::pos2(right - date_w - sha_w, rect.center().y), Align2::LEFT_CENTER, &c.short, FontId::monospace(12.0), theme::TEXT_DIM);
                p.text(egui::pos2(right - date_w, rect.center().y), Align2::LEFT_CENTER, &c.date, FontId::proportional(12.5), theme::TEXT_DIM);
                if resp.clicked() {
                    acts.push(Act::SelectCommit(i));
                }
                let sha = c.sha.clone();
                let short = c.short.clone();
                let subject = c.subject.clone();
                let parents = c.parents.len();
                let head = head.clone();
                resp.context_menu(|ui| commit_menu(ui, cmds, acts, &sha, &short, &subject, parents, &head));
            }
        });
    }

    // -----------------------------------------------------------------------------------------
    // Local changes

    pub(super) fn changes_view(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>, ctx: &egui::Context) {
        let mut hunk: Option<(String, bool, bool)> = None;
        let mut do_commit = false;
        {
            let cmds = &self.cmds;
            let r = &mut self.tabs[self.active];
            let unstaged: Vec<FileStatus> = r.status.iter().filter(|f| f.unstaged()).cloned().collect();
            let staged: Vec<FileStatus> = r.status.iter().filter(|f| f.staged()).cloned().collect();
            let has_head = !r.commits.is_empty();

            egui::Panel::left("changes_files").resizable(true).default_size(380.0).min_size(280.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(0)).show(ui, |ui| {
                // Commit composer at the bottom
                egui::Panel::bottom("commit_box").resizable(false).frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut r.commit_msg).hint_text(tr("Commit subject", "Commit-Titel")).desired_width(f32::INFINITY));
                    ui.add_space(2.0);
                    ui.add(egui::TextEdit::multiline(&mut r.commit_desc).hint_text(tr("Description (optional)", "Beschreibung (optional)")).desired_rows(3).desired_width(f32::INFINITY));
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
                    let btn = egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(if can { theme::ACCENT_DARK } else { Color32::from_rgb(0x36, 0x36, 0x39) }).min_size(Vec2::new(ui.available_width(), 28.0));
                    let resp = ui.add_enabled(can, btn).on_disabled_hover_text(tr("Stage files and enter a subject first", "Erst Dateien vormerken und einen Titel eingeben"));
                    if resp.clicked() {
                        do_commit = true;
                    }
                    // Subtle: what will run
                    let preview = commit_preview(&r.commit_msg, &r.commit_desc, r.amend);
                    ui.add_space(3.0);
                    ui.label(RichText::new(preview).monospace().size(10.5).color(theme::TEXT_FAINT));
                });

                egui::ScrollArea::vertical().id_salt("files").auto_shrink([false; 2]).show(ui, |ui| {
                    ui.add_space(6.0);
                    file_section_header(ui, tr("Unstaged", "Nicht vorgemerkt"), unstaged.len(), if unstaged.is_empty() { None } else { Some((tr("Stage All", "Alle vormerken"), "git add -A")) }, || {
                        acts.push(Act::Quick { title: tr("Stage all", "Alles vormerken").into(), args: vec!["add".into(), "-A".into()], explain: String::new() });
                    });
                    if unstaged.is_empty() {
                        empty_hint(ui, tr("No changes", "Keine Änderungen"));
                    }
                    for f in &unstaged {
                        file_row(ui, cmds, acts, f, false, r.sel_file.as_ref() == Some(&(f.path.clone(), false)), has_head);
                    }
                    ui.add_space(10.0);
                    let unstage_args: Vec<String> = if has_head { vec!["reset".into(), "-q".into()] } else { vec!["rm".into(), "--cached".into(), "-r".into(), "-q".into(), ".".into()] };
                    file_section_header(ui, tr("Staged", "Vorgemerkt"), staged.len(), if staged.is_empty() { None } else { Some((tr("Unstage All", "Alle zurücknehmen"), "git reset")) }, || {
                        acts.push(Act::Quick { title: tr("Unstage all", "Alles zurücknehmen").into(), args: unstage_args.clone(), explain: String::new() });
                    });
                    if staged.is_empty() {
                        empty_hint(ui, tr("Nothing staged yet", "Noch nichts vorgemerkt"));
                    }
                    for f in &staged {
                        file_row(ui, cmds, acts, f, true, r.sel_file.as_ref() == Some(&(f.path.clone(), true)), has_head);
                    }
                });
            });

            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(0)).show(ui, |ui| {
                let Some((path, staged_sel)) = r.sel_file.clone() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new(tr("Select a file to see its changes", "Wähle eine Datei, um ihre Änderungen zu sehen")).color(theme::TEXT_DIM));
                    });
                    return;
                };
                let pd = git::parse_diff(&r.file_diff);
                let untracked = r.status.iter().any(|f| f.path == path && f.index == '?');
                let code = r.status.iter().find(|f| f.path == path).map(|f| if staged_sel { f.index } else if f.index == '?' { '?' } else { f.worktree }).unwrap_or('M');
                // File header bar
                egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(12, 7)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        status_badge(ui, code);
                        let (file, dir) = split_path(&path);
                        ui.label(RichText::new(file).font(FontId::new(13.0, theme::semibold())).color(theme::TEXT_STRONG));
                        if !dir.is_empty() {
                            ui.label(RichText::new(dir).size(12.0).color(theme::TEXT_DIM));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(if staged_sel { tr("Staged", "Vorgemerkt") } else { tr("Unstaged", "Nicht vorgemerkt") }).size(11.5).color(theme::TEXT_FAINT));
                        });
                    });
                });
                egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 0 }).show(ui, |ui| {
                    diff_facts(ui, &pd);
                });
                egui::ScrollArea::both().id_salt("wdiff").auto_shrink([false; 2]).show(ui, |ui| {
                    if pd.binary {
                        return;
                    }
                    if pd.hunks.is_empty() {
                        diff_body(ui, &r.file_diff);
                    }
                    for (hi, h) in pd.hunks.iter().enumerate() {
                        // hunk toolbar (actions at the right of the hunk header)
                        if !untracked {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                let (_, _, ctxs) = pd.hunk_info.get(hi).cloned().unwrap_or_default();
                                ui.label(RichText::new(if ctxs.is_empty() { trs(format!("Hunk {}", hi + 1), format!("Abschnitt {}", hi + 1)) } else { ctxs }).size(11.5).color(theme::TEXT_FAINT));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.add_space(8.0);
                                    let patch = format!("{}{}", pd.header, h);
                                    if staged_sel {
                                        let b = ui.small_button(tr("Unstage Hunk", "Abschnitt zurücknehmen"));
                                        if tip_with_cmd(b, tr("Remove only this hunk from staging", "Nur diesen Abschnitt aus der Vormerkung nehmen"), "git apply --cached --reverse").clicked() {
                                            hunk = Some((patch.clone(), true, true));
                                        }
                                    } else {
                                        let b = ui.small_button(tr("Discard Hunk", "Abschnitt verwerfen"));
                                        if tip_with_cmd(b, tr("Throw away only this change in the file. Can't be undone.", "Verwirft nur diese Änderung in der Datei. Nicht rückgängig zu machen."), "git apply --reverse").clicked() {
                                            hunk = Some((patch.clone(), true, false));
                                        }
                                        let b = ui.small_button(tr("Stage Hunk", "Abschnitt vormerken"));
                                        if tip_with_cmd(b, tr("Stage only this hunk", "Nur diesen Abschnitt vormerken"), "git apply --cached").clicked() {
                                            hunk = Some((patch.clone(), false, true));
                                        }
                                    }
                                });
                            });
                        }
                        diff_body(ui, h);
                        ui.add_space(10.0);
                    }
                });
            });
        }
        if let Some((patch, reverse, cached)) = hunk {
            let mut args = vec!["apply".to_string()];
            if cached {
                args.push("--cached".into());
            }
            if reverse {
                args.push("--reverse".into());
            }
            args.push("--unidiff-zero".into());
            args.push("-".into());
            let title = match (reverse, cached) {
                (false, true) => tr("Stage hunk", "Abschnitt vormerken"),
                (true, true) => tr("Unstage hunk", "Abschnitt zurücknehmen"),
                _ => tr("Discard hunk", "Abschnitt verwerfen"),
            };
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
            self.quick(tr("Commit", "Commit").into(), args, ctx);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Welcome

    pub(super) fn welcome(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let w = 560.0f32;
        ui.add_space((ui.available_height() * 0.16).max(30.0));
        ui.vertical_centered(|ui| {
            ui.set_max_width(w);
            ui.label(RichText::new("YAGUI").font(FontId::new(30.0, theme::semibold())).color(theme::TEXT_STRONG));
            ui.label(RichText::new(tr("A friendly Git client", "Ein freundlicher Git-Client")).size(14.0).color(theme::TEXT_DIM));
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
                p.rect_filled(rect, 6, if resp.hovered() { Color32::from_rgb(0x36, 0x36, 0x39) } else { Color32::from_rgb(0x30, 0x30, 0x32) });
                p.rect_stroke(rect, 6, Stroke::new(1.0, if resp.hovered() { theme::ACCENT_DARK } else { theme::LINE }), egui::StrokeKind::Inside);
                icons::draw(&p, egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 30.0), Vec2::splat(24.0)), ic, theme::ACCENT);
                p.text(rect.center() + Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, title, FontId::new(14.0, theme::semibold()), theme::TEXT_STRONG);
                p.text(rect.center() + Vec2::new(0.0, 32.0), Align2::CENTER_CENTER, desc, FontId::proportional(11.5), theme::TEXT_DIM);
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
                        ui.label(RichText::new("·").color(theme::TEXT_FAINT));
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
                ui.label(RichText::new(tr("Recent repositories", "Zuletzt geöffnet")).font(FontId::new(12.5, theme::semibold())).color(theme::TEXT_DIM));
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
        ui.label(RichText::new(text).size(12.0).color(theme::TEXT_FAINT));
    });
}

fn file_section_header(ui: &mut egui::Ui, title: &str, count: usize, action: Option<(&str, &str)>, mut on_click: impl FnMut()) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(title).font(FontId::new(12.5, theme::semibold())).color(theme::TEXT));
        ui.label(RichText::new(count.to_string()).size(11.5).color(theme::TEXT_FAINT));
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

fn file_row(ui: &mut egui::Ui, cmds: &[catalog::Cmd], acts: &mut Vec<Act>, f: &FileStatus, staged: bool, selected: bool, has_head: bool) {
    let code = if f.conflicted() { 'U' } else if staged { f.index } else if f.index == '?' { '?' } else { f.worktree };
    let h = 24.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect, 0, theme::ROW_SEL);
    } else if resp.hovered() {
        p.rect_filled(rect, 0, theme::ROW_HOVER);
    }
    // status badge
    let br = egui::Rect::from_center_size(egui::pos2(rect.left() + 22.0, rect.center().y), Vec2::splat(13.0));
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
    let x = rect.left() + 36.0;
    let action_w = if resp.hovered() || selected { 30.0 } else { 0.0 };
    let clip = egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(rect.right() - 8.0 - action_w, rect.bottom()));
    let pc = ui.painter_at(clip);
    let g = pc.layout_no_wrap(file.to_string(), FontId::proportional(13.0), theme::TEXT);
    let gw = g.size().x;
    pc.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, theme::TEXT);
    if !dir.is_empty() {
        pc.text(egui::pos2(x + gw + 8.0, rect.center().y), Align2::LEFT_CENTER, dir, FontId::proportional(11.5), theme::TEXT_FAINT);
    }
    // hover action: stage / unstage
    let mut action_clicked = false;
    if action_w > 0.0 {
        let ar = egui::Rect::from_center_size(egui::pos2(rect.right() - 20.0, rect.center().y), Vec2::new(22.0, 18.0));
        let hovered = resp.hover_pos().map(|p| ar.contains(p)).unwrap_or(false);
        p.rect_filled(ar, 4, if hovered { Color32::from_rgb(0x4a, 0x4a, 0x4f) } else { Color32::from_rgb(0x3a, 0x3a, 0x3d) });
        icons::draw(&p, ar.shrink(4.0), if staged { Icon::Close } else { Icon::Plus }, theme::TEXT_STRONG);
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

fn branch_menu(ui: &mut egui::Ui, cmds: &[catalog::Cmd], acts: &mut Vec<Act>, name: &str, upstream: &str, head: &str, remote: bool) {
    let n = name.to_string();
    if remote {
        menu_cmd(ui, cmds, acts, "checkout_track", Some(&trs(format!("Checkout {n}…"), format!("{n} auschecken…"))), vec![("branch", n.clone())]);
        ui.separator();
        menu_cmd(ui, cmds, acts, "merge", Some(&trs(format!("Merge {n} into {head}…"), format!("{n} in {head} zusammenführen…"))), vec![("branch", n.clone())]);
        menu_cmd(ui, cmds, acts, "rebase", Some(&trs(format!("Rebase {head} on {n}…"), format!("{head} auf {n} neu aufsetzen…"))), vec![("branch", n.clone())]);
        menu_cmd(ui, cmds, acts, "branch_create", Some(tr("New branch from here…", "Neuer Branch von hier…")), vec![("commit", n.clone())]);
        ui.separator();
        if let Some((r, b)) = n.split_once('/') {
            menu_cmd(ui, cmds, acts, "push_delete", Some(&trs(format!("Delete {n}…"), format!("{n} löschen…"))), vec![("remote", r.to_string()), ("branch", b.to_string())]);
        }
    } else {
        let is_head = n == head;
        if !is_head {
            menu_cmd(ui, cmds, acts, "switch", Some(&trs(format!("Checkout {n}"), format!("{n} auschecken"))), vec![("branch", n.clone())]);
            ui.separator();
            menu_cmd(ui, cmds, acts, "merge", Some(&trs(format!("Merge {n} into {head}…"), format!("{n} in {head} zusammenführen…"))), vec![("branch", n.clone())]);
            menu_cmd(ui, cmds, acts, "rebase", Some(&trs(format!("Rebase {head} on {n}…"), format!("{head} auf {n} neu aufsetzen…"))), vec![("branch", n.clone())]);
            ui.separator();
        }
        menu_cmd(ui, cmds, acts, "push", Some(&trs(format!("Push {n}…"), format!("{n} pushen…"))), vec![("branch", n.clone())]);
        if !upstream.is_empty() && is_head {
            menu_cmd(ui, cmds, acts, "pull", Some(&trs(format!("Pull from {upstream}…"), format!("Pull von {upstream}…"))), vec![]);
        }
        menu_cmd(ui, cmds, acts, "branch_upstream", None, vec![("branch", n.clone())]);
        ui.separator();
        menu_cmd(ui, cmds, acts, "branch_create", Some(tr("New branch from here…", "Neuer Branch von hier…")), vec![("commit", n.clone())]);
        menu_cmd(ui, cmds, acts, "tag_create", Some(tr("New tag here…", "Neuer Tag hier…")), vec![("commit", n.clone())]);
        menu_cmd(ui, cmds, acts, "branch_rename", Some(tr("Rename…", "Umbenennen…")), vec![("branch", n.clone())]);
        if !is_head {
            menu_cmd(ui, cmds, acts, "branch_delete", Some(tr("Delete…", "Löschen…")), vec![("branch", n.clone())]);
        }
    }
    ui.separator();
    menu_item(ui, acts, tr("Copy name", "Namen kopieren"), Act::Copy(n));
}

#[allow(clippy::too_many_arguments)]
fn commit_menu(ui: &mut egui::Ui, cmds: &[catalog::Cmd], acts: &mut Vec<Act>, sha: &str, short: &str, subject: &str, parents: usize, head: &str) {
    let s = sha.to_string();
    menu_cmd(ui, cmds, acts, "checkout", Some(&trs(format!("Checkout {short}…"), format!("{short} auschecken…"))), vec![("branch", s.clone())]);
    menu_cmd(ui, cmds, acts, "branch_create", Some(tr("New branch here…", "Neuer Branch hier…")), vec![("commit", s.clone())]);
    menu_cmd(ui, cmds, acts, "tag_create", Some(tr("New tag here…", "Neuer Tag hier…")), vec![("commit", s.clone())]);
    ui.separator();
    menu_cmd(ui, cmds, acts, "cherry_pick", Some(tr("Cherry-pick…", "Cherry-Pick…")), vec![("commits", s.clone())]);
    let mut rv: Vec<(&'static str, String)> = vec![("commits", s.clone())];
    if parents > 1 {
        rv.push(("parent", "1".into()));
    }
    menu_cmd(ui, cmds, acts, "revert", Some(tr("Revert…", "Revert…")), rv);
    menu_cmd(ui, cmds, acts, "reset", Some(&trs(format!("Reset {head} to here…"), format!("{head} hierher zurücksetzen…"))), vec![("commit", s.clone())]);
    menu_item(ui, acts, &trs(format!("Interactive rebase {head} from here…"), format!("Interaktiver Rebase ab hier…")), Act::Rebase(s.clone()));
    menu_cmd(ui, cmds, acts, "rebase", Some(&trs(format!("Rebase {head} onto here…"), format!("{head} hierauf neu aufsetzen…"))), vec![("branch", s.clone())]);
    menu_cmd(ui, cmds, acts, "merge", Some(&trs(format!("Merge into {head}…"), format!("In {head} zusammenführen…"))), vec![("branch", s.clone())]);
    ui.separator();
    menu_cmd(ui, cmds, acts, "format_patch", Some(tr("Save as patch…", "Als Patch speichern…")), vec![("range", format!("-1 {short}"))]);
    menu_cmd(ui, cmds, acts, "diff", Some(tr("Compare with working tree…", "Mit Arbeitsordner vergleichen…")), vec![("a", s.clone())]);
    menu_cmd(ui, cmds, acts, "notes_add", Some(tr("Add note…", "Notiz hinzufügen…")), vec![("commit", s.clone())]);
    ui.separator();
    menu_item(ui, acts, tr("Copy SHA", "SHA kopieren"), Act::Copy(s.clone()));
    menu_item(ui, acts, tr("Copy message", "Beschreibung kopieren"), Act::Copy(subject.to_string()));
}

fn detail_pane(ui: &mut egui::Ui, r: &mut Repo, acts: &mut Vec<Act>) {
    egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin { left: 14, right: 14, top: 6, bottom: 0 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            for (t, label) in [(DetailTab::Commit, tr("Commit", "Commit")), (DetailTab::Changes, tr("Changes", "Änderungen")), (DetailTab::Tree, tr("File Tree", "Dateibaum"))] {
                let sel = r.detail_tab == t;
                let txt = RichText::new(label).font(FontId::new(13.0, theme::semibold())).color(if sel { theme::ACCENT } else { theme::TEXT_DIM });
                let resp = ui.add(egui::Button::new(txt).frame(false));
                if sel {
                    let rr = resp.rect;
                    ui.painter().line_segment([rr.left_bottom() + Vec2::new(0.0, 5.0), rr.right_bottom() + Vec2::new(0.0, 5.0)], Stroke::new(2.0, theme::ACCENT));
                }
                if resp.clicked() {
                    acts.push(Act::DetailTab(t));
                }
            }
        });
        ui.add_space(5.0);
    });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, theme::LINE);
    let Some(d) = &r.detail else {
        ui.add_space(10.0);
        ui.label(RichText::new(tr("Select a commit", "Wähle einen Commit")).color(theme::TEXT_DIM));
        return;
    };
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
                                ui.label(RichText::new(&d.author).font(FontId::new(13.5, theme::semibold())).color(theme::TEXT_STRONG));
                                ui.label(RichText::new(&d.email).size(12.0).color(theme::TEXT_DIM));
                            });
                            ui.label(RichText::new(&d.date).size(12.0).color(theme::TEXT_DIM));
                        });
                    });
                    ui.add_space(8.0);
                    egui::Grid::new("meta").num_columns(2).spacing([16.0, 5.0]).show(ui, |ui| {
                        let key = |ui: &mut egui::Ui, t: &str| {
                            ui.label(RichText::new(t).size(11.5).color(theme::TEXT_FAINT));
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
                    ui.label(RichText::new(lines.next().unwrap_or("")).font(FontId::new(15.0, theme::semibold())).color(theme::TEXT_STRONG));
                    let rest: String = lines.collect::<Vec<_>>().join("\n");
                    if !rest.trim().is_empty() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(rest.trim()).size(12.5));
                    }
                    ui.add_space(12.0);
                    ui.label(RichText::new(trs(format!("{} changed files", d.files.len()), format!("{} geänderte Dateien", d.files.len()))).size(11.5).color(theme::TEXT_FAINT));
                    ui.add_space(2.0);
                    for (st, path) in &d.files {
                        let code = st.chars().next().unwrap_or('M');
                        let resp = ui.horizontal(|ui| {
                            status_badge(ui, code);
                            let (file, dir) = split_path(path);
                            let r = ui.add(egui::Button::new(RichText::new(file).size(13.0)).frame(false));
                            if !dir.is_empty() {
                                ui.label(RichText::new(dir).size(11.5).color(theme::TEXT_FAINT));
                            }
                            r
                        }).inner;
                        if resp.on_hover_text(status_word(code)).clicked() {
                            acts.push(Act::DetailFile(path.clone()));
                        }
                    }
                });
            });
        }
        DetailTab::Changes => {
            egui::Panel::left("detail_files").resizable(true).default_size(260.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(4, 6))).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("dfiles").auto_shrink([false; 2]).show(ui, |ui| {
                    for (st, path) in &d.files {
                        let sel = r.detail_file.as_deref() == Some(path.as_str());
                        let (file, _) = split_path(path);
                        let resp = list_row(ui, None, file, false, "", sel, 18.0).on_hover_text(path);
                        let br = egui::Rect::from_center_size(egui::pos2(resp.rect.left() + 13.0, resp.rect.center().y), Vec2::splat(12.0));
                        let col = status_color(st.chars().next().unwrap_or('M'));
                        ui.painter().rect_filled(br, 3, col.gamma_multiply(0.25));
                        ui.painter().text(br.center(), Align2::CENTER_CENTER, st.chars().next().unwrap_or('M'), FontId::new(8.5, theme::semibold()), col);
                        if resp.clicked() {
                            acts.push(Act::DetailFile(path.clone()));
                        }
                    }
                });
            });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(egui::Margin::symmetric(0, 6))).show(ui, |ui| {
                if r.detail_file.is_some() {
                    diff_view(ui, &r.detail_diff, "ddiff");
                } else {
                    ui.label(RichText::new(tr("Select a file", "Wähle eine Datei")).color(theme::TEXT_DIM));
                }
            });
        }
        DetailTab::Tree => {
            egui::Panel::left("tree_files").resizable(true).default_size(300.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(4, 6))).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("tfiles").auto_shrink([false; 2]).show_rows(ui, 24.0, r.tree.len(), |ui, range| {
                    for i in range {
                        let p = &r.tree[i];
                        let sel = r.tree_file.as_deref() == Some(p.as_str());
                        if list_row(ui, None, p, false, "", sel, 0.0).clicked() {
                            acts.push(Act::TreeFile(p.clone()));
                        }
                    }
                });
            });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(10)).show(ui, |ui| {
                egui::ScrollArea::both().id_salt("tcontent").auto_shrink([false; 2]).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&r.tree_content).monospace().size(12.0)).extend());
                });
            });
        }
    }
}
