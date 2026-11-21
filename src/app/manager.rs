//! Everything around repositories as a whole:
//! - the tab list sidebar (toolbar button left of Quick Launch) with workspaces and the "+"
//!   that opens the Repository Manager,
//! - the Repository Manager: recent repositories, repositories found in the source folders,
//!   and for the selected one a summary (branches, tags, commits, README) and statistics
//!   (commits per month per author, contributors table, top contributors chart),
//! - workspaces (Home / Work…) with their own set of open tabs,
//! - the repository treemap and the performance benchmark windows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, RichText, Sense, Stroke, Vec2};

use super::tree::{self, Row};
use super::ui::{icon, icon_button};
use super::{Act, App};
use crate::git;
use crate::i18n::{tr, trs};
use crate::icons::{self, Icon};
use crate::theme;

// ---------------------------------------------------------------------------------------------
// Data

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Workspace {
    pub name: String,
    #[serde(default)]
    pub tabs: Vec<String>,
    #[serde(default)]
    pub active: usize,
}

impl Workspace {
    pub fn new(name: &str) -> Self {
        Workspace { name: name.into(), tabs: vec![], active: 0 }
    }
}

pub struct RepoInfo {
    pub path: PathBuf,
    pub uncommitted: usize,
    pub commits: String,
    pub initial: String,
    pub last: String,
    pub remotes: Vec<(String, String, Option<String>)>,
    pub branches: Vec<[String; 4]>,
    pub tags: Vec<[String; 4]>,
    pub recent: Vec<[String; 3]>,
    pub readme: Option<(String, String)>,
}

pub struct Stats {
    pub path: PathBuf,
    pub range: usize,
    /// month ("2026-03") -> author -> commits
    pub months: BTreeMap<String, BTreeMap<String, u32>>,
    /// authors by commits, most first
    pub authors: Vec<(String, u32)>,
    pub total: u32,
}

pub const RANGES: [(&str, &str, &str); 4] = [
    ("1.month.ago", "Last month", "Letzter Monat"),
    ("3.months.ago", "Last 3 months", "Letzte 3 Monate"),
    ("12.months.ago", "Last 12 months", "Letzte 12 Monate"),
    ("", "All time", "Gesamter Zeitraum"),
];

fn range_label(i: usize) -> &'static str {
    tr(RANGES[i].1, RANGES[i].2)
}

#[derive(Default)]
pub struct ManagerState {
    pub sel: Option<PathBuf>,
    pub tab: usize,
    pub info: Option<RepoInfo>,
    info_rx: Option<Receiver<RepoInfo>>,
    pub range: usize,
    pub stats: Option<Stats>,
    stats_rx: Option<Receiver<Stats>>,
    pub scan_rx: Option<Receiver<Vec<PathBuf>>>,
    pub filter: String,
    pub rename: Option<(PathBuf, String)>,
    pub confirm_remove: Option<PathBuf>,
}

pub struct WsDialog {
    pub list: Vec<Workspace>,
    pub sel: usize,
    pub in_title: bool,
}

// ---------------------------------------------------------------------------------------------
// Background work

fn run(dir: &Path, args: &[&str]) -> String {
    git::run(dir, args).stdout
}

/// The web page of a remote (GitHub, GitLab, Bitbucket, Azure…), if it can be told from its URL.
pub fn web_url(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches(".git");
    let (host, path) = if let Some(r) = u.strip_prefix("git@") {
        r.split_once(':')?
    } else if let Some(r) = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")) {
        let r = r.split_once('@').map(|x| x.1).unwrap_or(r);
        r.split_once('/')?
    } else if let Some(r) = u.strip_prefix("ssh://") {
        let r = r.split_once('@').map(|x| x.1).unwrap_or(r);
        let (h, p) = r.split_once('/')?;
        (h.split(':').next().unwrap_or(h), p)
    } else {
        return None;
    };
    Some(format!("https://{host}/{path}"))
}

fn issues_and_prs(web: &str) -> (String, String) {
    if web.contains("gitlab") {
        (format!("{web}/-/issues"), format!("{web}/-/merge_requests"))
    } else if web.contains("bitbucket") {
        (format!("{web}/issues"), format!("{web}/pull-requests"))
    } else {
        (format!("{web}/issues"), format!("{web}/pulls"))
    }
}

pub fn load_info(path: &Path) -> RepoInfo {
    let p = path;
    let uncommitted = run(p, &["status", "--porcelain"]).lines().filter(|l| !l.trim().is_empty()).count();
    let commits = run(p, &["rev-list", "--count", "HEAD"]).trim().to_string();
    let roots = run(p, &["rev-list", "--max-parents=0", "HEAD"]);
    let initial = rel(&roots.lines().last().map(|r| run(p, &["log", "-1", "--format=%cr", r]).trim().to_string()).unwrap_or_default());
    let last = rel(run(p, &["log", "-1", "--format=%cr"]).trim());
    let remotes = git::load_remotes(p).into_iter().map(|(n, u)| { let w = web_url(&u); (n, u, w) }).collect();
    let refs = |pat: &str| -> Vec<[String; 4]> {
        run(p, &["for-each-ref", "--sort=-committerdate", "--count=5", "--format=%(refname:short)%1f%(subject)%1f%(authorname)%1f%(committerdate:relative)", pat])
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\x1f').collect();
                (f.len() >= 4).then(|| [f[0].to_string(), f[1].to_string(), f[2].to_string(), rel(f[3])])
            })
            .collect()
    };
    let mut branches = refs("refs/heads");
    branches.extend(refs("refs/remotes"));
    branches.truncate(5);
    let tags = refs("refs/tags");
    let recent = run(p, &["log", "-5", "--format=%s%x1f%an%x1f%cr"])
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            (f.len() >= 3).then(|| [f[0].to_string(), f[1].to_string(), rel(f[2])])
        })
        .collect();
    let mut readme = None;
    for n in ["README.md", "README.MD", "readme.md", "Readme.md", "README", "README.txt", "README.rst"] {
        if let Ok(t) = std::fs::read_to_string(p.join(n)) {
            let t: String = t.chars().take(20000).collect();
            readme = Some((n.to_uppercase(), t));
            break;
        }
    }
    RepoInfo { path: p.to_path_buf(), uncommitted, commits, initial, last, remotes, branches, tags, recent, readme }
}

pub fn load_stats(path: &Path, range: usize) -> Stats {
    let since = RANGES[range].0;
    let mut args = vec!["log", "--no-merges", "--format=%an%x1f%ad", "--date=format:%Y-%m"];
    let s;
    if !since.is_empty() {
        s = format!("--since={since}");
        args.push(&s);
    }
    let out = run(path, &args);
    let mut months: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let mut totals: BTreeMap<String, u32> = BTreeMap::new();
    let mut total = 0;
    for l in out.lines() {
        if let Some((a, m)) = l.split_once('\x1f') {
            *months.entry(m.to_string()).or_default().entry(a.to_string()).or_default() += 1;
            *totals.entry(a.to_string()).or_default() += 1;
            total += 1;
        }
    }
    let mut authors: Vec<(String, u32)> = totals.into_iter().collect();
    authors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Stats { path: path.to_path_buf(), range, months, authors, total }
}

/// Folders under the source folders that hold a Git repository (not looking inside repositories).
pub fn scan(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    const SKIP: [&str; 12] = ["node_modules", "target", ".git", "$Recycle.Bin", "Windows", "Program Files", "Program Files (x86)", "ProgramData", "vendor", "bin", "obj", ".cache"];
    let mut found = Vec::new();
    let mut visited = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = dirs.into_iter().map(|d| (d, 0)).collect();
    while let Some((d, depth)) = stack.pop() {
        visited += 1;
        if visited > 60000 {
            break;
        }
        if d.join(".git").exists() {
            found.push(d);
            continue;
        }
        if depth >= 5 {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let Ok(ft) = e.file_type() else { continue };
                if !ft.is_dir() || ft.is_symlink() {
                    continue;
                }
                let n = e.file_name().to_string_lossy().to_string();
                if SKIP.contains(&n.as_str()) {
                    continue;
                }
                stack.push((e.path(), depth + 1));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

fn display_name(names: &BTreeMap<String, String>, p: &Path) -> String {
    names.get(&p.display().to_string()).cloned().unwrap_or_else(|| p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string()))
}

// ---------------------------------------------------------------------------------------------
// Drawing helpers

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(theme::PANEL_DARK()).corner_radius(4).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
    ui.add_space(8.0);
}

fn card_title(ui: &mut egui::Ui, t: &str) {
    ui.label(RichText::new(t).font(FontId::new(14.5, theme::semibold())).color(theme::TEXT_STRONG()));
    ui.add_space(6.0);
}

fn author_color(authors: &[(String, u32)], name: &str) -> Color32 {
    let i = authors.iter().position(|a| a.0 == name).unwrap_or(0);
    theme::lane_color(i)
}

fn line_chart(ui: &mut egui::Ui, s: &Stats) {
    let months: Vec<&String> = s.months.keys().collect();
    let top: Vec<&(String, u32)> = s.authors.iter().take(8).collect();
    let max = s.months.values().flat_map(|m| m.values()).copied().max().unwrap_or(1).max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(1000.0), 220.0), Sense::hover());
    let p = ui.painter_at(rect.expand(40.0));
    let plot = egui::Rect::from_min_max(rect.min + Vec2::new(36.0, 8.0), rect.max - Vec2::new(10.0, 26.0));
    p.rect_stroke(plot, 0, Stroke::new(1.0, theme::LINE()), egui::StrokeKind::Inside);
    for k in 0..=4 {
        let y = plot.bottom() - plot.height() * k as f32 / 4.0;
        p.line_segment([egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)], Stroke::new(1.0, theme::LINE().gamma_multiply(0.6)));
        p.text(egui::pos2(plot.left() - 6.0, y), Align2::RIGHT_CENTER, format!("{}", (max * k as f32 / 4.0).round()), FontId::proportional(10.5), theme::TEXT_FAINT());
    }
    let n = months.len().max(1);
    let x_of = |i: usize| if n == 1 { plot.center().x } else { plot.left() + plot.width() * i as f32 / (n - 1) as f32 };
    let step = (n / 12).max(1);
    for (i, m) in months.iter().enumerate() {
        if i % step == 0 || i == n - 1 {
            p.text(egui::pos2(x_of(i), plot.bottom() + 12.0), Align2::CENTER_CENTER, m.as_str(), FontId::proportional(10.5), theme::TEXT_DIM());
        }
    }
    for (a, _) in &top {
        let col = author_color(&s.authors, a);
        let pts: Vec<egui::Pos2> = months
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let v = s.months.get(*m).and_then(|x| x.get(a)).copied().unwrap_or(0) as f32;
                egui::pos2(x_of(i), plot.bottom() - plot.height() * v / max)
            })
            .collect();
        if pts.len() == 1 {
            p.circle_filled(pts[0], 3.5, col);
        } else {
            p.add(egui::Shape::line(pts.clone(), Stroke::new(2.0, col)));
            for q in pts {
                p.circle_filled(q, 2.5, col);
            }
        }
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (a, _) in &top {
            let (r, _) = ui.allocate_exact_size(Vec2::new(14.0, 12.0), Sense::hover());
            ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(2.5, author_color(&s.authors, a)));
            ui.label(RichText::new(a.as_str()).size(11.5).color(theme::TEXT_DIM()));
            ui.add_space(8.0);
        }
    });
}

fn donut(ui: &mut egui::Ui, s: &Stats) {
    let size = 240.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size + 80.0), Sense::hover());
    let p = ui.painter_at(rect);
    let c = rect.center();
    let r_out = size / 2.0;
    let r_in = r_out * 0.42;
    let total = s.total.max(1) as f32;
    let mut slices: Vec<(String, u32, Color32)> = s.authors.iter().take(5).map(|(a, n)| (a.clone(), *n, author_color(&s.authors, a))).collect();
    let rest: u32 = s.authors.iter().skip(5).map(|a| a.1).sum();
    if rest > 0 {
        slices.push((tr("Others", "Andere").to_string(), rest, theme::TEXT_FAINT()));
    }
    let mut a0 = -std::f32::consts::FRAC_PI_2;
    for (name, n, col) in &slices {
        let sweep = std::f32::consts::TAU * *n as f32 / total;
        let steps = ((sweep / 0.05).ceil() as usize).max(2);
        // each slice as small convex quads (outer arc to inner arc)
        for k in 0..steps {
            let t0 = a0 + sweep * k as f32 / steps as f32;
            let t1 = a0 + sweep * (k + 1) as f32 / steps as f32;
            let q = vec![
                c + Vec2::angled(t0) * r_in,
                c + Vec2::angled(t0) * r_out,
                c + Vec2::angled(t1) * r_out,
                c + Vec2::angled(t1) * r_in,
            ];
            p.add(egui::Shape::convex_polygon(q, *col, Stroke::NONE));
        }
        let mid = a0 + sweep / 2.0;
        let pct = (*n as f32 / total * 100.0).round();
        if pct >= 3.0 {
            p.text(c + Vec2::angled(mid) * (r_out + 18.0), Align2::CENTER_CENTER, format!("{pct} %"), FontId::proportional(11.0), theme::TEXT_DIM());
        }
        if sweep > 0.35 {
            p.text(c + Vec2::angled(mid) * ((r_in + r_out) / 2.0), Align2::CENTER_CENTER, short(name, 14), FontId::proportional(11.0), Color32::WHITE);
        }
        a0 += sweep;
    }
}

/// Git's relative dates ("3 hours ago") in the interface language.
pub fn rel(s: &str) -> String {
    if !crate::i18n::is_de() {
        return s.to_string();
    }
    let t = s.trim();
    let Some(body) = t.strip_suffix(" ago") else { return t.to_string() };
    let words = [("seconds", "Sekunden"), ("second", "Sekunde"), ("minutes", "Minuten"), ("minute", "Minute"), ("hours", "Stunden"), ("hour", "Stunde"), ("days", "Tagen"), ("day", "Tag"), ("weeks", "Wochen"), ("week", "Woche"), ("months", "Monaten"), ("month", "Monat"), ("years", "Jahren"), ("year", "Jahr")];
    let parts: Vec<String> = body
        .split(", ")
        .map(|p| {
            let mut out = p.to_string();
            for (en, de) in words {
                if let Some(n) = p.strip_suffix(en) {
                    out = format!("{n}{de}");
                    break;
                }
            }
            out
        })
        .collect();
    format!("vor {}", parts.join(", ").replace(" and ", " und "))
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() > n { format!("{}…", s.chars().take(n - 1).collect::<String>()) } else { s.to_string() }
}

// ---------------------------------------------------------------------------------------------
// App side

impl App {
    pub(super) fn tab_name(&self, p: &Path) -> String {
        display_name(&self.repo_names, p)
    }

    fn all_known(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.known_repos.clone();
        for r in &self.recent {
            let p = PathBuf::from(r);
            if !v.contains(&p) {
                v.push(p);
            }
        }
        v.retain(|p| !self.hidden_repos.contains(&p.display().to_string()));
        v
    }

    pub(super) fn start_scan(&mut self) {
        let dirs = self.src_dirs.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(scan(dirs));
        });
        self.mgr.scan_rx = Some(rx);
    }

    pub(super) fn open_manager(&mut self) {
        self.manager_tab = true;
        self.manager_active = true;
        self.commands_view = false;
        if self.known_repos.is_empty() && self.mgr.scan_rx.is_none() {
            self.start_scan();
        }
        if self.mgr.sel.is_none() {
            self.mgr.sel = self.recent.first().map(PathBuf::from).or_else(|| self.known_repos.first().cloned());
        }
    }

    fn select_repo(&mut self, p: PathBuf, ctx: &egui::Context) {
        if self.mgr.sel.as_ref() != Some(&p) || self.mgr.info.is_none() {
            self.mgr.sel = Some(p);
            self.mgr.info = None;
            self.mgr.stats = None;
            let _ = ctx;
        }
    }

    pub(super) fn poll_manager(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.mgr.scan_rx {
            if let Ok(found) = rx.try_recv() {
                self.known_repos = found;
                self.mgr.scan_rx = None;
                self.save();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        if let Some(rx) = &self.mgr.info_rx {
            if let Ok(i) = rx.try_recv() {
                self.mgr.info = Some(i);
                self.mgr.info_rx = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        if let Some(rx) = &self.mgr.stats_rx {
            if let Ok(s) = rx.try_recv() {
                self.mgr.stats = Some(s);
                self.mgr.stats_rx = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        // Ask for what the manager shows, once per selection.
        if self.manager_active {
            if let Some(sel) = self.mgr.sel.clone() {
                if self.mgr.info.as_ref().map(|i| i.path != sel).unwrap_or(true) && self.mgr.info_rx.is_none() {
                    let (tx, rx) = channel();
                    let p = sel.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(load_info(&p));
                    });
                    self.mgr.info_rx = Some(rx);
                }
                if self.mgr.tab == 1 && self.mgr.stats.as_ref().map(|s| s.path != sel || s.range != self.mgr.range).unwrap_or(true) && self.mgr.stats_rx.is_none() {
                    let (tx, rx) = channel();
                    let (p, r) = (sel.clone(), self.mgr.range);
                    std::thread::spawn(move || {
                        let _ = tx.send(load_stats(&p, r));
                    });
                    self.mgr.stats_rx = Some(rx);
                }
            }
        }
    }

    /// Opens a repository in a tab (or switches to it) and leaves the manager.
    pub(super) fn open_from_manager(&mut self, p: PathBuf, acts: &mut Vec<Act>) {
        acts.push(Act::OpenRepo(p));
        self.manager_active = false;
    }

    // ---------------------------------------------------------------- tab list sidebar

    pub(super) fn tab_list(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        egui::Panel::left("tablist").resizable(true).default_size(190.0).min_size(140.0).frame(egui::Frame::new().fill(theme::SIDEBAR()).inner_margin(egui::Margin { left: 6, right: 6, top: 8, bottom: 6 })).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                let ws = self.workspaces.get(self.ws).map(|w| w.name.clone()).unwrap_or_default();
                let r = ui.add(egui::Label::new(RichText::new(ws).font(FontId::new(13.5, theme::semibold())).color(theme::TEXT_STRONG())).sense(Sense::click()));
                icon(ui, Icon::ChevronDown, 9.0, theme::TEXT_DIM());
                let r = r.on_hover_text(tr("Switch or configure workspaces", "Workspace wechseln oder einrichten"));
                egui::Popup::menu(&r).show(|ui| self.workspace_menu(ui, acts));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, Icon::Plus, tr("Repository Manager – open or add repositories", "Repository Manager – Repositories öffnen oder hinzufügen")).clicked() {
                        acts.push(Act::Manager);
                    }
                });
            });
            ui.add_space(4.0);
            let names: Vec<(usize, String, PathBuf, bool)> = self.tabs.iter().enumerate().map(|(i, t)| (i, t.name.clone(), t.path.clone(), !t.status.is_empty())).collect();
            for (i, name, path, dirty) in names {
                let sel = i == self.active && !self.manager_active;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                let p = ui.painter_at(rect);
                if sel {
                    p.rect_filled(rect, 4, theme::ROW_SEL());
                } else if resp.hovered() {
                    p.rect_filled(rect, 4, theme::ROW_HOVER());
                }
                if dirty && self.prefs_tab_indicator {
                    p.circle_filled(egui::pos2(rect.left() + 10.0, rect.center().y), 3.0, theme::TEXT_DIM());
                }
                p.text(egui::pos2(rect.left() + 22.0, rect.center().y), Align2::LEFT_CENTER, short(&name, 26), FontId::proportional(12.5), if sel { theme::TEXT_STRONG() } else { theme::TEXT() });
                let resp = resp.on_hover_text(path.display().to_string());
                if resp.clicked() {
                    acts.push(Act::SwitchTab(i));
                }
                resp.context_menu(|ui| self.tab_menu(ui, acts, i));
            }
            if self.manager_tab {
                let sel = self.manager_active;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                let p = ui.painter_at(rect);
                if sel {
                    p.rect_filled(rect, 4, theme::ROW_SEL());
                } else if resp.hovered() {
                    p.rect_filled(rect, 4, theme::ROW_HOVER());
                }
                p.text(egui::pos2(rect.left() + 22.0, rect.center().y), Align2::LEFT_CENTER, tr("Repository Manager", "Repository Manager"), FontId::proportional(12.5), if sel { theme::TEXT_STRONG() } else { theme::TEXT() });
                if resp.clicked() {
                    acts.push(Act::Manager);
                }
            }
        });
    }

    pub(super) fn tab_menu(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>, i: usize) {
        let Some(path) = self.tabs.get(i).map(|t| t.path.clone()) else { return };
        if ui.button(tr("Close", "Schließen")).clicked() {
            acts.push(Act::CloseTab(i));
            ui.close();
        }
        if ui.button(tr("Close All", "Alle schließen")).clicked() {
            acts.push(Act::CloseOthers(None));
            ui.close();
        }
        if ui.button(tr("Close All But This", "Alle anderen schließen")).clicked() {
            acts.push(Act::CloseOthers(Some(i)));
            ui.close();
        }
        ui.separator();
        if ui.button(tr("Open In Console", "In Konsole öffnen")).clicked() {
            super::auth::open_terminal(self.shell, &path);
            ui.close();
        }
        if ui.button(tr("Open In File Explorer", "Im Datei-Explorer öffnen")).clicked() {
            super::auth::open_explorer(&path);
            ui.close();
        }
        ui.separator();
        if ui.button(tr("Rename…", "Umbenennen…")).clicked() {
            self.mgr.rename = Some((path.clone(), self.tab_name(&path)));
            ui.close();
        }
        ui.menu_button(tr("Move to Workspace", "In Workspace verschieben"), |ui| {
            for (wi, w) in self.workspaces.clone().iter().enumerate() {
                if wi != self.ws && ui.button(&w.name).clicked() {
                    acts.push(Act::MoveToWorkspace(i, wi));
                    ui.close();
                }
            }
        });
    }

    pub(super) fn workspace_menu(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        ui.label(RichText::new(tr("Workspaces", "Workspaces")).size(11.5).color(theme::TEXT_FAINT()));
        for (i, w) in self.workspaces.clone().iter().enumerate() {
            if ui.radio(i == self.ws, &w.name).clicked() {
                acts.push(Act::SwitchWorkspace(i));
                ui.close();
            }
        }
        ui.separator();
        if ui.button(tr("Configure…", "Einrichten…")).clicked() {
            self.ws_dialog = Some(WsDialog { list: self.workspaces.clone(), sel: self.ws, in_title: self.ws_in_title });
            ui.close();
        }
    }

    // ---------------------------------------------------------------- repository manager

    pub(super) fn manager_view(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>, ctx: &egui::Context) {
        let known = self.all_known();
        let recent: Vec<PathBuf> = self.recent.iter().map(PathBuf::from).filter(|p| !self.hidden_repos.contains(&p.display().to_string())).collect();
        let mut pick: Option<PathBuf> = None;
        let mut open: Option<PathBuf> = None;
        egui::Panel::left("mgr_list").resizable(true).default_size(340.0).min_size(240.0).frame(egui::Frame::new().fill(theme::SIDEBAR()).inner_margin(egui::Margin { left: 8, right: 8, top: 12, bottom: 6 })).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Repository Manager", "Repository Manager")).font(FontId::new(19.0, theme::semibold())).color(theme::TEXT_STRONG()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let b = icon_button(ui, Icon::Plus, tr("Add, clone or create", "Hinzufügen, klonen oder anlegen"));
                    egui::Popup::menu(&b).show(|ui| {
                        if ui.button(tr("Add Existing Repository…", "Vorhandenes Repository hinzufügen…")).clicked() {
                            acts.push(Act::OpenRepoDialog);
                            ui.close();
                        }
                        if ui.button(tr("Clone Repository…", "Repository klonen…")).clicked() {
                            acts.push(Act::Open("clone", vec![]));
                            ui.close();
                        }
                        if ui.button(tr("New Repository…", "Neues Repository…")).clicked() {
                            acts.push(Act::Open("init", vec![]));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(tr("Rescan Repositories…", "Repositories neu suchen…")).clicked() {
                            self.start_scan();
                            ui.close();
                        }
                        if ui.button(tr("Source Code Folders…", "Quellcode-Ordner…")).clicked() {
                            acts.push(Act::Prefs);
                            ui.close();
                        }
                    });
                    if self.mgr.scan_rx.is_some() {
                        ui.spinner();
                    }
                });
            });
            ui.add_space(6.0);
            ui.add(egui::TextEdit::singleline(&mut self.mgr.filter).hint_text(tr("Filter", "Filter")).desired_width(f32::INFINITY));
            ui.add_space(4.0);
            let f = self.mgr.filter.to_lowercase();
            let names = self.repo_names.clone();
            let hit = |p: &Path| f.is_empty() || display_name(&names, p).to_lowercase().contains(&f) || p.display().to_string().to_lowercase().contains(&f);
            let sel = self.mgr.sel.clone();
            let mut row = |ui: &mut egui::Ui, p: &Path, label: &str, indent: f32, acts: &mut Vec<Act>, me: &mut App| {
                let is_sel = sel.as_deref() == Some(p);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                let pt = ui.painter_at(rect);
                if is_sel {
                    pt.rect_filled(rect, 3, theme::ACCENT_DARK());
                } else if resp.hovered() {
                    pt.rect_filled(rect, 3, theme::ROW_HOVER());
                }
                let x = rect.left() + 10.0 + indent;
                icons::draw(&pt, egui::Rect::from_center_size(egui::pos2(x + 6.0, rect.center().y), Vec2::splat(13.0)), Icon::History, if is_sel { Color32::WHITE } else { theme::TEXT_DIM() });
                pt.text(egui::pos2(x + 20.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.0), if is_sel { Color32::WHITE } else { theme::TEXT() });
                let resp = resp.on_hover_text(p.display().to_string());
                if resp.double_clicked() {
                    open = Some(p.to_path_buf());
                } else if resp.clicked() {
                    pick = Some(p.to_path_buf());
                }
                let pp = p.to_path_buf();
                resp.context_menu(|ui| {
                    if ui.button(tr("Open", "Öffnen")).clicked() {
                        open = Some(pp.clone());
                        ui.close();
                    }
                    if ui.button(tr("Open In File Explorer", "Im Datei-Explorer öffnen")).clicked() {
                        super::auth::open_explorer(&pp);
                        ui.close();
                    }
                    if ui.button(tr("Open In Console", "In Konsole öffnen")).clicked() {
                        super::auth::open_terminal(me.shell, &pp);
                        ui.close();
                    }
                    ui.menu_button(tr("Move to Workspace", "In Workspace verschieben"), |ui| {
                        for (wi, w) in me.workspaces.clone().iter().enumerate() {
                            if ui.button(&w.name).clicked() {
                                acts.push(Act::OpenInWorkspace(pp.clone(), wi));
                                ui.close();
                            }
                        }
                    });
                    if ui.button(tr("Rename…", "Umbenennen…")).clicked() {
                        me.mgr.rename = Some((pp.clone(), display_name(&me.repo_names, &pp)));
                        ui.close();
                    }
                    if ui.button(tr("Copy Path", "Pfad kopieren")).clicked() {
                        acts.push(Act::Copy(pp.display().to_string()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(tr("Remove…", "Entfernen…")).clicked() {
                        me.mgr.confirm_remove = Some(pp.clone());
                        ui.close();
                    }
                    if ui.button(tr("Rescan Repositories…", "Repositories neu suchen…")).clicked() {
                        me.start_scan();
                        ui.close();
                    }
                });
            };
            egui::ScrollArea::vertical().id_salt("mgr_scroll").auto_shrink([false; 2]).show(ui, |ui| {
                let ctx = ui.ctx().clone();
                // Recent
                let rk = "mgr-recent";
                let open_recent = tree::is_open(&ctx, rk, "", true);
                let (hr, _) = tree::dir_row(ui, tr("Recent", "Zuletzt geöffnet"), 0, recent.len(), open_recent, None);
                if hr.clicked() {
                    tree::toggle(&ctx, rk, "", true);
                }
                if open_recent {
                    for p in recent.iter().filter(|p| hit(p)).take(12) {
                        let label = display_name(&self.repo_names, p);
                        row(ui, p, &label, 16.0, acts, self);
                    }
                }
                ui.add_space(4.0);
                // Repositories, grouped by folder below the source folders
                let rk = "mgr-repos";
                let open_repos = tree::is_open(&ctx, rk, "", true);
                let (hr, _) = tree::dir_row(ui, tr("Repositories", "Repositories"), 0, known.len(), open_repos, None);
                if hr.clicked() {
                    tree::toggle(&ctx, rk, "", true);
                }
                if open_repos {
                    let filtered: Vec<&PathBuf> = known.iter().filter(|p| hit(p)).collect();
                    let rel: Vec<String> = filtered
                        .iter()
                        .map(|p| {
                            for d in &self.src_dirs {
                                if let Ok(r) = p.strip_prefix(d) {
                                    let s = r.display().to_string().replace('\\', "/");
                                    if !s.is_empty() {
                                        return s;
                                    }
                                }
                            }
                            p.display().to_string().replace('\\', "/")
                        })
                        .collect();
                    let refs: Vec<&str> = rel.iter().map(|s| s.as_str()).collect();
                    for r in tree::rows(&ctx, "mgr-tree", &refs, false) {
                        match r {
                            Row::Dir { path, label, depth, count, open } => {
                                if tree::dir_row(ui, &label, depth + 1, count, open, None).0.clicked() {
                                    tree::toggle(&ctx, "mgr-tree", &path, false);
                                }
                            }
                            Row::File { idx, depth, .. } => {
                                let p = filtered[idx];
                                let label = display_name(&self.repo_names, p);
                                row(ui, p, &label, 16.0 + depth as f32 * tree::INDENT, acts, self);
                            }
                        }
                    }
                    if known.is_empty() {
                        ui.add_space(8.0);
                        ui.label(RichText::new(if self.mgr.scan_rx.is_some() { tr("Looking for repositories…", "Suche Repositories…") } else { tr("No repositories found yet. Add one with + or set your source code folders.", "Noch keine Repositories gefunden. Füge eines mit + hinzu oder lege deine Quellcode-Ordner fest.") }).size(12.0).color(theme::TEXT_FAINT()));
                    }
                }
            });
        });
        if let Some(p) = pick {
            self.select_repo(p, ctx);
        }
        if let Some(p) = open.take() {
            self.open_from_manager(p, acts);
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL()).inner_margin(egui::Margin { left: 24, right: 20, top: 14, bottom: 8 })).show(ui, |ui| {
            let Some(sel) = self.mgr.sel.clone() else {
                ui.add_space(120.0);
                ui.vertical_centered(|ui| {
                    icon(ui, Icon::Folder, 44.0, theme::TEXT_FAINT());
                    ui.label(RichText::new(tr("Drop a repository folder here or pick one on the left.", "Ziehe einen Repository-Ordner hierher oder wähle links eines.")).color(theme::TEXT_DIM()));
                });
                return;
            };
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(display_name(&self.repo_names, &sel)).font(FontId::new(21.0, theme::semibold())).color(theme::TEXT_STRONG()));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(super::auth::native(&sel)).size(12.0).color(theme::TEXT_DIM()));
                        if icon_button(ui, Icon::Folder, tr("Open In File Explorer", "Im Datei-Explorer öffnen")).clicked() {
                            super::auth::open_explorer(&sel);
                        }
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                    if ui.add(egui::Button::new(RichText::new(tr("Open", "Öffnen")).color(Color32::WHITE)).fill(theme::ACCENT_DARK()).min_size(Vec2::new(80.0, 26.0))).clicked() {
                        open = Some(sel.clone());
                    }
                });
            });
            ui.add_space(8.0);
            let Some(info) = self.mgr.info.as_ref().filter(|i| i.path == sel) else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(tr("Reading the repository…", "Lese das Repository…")).color(theme::TEXT_DIM()));
                });
                return;
            };
            card(ui, |ui| {
                ui.horizontal_top(|ui| {
                    egui::Grid::new("mgr_facts").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                        let k = |ui: &mut egui::Ui, t: &str| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(t).font(FontId::new(12.5, theme::semibold()))));
                        };
                        k(ui, tr("Uncommitted Files:", "Nicht committete Dateien:"));
                        ui.label(info.uncommitted.to_string());
                        ui.end_row();
                        k(ui, tr("Commits:", "Commits:"));
                        ui.label(&info.commits);
                        ui.end_row();
                        k(ui, tr("Initial Commit:", "Erster Commit:"));
                        ui.label(&info.initial);
                        ui.end_row();
                        k(ui, tr("Last Commit:", "Letzter Commit:"));
                        ui.label(&info.last);
                        ui.end_row();
                    });
                    ui.add_space(40.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(tr("Remotes:", "Remotes:")).font(FontId::new(12.5, theme::semibold())));
                        if info.remotes.is_empty() {
                            ui.label(RichText::new(tr("none", "keine")).color(theme::TEXT_FAINT()));
                        }
                        for (n, u, w) in &info.remotes {
                            ui.horizontal(|ui| {
                                icon(ui, Icon::Cloud, 13.0, theme::TEXT_DIM());
                                ui.label(n).on_hover_text(u);
                                if let Some(w) = w {
                                    let host = if w.contains("github") { "GitHub" } else if w.contains("gitlab") { "GitLab" } else if w.contains("bitbucket") { "Bitbucket" } else if w.contains("azure") { "Azure DevOps" } else { tr("Website", "Webseite") };
                                    let (iss, prs) = issues_and_prs(w);
                                    if ui.link(host).clicked() {
                                        super::auth::open_url(w);
                                    }
                                    if ui.link(tr("Issues", "Issues")).clicked() {
                                        super::auth::open_url(&iss);
                                    }
                                    if ui.link(tr("Pull Requests", "Pull Requests")).clicked() {
                                        super::auth::open_url(&prs);
                                    }
                                }
                            });
                        }
                    });
                });
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                for (i, t) in [tr("Summary", "Übersicht"), tr("Statistics", "Statistik")].iter().enumerate() {
                    let s = self.mgr.tab == i;
                    let r = ui.add(egui::Button::new(RichText::new(*t).font(FontId::new(13.0, theme::semibold())).color(if s { theme::ACCENT() } else { theme::TEXT_DIM() })).frame(false));
                    if s {
                        ui.painter().line_segment([r.rect.left_bottom() + Vec2::new(0.0, 4.0), r.rect.right_bottom() + Vec2::new(0.0, 4.0)], Stroke::new(2.0, theme::ACCENT()));
                    }
                    if r.clicked() {
                        self.mgr.tab = i;
                    }
                }
            });
            ui.add_space(8.0);
            let tab = self.mgr.tab;
            egui::ScrollArea::vertical().id_salt("mgr_detail").auto_shrink([false; 2]).show(ui, |ui| {
                if tab == 0 {
                    let info = self.mgr.info.as_ref().unwrap();
                    card(ui, |ui| {
                        card_title(ui, tr("Branches", "Branches"));
                        if info.branches.is_empty() {
                            ui.label(RichText::new(tr("none", "keine")).color(theme::TEXT_FAINT()));
                        }
                        egui::Grid::new("mgr_br").num_columns(4).spacing([18.0, 4.0]).show(ui, |ui| {
                            for b in &info.branches {
                                ui.horizontal(|ui| {
                                    icon(ui, Icon::Branch, 13.0, theme::TEXT_DIM());
                                    ui.label(short(&b[0], 32));
                                });
                                ui.label(RichText::new(short(&b[1], 48)).color(theme::TEXT_DIM()));
                                ui.label(&b[2]);
                                ui.label(RichText::new(&b[3]).color(theme::TEXT_DIM()));
                                ui.end_row();
                            }
                        });
                    });
                    card(ui, |ui| {
                        card_title(ui, tr("Tags", "Tags"));
                        egui::Grid::new("mgr_tags").num_columns(4).spacing([18.0, 4.0]).show(ui, |ui| {
                            for b in &info.tags {
                                ui.horizontal(|ui| {
                                    icon(ui, Icon::Tag, 13.0, theme::TEXT_DIM());
                                    ui.label(short(&b[0], 32));
                                });
                                ui.label(RichText::new(short(&b[1], 48)).color(theme::TEXT_DIM()));
                                ui.label(&b[2]);
                                ui.label(RichText::new(&b[3]).color(theme::TEXT_DIM()));
                                ui.end_row();
                            }
                        });
                    });
                    card(ui, |ui| {
                        card_title(ui, tr("Commits", "Commits"));
                        egui::Grid::new("mgr_commits").num_columns(3).spacing([18.0, 4.0]).show(ui, |ui| {
                            for c in &info.recent {
                                ui.horizontal(|ui| {
                                    icon(ui, Icon::Commit, 13.0, theme::TEXT_DIM());
                                    ui.label(short(&c[0], 60));
                                });
                                ui.label(&c[1]);
                                ui.label(RichText::new(&c[2]).color(theme::TEXT_DIM()));
                                ui.end_row();
                            }
                        });
                    });
                    if let Some((name, text)) = &info.readme {
                        card(ui, |ui| {
                            card_title(ui, name);
                            ui.add(egui::Label::new(RichText::new(text).monospace().size(12.0)).wrap());
                        });
                    }
                } else {
                    card(ui, |ui| {
                        egui::ComboBox::from_id_salt("mgr_range").selected_text(range_label(self.mgr.range)).show_ui(ui, |ui| {
                            for i in 0..RANGES.len() {
                                ui.selectable_value(&mut self.mgr.range, i, range_label(i));
                            }
                        });
                        ui.add_space(8.0);
                        match self.mgr.stats.as_ref().filter(|s| s.path == sel && s.range == self.mgr.range) {
                            None => {
                                ui.spinner();
                            }
                            Some(s) if s.total == 0 => {
                                ui.label(RichText::new(tr("No commits in this period.", "Keine Commits in diesem Zeitraum.")).color(theme::TEXT_DIM()));
                            }
                            Some(s) => {
                                card_title(ui, tr("Commits per month", "Commits pro Monat"));
                                line_chart(ui, s);
                                ui.add_space(18.0);
                                ui.horizontal_top(|ui| {
                                    ui.vertical(|ui| {
                                        card_title(ui, tr("All contributors", "Alle Beteiligten"));
                                        egui::Frame::new().stroke(Stroke::new(1.0, theme::LINE())).inner_margin(6).show(ui, |ui| {
                                            egui::Grid::new("mgr_contrib").num_columns(2).striped(true).spacing([40.0, 3.0]).min_col_width(60.0).show(ui, |ui| {
                                                for (a, n) in &s.authors {
                                                    ui.label(a);
                                                    ui.label(n.to_string());
                                                    ui.end_row();
                                                }
                                            });
                                        });
                                    });
                                    ui.add_space(40.0);
                                    ui.vertical(|ui| {
                                        card_title(ui, tr("Top contributors", "Die meisten Commits"));
                                        donut(ui, s);
                                    });
                                });
                            }
                        }
                    });
                }
            });
        });
        if let Some(p) = open {
            self.open_from_manager(p, acts);
        }
        // Files dropped onto the window are added as repositories.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for d in dropped {
            if let Some(top) = git::find_toplevel(&d) {
                if !self.known_repos.contains(&top) {
                    self.known_repos.push(top.clone());
                }
                self.mgr.sel = Some(top);
                self.save();
            }
        }
    }

    /// Small dialogs of the manager: rename and remove, and the workspace configuration.
    pub(super) fn manager_dialogs(&mut self, ctx: &egui::Context) {
        if let Some((p, mut name)) = self.mgr.rename.take() {
            let mut keep = true;
            let mut done = false;
            egui::Window::new(tr("Rename Repository", "Repository umbenennen")).id(Id::unique("mgr_rename")).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
                ui.label(RichText::new(tr("The name shown in tabs and lists. The folder isn't renamed.", "Der Name in Tabs und Listen. Der Ordner wird nicht umbenannt.")).size(12.0).color(theme::TEXT_DIM()));
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(320.0));
                r.request_focus();
                ui.horizontal(|ui| {
                    if ui.button(tr("Rename", "Umbenennen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        done = true;
                    }
                    if ui.button(tr("Cancel", "Abbrechen")).clicked() {
                        keep = false;
                    }
                });
            });
            if done {
                let key = p.display().to_string();
                if name.trim().is_empty() {
                    self.repo_names.remove(&key);
                } else {
                    self.repo_names.insert(key, name.trim().to_string());
                }
                let n = self.tab_name(&p);
                for t in self.tabs.iter_mut().filter(|t| t.path == p) {
                    t.name = n.clone();
                }
                self.save();
            } else if keep && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.mgr.rename = Some((p, name));
            }
        }
        if let Some(p) = self.mgr.confirm_remove.take() {
            let mut keep = true;
            egui::Window::new(tr("Remove Repository", "Repository entfernen")).id(Id::unique("mgr_remove")).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
                ui.label(trs(format!("Remove {} from the list?", self.tab_name(&p)), format!("{} aus der Liste entfernen?", self.tab_name(&p))));
                ui.label(RichText::new(tr("Only the entry is removed. Files on disk stay untouched.", "Nur der Eintrag verschwindet. Die Dateien bleiben unverändert.")).size(12.0).color(theme::TEXT_DIM()));
                ui.horizontal(|ui| {
                    if ui.button(tr("Remove", "Entfernen")).clicked() {
                        self.hidden_repos.push(p.display().to_string());
                        self.recent.retain(|r| PathBuf::from(r) != p);
                        if self.mgr.sel.as_ref() == Some(&p) {
                            self.mgr.sel = None;
                        }
                        self.save();
                        keep = false;
                    }
                    if ui.button(tr("Cancel", "Abbrechen")).clicked() {
                        keep = false;
                    }
                });
            });
            if keep && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.mgr.confirm_remove = Some(p);
            }
        }
        if let Some(mut d) = self.ws_dialog.take() {
            let mut keep = true;
            let mut submit = false;
            egui::Window::new(tr("Workspaces", "Workspaces")).id(Id::unique("ws_dialog")).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(tr("Each workspace keeps its own set of open repositories.", "Jeder Workspace hat seine eigenen offenen Repositories.")).size(12.0).color(theme::TEXT_DIM()));
                ui.add_space(6.0);
                for (i, w) in d.list.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.radio(d.sel == i, "").clicked() {
                            d.sel = i;
                        }
                        ui.add(egui::TextEdit::singleline(&mut w.name).desired_width(240.0));
                        ui.label(RichText::new(trs(format!("{} tabs", w.tabs.len()), format!("{} Tabs", w.tabs.len()))).size(11.5).color(theme::TEXT_FAINT()));
                    });
                }
                ui.horizontal(|ui| {
                    if icon_button(ui, Icon::Plus, tr("Add workspace", "Workspace hinzufügen")).clicked() {
                        d.list.push(Workspace::new(tr("New Workspace", "Neuer Workspace")));
                        d.sel = d.list.len() - 1;
                    }
                    if d.list.len() > 1 && icon_button(ui, Icon::Close, tr("Remove the selected workspace", "Gewählten Workspace entfernen")).clicked() {
                        d.list.remove(d.sel);
                        d.sel = d.sel.min(d.list.len() - 1);
                    }
                });
                ui.checkbox(&mut d.in_title, tr("Show workspace name in title bar", "Workspace-Namen in der Titelleiste zeigen"));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(tr("Save", "Speichern")).clicked() {
                        submit = true;
                    }
                    if ui.button(tr("Close", "Schließen")).clicked() {
                        keep = false;
                    }
                });
            });
            if submit {
                // keep the current workspace's tabs in sync before replacing the list
                self.store_workspace_tabs();
                let cur_name = self.workspaces.get(self.ws).map(|w| w.name.clone());
                for (i, w) in d.list.iter_mut().enumerate() {
                    if let Some(old) = self.workspaces.get(i) {
                        if w.tabs.is_empty() {
                            w.tabs = old.tabs.clone();
                        }
                    }
                }
                self.workspaces = d.list;
                self.ws = cur_name.and_then(|n| self.workspaces.iter().position(|w| w.name == n)).unwrap_or(0).min(self.workspaces.len() - 1);
                self.ws_in_title = d.in_title;
                self.update_title(ctx);
                self.save();
            } else if keep && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.ws_dialog = Some(d);
            }
        }
        self.treemap_window(ctx);
        self.bench_window(ctx);
    }

    pub(super) fn store_workspace_tabs(&mut self) {
        let tabs: Vec<String> = self.tabs.iter().map(|t| t.path.display().to_string()).collect();
        let active = self.active;
        if let Some(w) = self.workspaces.get_mut(self.ws) {
            w.tabs = tabs;
            w.active = active;
        }
    }

    pub(super) fn switch_workspace(&mut self, i: usize, ctx: &egui::Context) {
        if i == self.ws || i >= self.workspaces.len() {
            return;
        }
        self.store_workspace_tabs();
        self.ws = i;
        self.tabs.clear();
        let w = self.workspaces[i].clone();
        for t in &w.tabs {
            let p = PathBuf::from(t);
            if p.exists() && git::find_toplevel(&p).is_some() {
                let mut r = super::Repo::open(p.clone());
                r.name = self.tab_name(&p);
                self.tabs.push(r);
            }
        }
        self.active = w.active.min(self.tabs.len().saturating_sub(1));
        self.update_title(ctx);
        self.save();
    }

    pub(super) fn update_title(&self, ctx: &egui::Context) {
        let t = match (self.ws_in_title, self.workspaces.get(self.ws)) {
            (true, Some(w)) => format!("YAGUI — {}", w.name),
            _ => "YAGUI".to_string(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(t));
    }
}

// ---------------------------------------------------------------------------------------------
// Treemap

pub struct Treemap {
    pub path: PathBuf,
    pub range: usize,
    files: Vec<(String, u32)>,
    rx: Option<Receiver<Vec<(String, u32)>>>,
    pub folder: String,
    pub sel: Option<String>,
    detail: Option<(String, Vec<[String; 3]>, Vec<(String, u32)>)>,
    detail_tab: usize,
}

impl Treemap {
    pub fn new(path: PathBuf) -> Self {
        let mut t = Treemap { path, range: 1, files: vec![], rx: None, folder: String::new(), sel: None, detail: None, detail_tab: 0 };
        t.load();
        t
    }
    fn load(&mut self) {
        let (tx, rx) = channel();
        let (p, r) = (self.path.clone(), self.range);
        std::thread::spawn(move || {
            let mut args = vec!["log", "--no-merges", "--name-only", "--format="];
            let s;
            if !RANGES[r].0.is_empty() {
                s = format!("--since={}", RANGES[r].0);
                args.push(&s);
            }
            let out = git::run(&p, &args).stdout;
            let mut m: BTreeMap<String, u32> = BTreeMap::new();
            for l in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
                *m.entry(l.to_string()).or_default() += 1;
            }
            let _ = tx.send(m.into_iter().filter(|(f, _)| p.join(f).exists()).collect());
        });
        self.rx = Some(rx);
        self.files.clear();
    }
}

/// Squarified treemap layout of weighted items into a rectangle.
fn squarify(items: &[(String, f32)], rect: egui::Rect) -> Vec<(String, egui::Rect)> {
    let mut out = Vec::new();
    let total: f32 = items.iter().map(|i| i.1).sum();
    if total <= 0.0 || items.is_empty() {
        return out;
    }
    let scale = rect.area() / total;
    let mut rest: Vec<(String, f32)> = items.iter().map(|(n, w)| (n.clone(), w * scale)).collect();
    let mut r = rect;
    while !rest.is_empty() {
        let short = r.width().min(r.height()).max(1.0);
        let mut row: Vec<(String, f32)> = Vec::new();
        let worst = |row: &Vec<(String, f32)>| -> f32 {
            let s: f32 = row.iter().map(|x| x.1).sum();
            let mx = row.iter().map(|x| x.1).fold(0.0, f32::max);
            let mn = row.iter().map(|x| x.1).fold(f32::MAX, f32::min);
            (short * short * mx / (s * s)).max(s * s / (short * short * mn))
        };
        while let Some(next) = rest.first().cloned() {
            let mut cand = row.clone();
            cand.push(next);
            if row.is_empty() || worst(&cand) <= worst(&row) {
                row = cand;
                rest.remove(0);
            } else {
                break;
            }
        }
        let s: f32 = row.iter().map(|x| x.1).sum();
        if r.width() >= r.height() {
            let w = s / r.height().max(1.0);
            let mut y = r.top();
            for (n, a) in &row {
                let h = a / w.max(0.001);
                out.push((n.clone(), egui::Rect::from_min_size(egui::pos2(r.left(), y), Vec2::new(w, h))));
                y += h;
            }
            r = egui::Rect::from_min_max(egui::pos2(r.left() + w, r.top()), r.max);
        } else {
            let h = s / r.width().max(1.0);
            let mut x = r.left();
            for (n, a) in &row {
                let w = a / h.max(0.001);
                out.push((n.clone(), egui::Rect::from_min_size(egui::pos2(x, r.top()), Vec2::new(w, h))));
                x += w;
            }
            r = egui::Rect::from_min_max(egui::pos2(r.left(), r.top() + h), r.max);
        }
    }
    out
}

impl App {
    fn treemap_window(&mut self, ctx: &egui::Context) {
        let Some(mut t) = self.treemap.take() else { return };
        if let Some(rx) = &t.rx {
            if let Ok(f) = rx.try_recv() {
                t.files = f;
                t.rx = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        let mut open = true;
        let mut reload = false;
        let title = trs(format!("Repository Treemap  {}", super::auth::native(&t.path)), format!("Repository-Treemap  {}", super::auth::native(&t.path)));
        egui::Window::new(RichText::new(title).font(FontId::new(14.0, theme::semibold())))
            .id(Id::unique("treemap"))
            .open(&mut open)
            .default_size([1100.0, 700.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let before = t.range;
                    egui::ComboBox::from_id_salt("tm_range").selected_text(range_label(t.range)).show_ui(ui, |ui| {
                        for i in 0..RANGES.len() {
                            ui.selectable_value(&mut t.range, i, range_label(i));
                        }
                    });
                    reload = before != t.range;
                    ui.label(RichText::new(tr("Size = how often a file changed. Click a folder to go in, a file to see its history.", "Größe = wie oft eine Datei geändert wurde. Ordner anklicken zum Hineingehen, Datei für ihren Verlauf.")).size(11.5).color(theme::TEXT_FAINT()));
                });
                ui.horizontal(|ui| {
                    if ui.link(".").clicked() {
                        t.folder.clear();
                    }
                    let parts: Vec<String> = t.folder.split('/').filter(|s| !s.is_empty()).map(String::from).collect();
                    for (i, part) in parts.iter().enumerate() {
                        ui.label("/");
                        if ui.link(part).clicked() {
                            t.folder = parts[..=i].join("/");
                        }
                    }
                });
                if t.rx.is_some() {
                    ui.spinner();
                    return;
                }
                let avail = ui.available_size();
                let side = 330.0;
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new((avail.x - side - 10.0).max(200.0), (avail.y - 6.0).max(200.0)), Sense::hover());
                    // children of the current folder with their total weight
                    let prefix = if t.folder.is_empty() { String::new() } else { format!("{}/", t.folder) };
                    let mut groups: BTreeMap<String, (f32, bool)> = BTreeMap::new();
                    for (f, n) in &t.files {
                        if let Some(rest) = f.strip_prefix(&prefix) {
                            let (head, is_dir) = match rest.split_once('/') {
                                Some((h, _)) => (h.to_string(), true),
                                None => (rest.to_string(), false),
                            };
                            let e = groups.entry(head).or_insert((0.0, is_dir));
                            e.0 += *n as f32;
                        }
                    }
                    let mut items: Vec<(String, f32)> = groups.iter().map(|(k, v)| (k.clone(), v.0)).collect();
                    items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 0, theme::PANEL_DARK());
                    for (name, r) in squarify(&items, rect.shrink(1.0)) {
                        let (w, is_dir) = groups[&name];
                        let full = format!("{prefix}{name}");
                        let rr = r.shrink(1.5);
                        let resp = ui.interact(rr, Id::unique(("tm", &full)), Sense::click());
                        let sel = t.sel.as_deref() == Some(full.as_str());
                        let fill = if is_dir { theme::PANEL() } else { theme::ROW_HOVER() };
                        p.rect_filled(rr, 2, if resp.hovered() { theme::ROW_SEL() } else { fill });
                        p.rect_stroke(rr, 2, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { theme::ACCENT() } else { theme::LINE() }), egui::StrokeKind::Inside);
                        if is_dir && rr.width() > 70.0 && rr.height() > 50.0 {
                            let inner = egui::Rect::from_min_max(rr.min + Vec2::new(3.0, 18.0), rr.max - Vec2::new(3.0, 3.0));
                            let sub_prefix = format!("{full}/");
                            let mut sub: BTreeMap<String, (f32, bool)> = BTreeMap::new();
                            for (f, n) in &t.files {
                                if let Some(rest) = f.strip_prefix(&sub_prefix) {
                                    let (h, d) = match rest.split_once('/') { Some((h, _)) => (h.to_string(), true), None => (rest.to_string(), false) };
                                    let e = sub.entry(h).or_insert((0.0, d));
                                    e.0 += *n as f32;
                                }
                            }
                            let mut items2: Vec<(String, f32)> = sub.iter().map(|(k, v)| (k.clone(), v.0)).collect();
                            items2.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                            for (n2, r2) in squarify(&items2, inner) {
                                let r2 = r2.shrink(1.0);
                                let d2 = sub[&n2].1;
                                p.rect_filled(r2, 2, if d2 { theme::PANEL_DARK() } else { theme::ROW_HOVER() });
                                p.rect_stroke(r2, 2, Stroke::new(1.0, theme::LINE()), egui::StrokeKind::Inside);
                                if r2.width() > 34.0 && r2.height() > 14.0 {
                                    p.with_clip_rect(r2.shrink(2.0)).text(r2.left_top() + Vec2::new(4.0, 3.0), Align2::LEFT_TOP, if d2 { format!("{n2}/") } else { n2.clone() }, FontId::proportional(10.5), theme::TEXT_DIM());
                                }
                            }
                        }
                        if rr.width() > 40.0 && rr.height() > 16.0 {
                            let label = if is_dir { format!("{name}/") } else { name.clone() };
                            let clip = p.with_clip_rect(rr.shrink(3.0));
                            clip.text(rr.left_top() + Vec2::new(5.0, 4.0), Align2::LEFT_TOP, label, FontId::proportional(11.5), theme::TEXT());
                            if rr.height() > 34.0 && !is_dir {
                                clip.text(rr.center(), Align2::CENTER_CENTER, format!("{}", w as u32), FontId::proportional(11.0), theme::TEXT_FAINT());
                            }
                        }
                        let resp = resp.on_hover_text(trs(format!("{full}\n{} changes", w as u32), format!("{full}\n{} Änderungen", w as u32)));
                        if resp.clicked() {
                            if is_dir {
                                t.folder = full.clone();
                            } else {
                                t.sel = Some(full.clone());
                                let log = git::run(&t.path, &["log", "-100", "--format=%s%x1f%an%x1f%cr", "--", &full]).stdout;
                                let commits = log.lines().filter_map(|l| { let f: Vec<&str> = l.split('\x1f').collect(); (f.len() >= 3).then(|| [f[0].to_string(), f[1].to_string(), rel(f[2])]) }).collect();
                                let sl = git::run(&t.path, &["shortlog", "-sn", "HEAD", "--", &full]).stdout;
                                let authors = sl.lines().filter_map(|l| { let l = l.trim(); let (n, a) = l.split_once('\t')?; Some((a.to_string(), n.trim().parse().ok()?)) }).collect();
                                t.detail = Some((full.clone(), commits, authors));
                            }
                        }
                    }
                    ui.vertical(|ui| {
                        ui.set_width(side);
                        match &t.detail {
                            None => {
                                ui.label(RichText::new(tr("Click a file to see its commits and authors.", "Klicke eine Datei an, um ihre Commits und Autoren zu sehen.")).color(theme::TEXT_DIM()));
                            }
                            Some((f, commits, authors)) => {
                                let (file, dir) = f.rsplit_once('/').map(|(d, f)| (f, d)).unwrap_or((f.as_str(), ""));
                                ui.label(RichText::new(file).font(FontId::new(14.0, theme::semibold())));
                                ui.label(RichText::new(dir).size(11.5).color(theme::TEXT_DIM()));
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut t.detail_tab, 0, tr("Commits", "Commits"));
                                    ui.selectable_value(&mut t.detail_tab, 1, tr("Authors", "Autoren"));
                                });
                                egui::ScrollArea::vertical().id_salt("tm_side").show(ui, |ui| {
                                    if t.detail_tab == 0 {
                                        for c in commits {
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new(short(&c[0], 34)).size(12.0)).on_hover_text(format!("{}\n{} · {}", c[0], c[1], c[2]));
                                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(&c[2]).size(11.0).color(theme::TEXT_FAINT())));
                                            });
                                        }
                                    } else {
                                        for (a, n) in authors {
                                            ui.horizontal(|ui| {
                                                ui.label(a);
                                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(n.to_string()));
                                            });
                                        }
                                    }
                                });
                            }
                        }
                    });
                });
            });
        if reload {
            t.load();
        }
        if open {
            self.treemap = Some(t);
        }
    }

    // ---------------------------------------------------------------- benchmark

    fn bench_window(&mut self, ctx: &egui::Context) {
        let Some(mut b) = self.bench.take() else { return };
        if let Some(rx) = &b.rx {
            while let Ok(r) = rx.try_recv() {
                b.results.push(r);
            }
            if b.results.len() >= BENCH.len() {
                b.rx = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        let mut open = true;
        let mut start = false;
        egui::Window::new(RichText::new(tr("Performance Benchmark", "Leistungstest")).font(FontId::new(14.0, theme::semibold()))).id(Id::unique("bench")).open(&mut open).collapsible(false).resizable(false).default_width(560.0).show(ctx, |ui| {
            ui.label(RichText::new(tr("Times the Git commands this app runs most, on the current repository.", "Misst die Git-Befehle, die diese App am häufigsten ausführt, im aktuellen Repository.")).size(12.0).color(theme::TEXT_DIM()));
            ui.add_space(6.0);
            egui::Grid::new("bench_grid").num_columns(3).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                for (i, (label_en, label_de, args)) in BENCH.iter().enumerate() {
                    ui.label(tr(label_en, label_de));
                    ui.label(RichText::new(format!("git {}", args.join(" "))).monospace().size(11.0).color(theme::TEXT_FAINT()));
                    match b.results.get(i) {
                        Some(ms) => ui.label(RichText::new(format!("{ms} ms")).color(if *ms > 1000 { theme::ORANGE() } else { theme::GREEN() })),
                        None if b.rx.is_some() => ui.spinner(),
                        None => ui.label("–"),
                    };
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(b.rx.is_none(), egui::Button::new(tr("Run", "Starten"))).clicked() {
                    start = true;
                }
                if !b.results.is_empty() && b.rx.is_none() {
                    let total: u128 = b.results.iter().sum();
                    ui.label(trs(format!("Total: {total} ms"), format!("Gesamt: {total} ms")));
                }
            });
        });
        if start {
            b.results.clear();
            let (tx, rx) = channel();
            let p = b.path.clone();
            std::thread::spawn(move || {
                for (_, _, args) in BENCH.iter() {
                    let t0 = std::time::Instant::now();
                    let _ = git::run(&p, args);
                    let _ = tx.send(t0.elapsed().as_millis());
                }
            });
            b.rx = Some(rx);
        }
        if open {
            self.bench = Some(b);
        }
    }
}

pub const BENCH: [(&str, &str, &[&str]); 6] = [
    ("Status", "Status", &["status", "--porcelain=v1", "-uall"]),
    ("History (3000 commits)", "Verlauf (3000 Commits)", &["log", "--all", "-n3000", "--format=%H%x1f%P%x1f%an%x1f%s"]),
    ("References", "Referenzen", &["for-each-ref"]),
    ("Commit count", "Anzahl Commits", &["rev-list", "--count", "HEAD"]),
    ("Working-tree diff", "Unterschiede im Arbeitsordner", &["diff", "--stat"]),
    ("Stashes", "Stashes", &["stash", "list"]),
];

pub struct Bench {
    pub path: PathBuf,
    pub results: Vec<u128>,
    pub rx: Option<Receiver<u128>>,
}
