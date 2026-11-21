//! More dialogs and settings:
//! - Create Branch (check out after create; local changes: don't change / stash and reapply /
//!   discard) and Create Tag (annotated, push right away),
//! - Settings for This Repository (user, main branch, no fast-forward, tab width, commit
//!   template, Signed-off-by, issue tracker links, custom commands),
//! - application preferences that change behaviour (commit sorting, automatic fetch, push on
//!   commit, subject length indicator, merge and diff tools, verbose output…),
//! - custom commands (in menus and context menus), keyboard shortcuts and About.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, RichText, Vec2};

use super::App;
use crate::git::JobSpec;
use crate::i18n::{tr, trs};
use crate::theme;

// ---------------------------------------------------------------------------------------------
// Settings data

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CustomCmd {
    pub name: String,
    /// Command line; `git …` runs Git, anything else runs in the system shell. Placeholders:
    /// ${repo}, ${branch} (current), ${ref} (the clicked branch/tag), ${sha}, ${file}.
    pub command: String,
    /// 0 repository, 1 commit, 2 branch
    #[serde(default)]
    pub target: u8,
    /// Ask before running.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct IssueRule {
    pub name: String,
    pub regex: String,
    pub url: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct RepoSettings {
    #[serde(default)]
    pub main_branch: String,
    #[serde(default = "four")]
    pub tab_width: usize,
    #[serde(default)]
    pub signoff: bool,
    #[serde(default)]
    pub issue_rules: Vec<IssueRule>,
    #[serde(default)]
    pub custom: Vec<CustomCmd>,
}

fn four() -> usize {
    4
}

impl Default for RepoSettings {
    fn default() -> Self {
        RepoSettings { main_branch: String::new(), tab_width: 4, signoff: false, issue_rules: vec![], custom: vec![] }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub light: bool,
    #[serde(default)]
    pub layout_vertical: bool,
    #[serde(default = "one")]
    pub zoom: f32,
    #[serde(default = "diff_font")]
    pub diff_font: f32,
    #[serde(default)]
    pub topo: bool,
    #[serde(default = "yes")]
    pub auto_fetch: bool,
    #[serde(default = "ten")]
    pub fetch_minutes: u32,
    #[serde(default)]
    pub fetch_tags: bool,
    #[serde(default = "yes")]
    pub tab_indicator: bool,
    #[serde(default)]
    pub push_on_commit: bool,
    #[serde(default = "yes")]
    pub compact_labels: bool,
    #[serde(default = "dash")]
    pub space_char: String,
    #[serde(default = "low")]
    pub subject_low: usize,
    #[serde(default = "high")]
    pub subject_high: usize,
    #[serde(default = "guide")]
    pub page_guide: usize,
    #[serde(default)]
    pub verbose: bool,
    #[serde(default = "yes")]
    pub issue_links: bool,
    /// Preferences > Updates: check GitHub for a new version once a day.
    #[serde(default = "yes")]
    pub auto_update: bool,
    #[serde(default)]
    pub last_update_check: u64,
}

fn one() -> f32 {
    1.0
}
fn diff_font() -> f32 {
    12.5
}
fn yes() -> bool {
    true
}
fn ten() -> u32 {
    10
}
fn dash() -> String {
    "-".into()
}
fn low() -> usize {
    50
}
fn high() -> usize {
    70
}
fn guide() -> usize {
    72
}

impl Default for Prefs {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

/// Tab width used when drawing diffs (Settings for This Repository > Diff View).
static TAB_WIDTH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(4);
static DIFF_FONT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(125);

pub fn tab_width() -> usize {
    TAB_WIDTH.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn diff_font_size() -> f32 {
    DIFF_FONT.load(std::sync::atomic::Ordering::Relaxed) as f32 / 10.0
}
pub fn set_diff_view(tab: usize, font: f32) {
    TAB_WIDTH.store(tab.clamp(1, 16), std::sync::atomic::Ordering::Relaxed);
    DIFF_FONT.store((font.clamp(8.0, 24.0) * 10.0) as u32, std::sync::atomic::Ordering::Relaxed);
}

/// Links found in a commit message by the repository's issue tracker rules.
pub fn issue_links(rules: &[IssueRule], msg: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for r in rules {
        if r.regex.trim().is_empty() || r.url.trim().is_empty() {
            continue;
        }
        for (text, groups) in simple_regex_find(&r.regex, msg) {
            let mut url = r.url.clone();
            for (i, g) in groups.iter().enumerate() {
                url = url.replace(&format!("${}", i + 1), g);
            }
            if !out.iter().any(|(t, _): &(String, String)| *t == text) {
                out.push((text, url));
            }
        }
    }
    out
}

/// A small regex subset good enough for issue keys: literals, `\d`, `\w`, `[A-Z]`-style
/// classes, `+`, `*`, `?` and one level of capture groups. Returns (match, groups).
fn simple_regex_find(pattern: &str, text: &str) -> Vec<(String, Vec<String>)> {
    #[derive(Clone, Debug)]
    enum Atom {
        Lit(char),
        Digit,
        Word,
        Any,
        Class(Vec<(char, char)>, bool),
        Open,
        Close,
    }
    #[derive(Clone, Debug)]
    struct Item {
        atom: Atom,
        min: usize,
        max: usize,
    }
    let mut items: Vec<Item> = Vec::new();
    let cs: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let atom = match cs[i] {
            '\\' if i + 1 < cs.len() => {
                i += 1;
                match cs[i] {
                    'd' => Atom::Digit,
                    'w' => Atom::Word,
                    c => Atom::Lit(c),
                }
            }
            '.' => Atom::Any,
            '(' => Atom::Open,
            ')' => Atom::Close,
            '[' => {
                let mut ranges = Vec::new();
                let mut neg = false;
                i += 1;
                if i < cs.len() && cs[i] == '^' {
                    neg = true;
                    i += 1;
                }
                while i < cs.len() && cs[i] != ']' {
                    let a = if cs[i] == '\\' && i + 1 < cs.len() { i += 1; cs[i] } else { cs[i] };
                    if i + 2 < cs.len() && cs[i + 1] == '-' && cs[i + 2] != ']' {
                        ranges.push((a, cs[i + 2]));
                        i += 3;
                    } else {
                        if a == 'd' && i > 0 && cs[i - 1] == '\\' {
                            ranges.push(('0', '9'));
                        } else {
                            ranges.push((a, a));
                        }
                        i += 1;
                    }
                }
                Atom::Class(ranges, neg)
            }
            '^' | '$' => {
                i += 1;
                continue;
            }
            c => Atom::Lit(c),
        };
        i += 1;
        let (mut min, mut max) = (1, 1);
        if i < cs.len() {
            match cs[i] {
                '+' => {
                    max = usize::MAX;
                    i += 1;
                }
                '*' => {
                    min = 0;
                    max = usize::MAX;
                    i += 1;
                }
                '?' => {
                    min = 0;
                    i += 1;
                }
                _ => {}
            }
        }
        items.push(Item { atom, min, max });
    }
    fn matches(a: &Atom, c: char) -> bool {
        match a {
            Atom::Lit(l) => *l == c,
            Atom::Digit => c.is_ascii_digit(),
            Atom::Word => c.is_alphanumeric() || c == '_',
            Atom::Any => c != '\n',
            Atom::Class(r, neg) => r.iter().any(|(a, b)| *a <= c && c <= *b) != *neg,
            _ => false,
        }
    }
    fn go(items: &[Item], k: usize, t: &[char], pos: usize, groups: &mut Vec<(usize, usize)>, open: &mut Vec<usize>) -> Option<usize> {
        if k == items.len() {
            return Some(pos);
        }
        let it = &items[k];
        match it.atom {
            Atom::Open => {
                open.push(pos);
                let r = go(items, k + 1, t, pos, groups, open);
                if r.is_none() {
                    open.pop();
                }
                r
            }
            Atom::Close => {
                let start = open.pop().unwrap_or(pos);
                groups.push((start, pos));
                let r = go(items, k + 1, t, pos, groups, open);
                if r.is_none() {
                    groups.pop();
                    open.push(start);
                }
                r
            }
            _ => {
                let mut n = 0;
                while n < it.max && pos + n < t.len() && matches(&it.atom, t[pos + n]) {
                    n += 1;
                }
                loop {
                    if n >= it.min {
                        if let Some(e) = go(items, k + 1, t, pos + n, groups, open) {
                            return Some(e);
                        }
                    }
                    if n == 0 {
                        return None;
                    }
                    n -= 1;
                }
            }
        }
    }
    let t: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut s = 0;
    while s < t.len() && out.len() < 50 {
        let mut groups = Vec::new();
        let mut open = Vec::new();
        if let Some(e) = go(&items, 0, &t, s, &mut groups, &mut open) {
            if e > s {
                let g = groups.iter().map(|(a, b)| t[*a..*b].iter().collect()).collect();
                out.push((t[s..e].iter().collect(), g));
                s = e;
                continue;
            }
        }
        s += 1;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Dialog state

pub struct BranchDialog {
    pub at: String,
    pub name: String,
    pub checkout: bool,
    pub local: u8,
    pub tag: bool,
    pub message: String,
    pub push: bool,
}

pub struct RepoSettingsDialog {
    pub path: PathBuf,
    pub tab: usize,
    pub use_global: bool,
    pub name: String,
    pub email: String,
    pub no_ff: bool,
    pub template_global: bool,
    pub template: String,
    pub s: RepoSettings,
    pub rule_sel: usize,
    pub test: String,
    pub cmd_sel: usize,
}

/// Quotes one argument for the small shell script Git runs for a command sequence.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl App {
    fn repo_key(&self) -> Option<String> {
        self.tabs.get(self.active).map(|r| r.path.display().to_string())
    }

    pub(super) fn repo_settings(&self) -> RepoSettings {
        self.repo_key().and_then(|k| self.repo_cfg.get(&k).cloned()).unwrap_or_default()
    }

    /// Runs several git commands one after the other (stopping at the first failure) as one
    /// visible job, through Git's own shell: `git -c alias.yagui-seq='!…' yagui-seq`.
    pub(super) fn quick_sequence(&mut self, title: String, steps: Vec<Vec<String>>, ctx: &egui::Context) {
        let Some(r) = self.repo() else { return };
        let script = steps.iter().map(|s| format!("git {}", s.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join(" && ");
        let display = steps.iter().map(|s| format!("git {}", s.iter().map(|a| crate::catalog::quote_for_display(a)).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join("\n");
        let args = vec!["-c".into(), format!("alias.yagui-seq=!{script}"), "yagui-seq".into()];
        let cwd = Some(r.path.clone());
        self.spawn(JobSpec { title, program: "git".into(), args, display, cwd, stdin: None, env: vec![], changes_repo: true, detached: false, automatic: false }, ctx);
    }

    pub(super) fn open_branch_dialog(&mut self, at: Option<String>, tag: bool) {
        let at = at.unwrap_or_else(|| self.tabs.get(self.active).map(|r| r.head.clone()).filter(|h| !h.is_empty()).unwrap_or_else(|| "HEAD".into()));
        self.branch_dialog = Some(BranchDialog { at, name: String::new(), checkout: true, local: 0, tag, message: String::new(), push: false });
    }

    pub(super) fn extra_dialogs(&mut self, ctx: &egui::Context) {
        self.branch_tag_dialog(ctx);
        self.repo_settings_dialog(ctx);
        self.shortcuts_window(ctx);
        self.about_window(ctx);
        self.custom_confirm(ctx);
        self.stash_rename_dialog(ctx);
        self.stale_dialog(ctx);
    }

    /// Local branches that are merged into the main branch or whose server branch is gone.
    pub(super) fn open_stale(&mut self) {
        let Some(r) = self.tabs.get(self.active) else { return };
        let main = {
            let m = self.repo_settings().main_branch;
            if !m.is_empty() { m } else if r.local.iter().any(|b| b.name == "main") { "main".into() } else { "master".into() }
        };
        let merged: Vec<String> = crate::git::run(&r.path, &["branch", "--merged", &main, "--format=%(refname:short)"]).stdout.lines().map(String::from).collect();
        let mut list = Vec::new();
        for b in &r.local {
            if b.is_head || b.name == main {
                continue;
            }
            let gone = b.track.contains("gone");
            let is_merged = merged.contains(&b.name);
            if gone || is_merged {
                let why = match (is_merged, gone) {
                    (true, true) => trs(format!("merged into {main}, server branch deleted"), format!("in {main} gemergt, Server-Branch gelöscht")),
                    (true, false) => trs(format!("merged into {main}"), format!("in {main} gemergt")),
                    _ => tr("server branch deleted", "Server-Branch gelöscht").to_string(),
                };
                list.push((b.name.clone(), true, why));
            }
        }
        self.stale = Some(list);
    }

    fn stale_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut list) = self.stale.take() else { return };
        let mut keep = true;
        let mut delete: Option<bool> = None;
        egui::Window::new(RichText::new(tr("Stale Local Branches", "Veraltete lokale Branches")).font(FontId::new(14.5, theme::semibold()))).id(Id::unique("stale")).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.set_width(460.0);
            if list.is_empty() {
                ui.label(tr("No stale branches – nothing to clean up.", "Keine veralteten Branches – nichts aufzuräumen."));
            } else {
                ui.label(RichText::new(tr("These branches are done. Pick the ones to delete.", "Diese Branches sind erledigt. Wähle die, die gelöscht werden sollen.")).size(12.5).color(theme::TEXT_DIM()));
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (name, on, why) in list.iter_mut() {
                        ui.horizontal(|ui| {
                            ui.checkbox(on, name.as_str());
                            ui.label(RichText::new(why.as_str()).size(11.5).color(theme::TEXT_FAINT()));
                        });
                    }
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let n = list.iter().filter(|x| x.1).count();
                if ui.add_enabled(n > 0, egui::Button::new(trs(format!("Delete {n}"), format!("{n} löschen")))).on_hover_text("git branch -d …").clicked() {
                    delete = Some(false);
                }
                if ui.add_enabled(n > 0, egui::Button::new(tr("Force delete", "Erzwungen löschen"))).on_hover_text(tr("git branch -D … – also deletes branches with commits that exist nowhere else", "git branch -D … – löscht auch Branches mit Commits, die es sonst nirgends gibt")).clicked() {
                    delete = Some(true);
                }
                if ui.button(tr("Close", "Schließen")).clicked() {
                    keep = false;
                }
            });
        });
        if let Some(force) = delete {
            let mut args = vec!["branch".to_string(), if force { "-D".into() } else { "-d".into() }];
            args.extend(list.iter().filter(|x| x.1).map(|x| x.0.clone()));
            self.quick(tr("Delete stale branches", "Veraltete Branches löschen").into(), args, ctx);
            return;
        }
        if keep && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.stale = Some(list);
        }
    }

    fn stash_rename_dialog(&mut self, ctx: &egui::Context) {
        let Some((name, mut text)) = self.stash_rename.take() else { return };
        let mut keep = true;
        let mut go = false;
        egui::Window::new(tr("Rename Stash", "Stash umbenennen")).id(Id::unique("stash_rename")).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(RichText::new(&name).monospace());
            ui.add(egui::TextEdit::singleline(&mut text).hint_text(tr("New description", "Neue Beschreibung")).desired_width(320.0)).request_focus();
            ui.horizontal(|ui| {
                if ui.add_enabled(!text.trim().is_empty(), egui::Button::new(tr("Rename", "Umbenennen"))).clicked() {
                    go = true;
                }
                if ui.button(tr("Cancel", "Abbrechen")).clicked() {
                    keep = false;
                }
            });
        });
        if go {
            if let Some(r) = self.tabs.get(self.active) {
                let sha = crate::git::run(&r.path, &["rev-parse", &name]).stdout.trim().to_string();
                let n: usize = name.trim_start_matches("stash@{").trim_end_matches('}').parse().unwrap_or(0);
                if !sha.is_empty() {
                    self.quick_sequence(tr("Rename stash", "Stash umbenennen").into(), vec![vec!["stash".into(), "store".into(), "-m".into(), text.trim().to_string(), sha], vec!["stash".into(), "drop".into(), format!("stash@{{{}}}", n + 1)]], ctx);
                }
            }
            return;
        }
        if keep && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.stash_rename = Some((name, text));
        }
    }

    fn branch_tag_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.branch_dialog.take() else { return };
        let mut open = true;
        let mut cancel = false;
        let mut run = false;
        let space = self.p.space_char.clone();
        let title = if d.tag { tr("Create Tag", "Tag erstellen") } else { tr("Create Branch", "Branch erstellen") };
        let has_changes = self.tabs.get(self.active).map(|r| !r.status.is_empty()).unwrap_or(false);
        let remote = self.tabs.get(self.active).and_then(|r| r.remotes.first().map(|x| x.0.clone())).unwrap_or_else(|| "origin".into());
        egui::Window::new(RichText::new(title).font(FontId::new(14.5, theme::semibold())))
            .id(Id::unique("branch_tag_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                ui.set_width(480.0);
                ui.label(RichText::new(if d.tag { tr("Create an annotated tag at the selected point.", "Erstellt einen Tag mit Beschreibung am gewählten Punkt.") } else { tr("Create a branch at the selected point.", "Erstellt einen Branch am gewählten Punkt.") }).size(12.5).color(theme::TEXT_DIM()));
                ui.add_space(10.0);
                egui::Grid::new("bt_grid").num_columns(2).spacing([12.0, 8.0]).min_col_width(120.0).show(ui, |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(if d.tag { tr("Create tag at:", "Tag erstellen bei:") } else { tr("Create branch at:", "Branch erstellen bei:") }).color(theme::TEXT_DIM())));
                    ui.add(egui::TextEdit::singleline(&mut d.at).desired_width(320.0).font(egui::TextStyle::Monospace));
                    ui.end_row();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(if d.tag { tr("Tag name:", "Tag-Name:") } else { tr("Branch name:", "Branch-Name:") }).color(theme::TEXT_DIM())));
                    let r = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(320.0));
                    if d.name.is_empty() {
                        r.request_focus();
                    }
                    if !space.is_empty() && d.name.contains(' ') {
                        d.name = d.name.replace(' ', &space);
                    }
                    ui.end_row();
                    if d.tag {
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| ui.label(RichText::new(tr("Message:", "Beschreibung:")).color(theme::TEXT_DIM())));
                        ui.add(egui::TextEdit::multiline(&mut d.message).desired_rows(3).desired_width(320.0).hint_text(tr("optional – with a message the tag is annotated", "optional – mit Beschreibung wird der Tag „annotiert“")));
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut d.push, trs(format!("Push to '{remote}'"), format!("Nach '{remote}' pushen")));
                        ui.end_row();
                    } else {
                        ui.label("");
                        ui.checkbox(&mut d.checkout, tr("Check out after create", "Nach dem Erstellen auschecken"));
                        ui.end_row();
                        if d.checkout {
                            ui.with_layout(Layout::right_to_left(Align::TOP), |ui| ui.label(RichText::new(tr("Local changes:", "Lokale Änderungen:")).color(theme::TEXT_DIM())));
                            ui.vertical(|ui| {
                                ui.add_enabled_ui(has_changes, |ui| {
                                    ui.radio_value(&mut d.local, 0, tr("Don't change", "Nicht ändern"));
                                    ui.radio_value(&mut d.local, 1, tr("Stash and reapply", "Zwischenlagern und wieder anwenden"));
                                    ui.radio_value(&mut d.local, 2, tr("Discard", "Verwerfen"));
                                });
                                if !has_changes {
                                    ui.label(RichText::new(tr("(there are no local changes)", "(es gibt keine lokalen Änderungen)")).size(11.5).color(theme::TEXT_FAINT()));
                                }
                            });
                            ui.end_row();
                        }
                        ui.label("");
                        ui.label(RichText::new(tr("Use '/' as a path separator to create folders", "Mit '/' im Namen entstehen Ordner")).size(11.5).color(theme::TEXT_FAINT()));
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                let steps = branch_steps(&d, has_changes, &remote);
                let line = steps.iter().map(|s| format!("git {}", s.join(" "))).collect::<Vec<_>>().join("  &&  ");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&line).monospace().size(10.5).color(theme::TEXT_FAINT())).on_hover_text(tr("What will run", "Was ausgeführt wird"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let ok = !d.name.trim().is_empty() && !d.at.trim().is_empty();
                        if ui.add_enabled(ok, egui::Button::new(RichText::new(tr("Create", "Erstellen")).color(Color32::WHITE)).fill(theme::ACCENT_DARK()).min_size(Vec2::new(86.0, 26.0))).clicked() || (ok && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)) && !d.tag {
                            run = true;
                        }
                        if ui.button(tr("Cancel", "Abbrechen")).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if cancel {
            open = false;
        }
        if run {
            let has = self.tabs.get(self.active).map(|r| !r.status.is_empty()).unwrap_or(false);
            let steps = branch_steps(&d, has, &remote);
            let title = if d.tag { trs(format!("Create tag {}", d.name), format!("Tag {} erstellen", d.name)) } else { trs(format!("Create branch {}", d.name), format!("Branch {} erstellen", d.name)) };
            if steps.len() == 1 {
                self.quick(title, steps[0].clone(), ctx);
            } else {
                self.quick_sequence(title, steps, ctx);
            }
            return;
        }
        if open && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.branch_dialog = Some(d);
        }
    }

    // ------------------------------------------------------------------ repository settings

    pub(super) fn open_repo_settings(&mut self) {
        let Some(r) = self.tabs.get(self.active) else { return };
        let p = r.path.clone();
        let get = |scope: &str, k: &str| crate::git::run(&p, &["config", scope, "--get", k]).stdout.trim().to_string();
        let name = get("--local", "user.name");
        let email = get("--local", "user.email");
        let use_global = name.is_empty() && email.is_empty();
        let (name, email) = if use_global { (get("--global", "user.name"), get("--global", "user.email")) } else { (name, email) };
        let template = get("--local", "commit.template");
        self.repo_dialog = Some(RepoSettingsDialog {
            path: p.clone(),
            tab: 0,
            use_global,
            name,
            email,
            no_ff: get("--local", "merge.ff") == "false",
            template_global: template.is_empty(),
            template,
            s: self.repo_cfg.get(&p.display().to_string()).cloned().unwrap_or_default(),
            rule_sel: 0,
            test: String::new(),
            cmd_sel: 0,
        });
    }

    fn repo_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.repo_dialog.take() else { return };
        let mut open = true;
        let mut cancel = false;
        let mut save = false;
        let branches: Vec<String> = self.tabs.iter().find(|t| t.path == d.path).map(|t| t.local.iter().map(|b| b.name.clone()).collect()).unwrap_or_default();
        egui::Window::new(RichText::new(tr("Repository Settings", "Repository-Einstellungen")).font(FontId::new(14.5, theme::semibold())))
            .id(Id::unique("repo_settings"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                ui.set_width(560.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    for (i, t) in [tr("General", "Allgemein"), tr("Commit", "Commit"), tr("Issue Tracker", "Issue-Tracker"), tr("Custom Commands", "Eigene Befehle")].iter().enumerate() {
                        let s = d.tab == i;
                        let r = ui.add(egui::Button::new(RichText::new(*t).font(FontId::new(13.0, theme::semibold())).color(if s { theme::ACCENT() } else { theme::TEXT_DIM() })).frame(false));
                        if s {
                            ui.painter().line_segment([r.rect.left_bottom() + Vec2::new(0.0, 4.0), r.rect.right_bottom() + Vec2::new(0.0, 4.0)], egui::Stroke::new(2.0, theme::ACCENT()));
                        }
                        if r.clicked() {
                            d.tab = i;
                        }
                    }
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
                match d.tab {
                    0 => {
                        ui.label(RichText::new(tr("User Information", "Benutzer")).font(FontId::new(13.0, theme::semibold())));
                        ui.checkbox(&mut d.use_global, tr("Use global git credentials", "Globale Git-Angaben verwenden")).on_hover_text(tr("Off: this repository commits with its own name and e-mail", "Aus: dieses Repository committet mit eigenem Namen und eigener E-Mail"));
                        ui.add_enabled_ui(!d.use_global, |ui| {
                            ui.add(egui::TextEdit::singleline(&mut d.name).hint_text(tr("Name", "Name")).desired_width(360.0));
                            ui.add(egui::TextEdit::singleline(&mut d.email).hint_text(tr("E-mail", "E-Mail")).desired_width(360.0));
                        });
                        ui.add_space(8.0);
                        ui.label(RichText::new(tr("Lean Branching", "Schlanke Branches")).font(FontId::new(13.0, theme::semibold())));
                        ui.horizontal(|ui| {
                            ui.label(tr("Main Branch:", "Haupt-Branch:"));
                            let shown = if d.s.main_branch.is_empty() { tr("default", "Standard").to_string() } else { d.s.main_branch.clone() };
                            egui::ComboBox::from_id_salt("main_branch").width(220.0).selected_text(shown).show_ui(ui, |ui| {
                                ui.selectable_value(&mut d.s.main_branch, String::new(), tr("default (main / master)", "Standard (main / master)"));
                                for b in &branches {
                                    ui.selectable_value(&mut d.s.main_branch, b.clone(), b);
                                }
                            });
                        });
                        ui.checkbox(&mut d.no_ff, tr("No fast-forward on merge", "Beim Merge immer einen Merge-Commit anlegen")).on_hover_text("git config merge.ff false");
                        ui.add_space(8.0);
                        ui.label(RichText::new(tr("Diff View", "Unterschiede")).font(FontId::new(13.0, theme::semibold())));
                        ui.horizontal(|ui| {
                            ui.label(tr("Tab Width:", "Tabulatorbreite:"));
                            ui.add(egui::DragValue::new(&mut d.s.tab_width).range(1..=16));
                        });
                    }
                    1 => {
                        ui.label(RichText::new(tr("Commit Template", "Commit-Vorlage")).font(FontId::new(13.0, theme::semibold())));
                        ui.checkbox(&mut d.template_global, tr("Use global git configuration file", "Globale Git-Einstellung verwenden"));
                        ui.add_enabled_ui(!d.template_global, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut d.template).hint_text(tr("path to a template file", "Pfad zu einer Vorlagendatei")).desired_width(400.0));
                                if ui.button("…").clicked() {
                                    if let Some(f) = rfd::FileDialog::new().set_directory(&d.path).pick_file() {
                                        d.template = f.display().to_string();
                                    }
                                }
                            });
                        });
                        ui.add_space(6.0);
                        ui.checkbox(&mut d.s.signoff, tr("Add Signed-off-by line at the end of the commit message", "„Signed-off-by“-Zeile an jede Commit-Beschreibung anhängen"));
                    }
                    2 => {
                        ui.label(RichText::new(tr("Turns issue numbers in commit messages into links (e.g. JIRA-123 or #45).", "Macht Issue-Nummern in Commit-Beschreibungen zu Links (z. B. JIRA-123 oder #45).")).size(12.0).color(theme::TEXT_DIM()));
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                egui::Frame::new().fill(theme::PANEL_DARK()).stroke(egui::Stroke::new(1.0, theme::LINE())).show(ui, |ui| {
                                    ui.set_width(150.0);
                                    ui.set_min_height(160.0);
                                    for (i, r) in d.s.issue_rules.iter().enumerate() {
                                        if ui.selectable_label(d.rule_sel == i, if r.name.is_empty() { tr("(unnamed)", "(ohne Namen)") } else { &r.name }).clicked() {
                                            d.rule_sel = i;
                                        }
                                    }
                                });
                                ui.horizontal(|ui| {
                                    let add = ui.small_button("+");
                                    egui::Popup::menu(&add).show(|ui| {
                                        for (n, re, url) in [("GitHub", "#(\\d+)", "https://github.com/OWNER/REPO/issues/$1"), ("GitLab", "#(\\d+)", "https://gitlab.com/OWNER/REPO/-/issues/$1"), ("Jira", "([A-Z]+-\\d+)", "https://YOURCOMPANY.atlassian.net/browse/$1"), (tr("Custom", "Eigene"), "", "")] {
                                            if ui.button(n).clicked() {
                                                d.s.issue_rules.push(IssueRule { name: n.into(), regex: re.into(), url: url.into() });
                                                d.rule_sel = d.s.issue_rules.len() - 1;
                                                ui.close();
                                            }
                                        }
                                    });
                                    if !d.s.issue_rules.is_empty() && ui.small_button("−").clicked() {
                                        d.s.issue_rules.remove(d.rule_sel.min(d.s.issue_rules.len() - 1));
                                        d.rule_sel = 0;
                                    }
                                });
                            });
                            ui.vertical(|ui| {
                                if let Some(r) = d.s.issue_rules.get_mut(d.rule_sel) {
                                    egui::Grid::new("issue_rule").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                                        ui.label(tr("Name:", "Name:"));
                                        ui.add(egui::TextEdit::singleline(&mut r.name).desired_width(300.0));
                                        ui.end_row();
                                        ui.label(tr("Regular expression:", "Suchmuster:"));
                                        ui.add(egui::TextEdit::singleline(&mut r.regex).desired_width(300.0).font(egui::TextStyle::Monospace));
                                        ui.end_row();
                                        ui.label(tr("Result URL:", "Ziel-Adresse:"));
                                        ui.add(egui::TextEdit::singleline(&mut r.url).desired_width(300.0).font(egui::TextStyle::Monospace));
                                        ui.end_row();
                                        ui.label("");
                                        ui.label(RichText::new(tr("Use $1, $2… for the parts in ( )", "Mit $1, $2… die Teile in ( ) einsetzen")).size(11.5).color(theme::TEXT_FAINT()));
                                        ui.end_row();
                                        ui.label(tr("Test message:", "Testtext:"));
                                        ui.add(egui::TextEdit::singleline(&mut d.test).desired_width(300.0).hint_text("Fix #42"));
                                        ui.end_row();
                                    });
                                    for (t, u) in issue_links(std::slice::from_ref(r), &d.test) {
                                        ui.label(RichText::new(format!("{t}  →  {u}")).size(12.0).color(theme::ACCENT()));
                                    }
                                } else {
                                    ui.label(RichText::new(tr("Add a rule with +", "Mit + eine Regel hinzufügen")).color(theme::TEXT_FAINT()));
                                }
                            });
                        });
                    }
                    _ => {
                        custom_cmds_editor(ui, &mut d.s.custom, &mut d.cmd_sel, "repo_cc");
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new(tr("Save", "Speichern")).color(Color32::WHITE)).fill(theme::ACCENT_DARK()).min_size(Vec2::new(86.0, 26.0))).clicked() {
                        save = true;
                    }
                    if ui.button(tr("Cancel", "Abbrechen")).clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            open = false;
        }
        if save {
            let p = d.path.clone();
            let cfg = |args: &[&str]| {
                let _ = crate::git::run(&p, args);
            };
            if d.use_global {
                cfg(&["config", "--local", "--unset", "user.name"]);
                cfg(&["config", "--local", "--unset", "user.email"]);
            } else {
                if !d.name.trim().is_empty() {
                    cfg(&["config", "--local", "user.name", d.name.trim()]);
                }
                if !d.email.trim().is_empty() {
                    cfg(&["config", "--local", "user.email", d.email.trim()]);
                }
            }
            if d.no_ff {
                cfg(&["config", "--local", "merge.ff", "false"]);
            } else {
                cfg(&["config", "--local", "--unset", "merge.ff"]);
            }
            if d.template_global || d.template.trim().is_empty() {
                cfg(&["config", "--local", "--unset", "commit.template"]);
            } else {
                cfg(&["config", "--local", "commit.template", d.template.trim()]);
            }
            self.repo_cfg.insert(p.display().to_string(), d.s.clone());
            self.apply_repo_view();
            self.toast = Some((tr("Repository settings saved.", "Repository-Einstellungen gespeichert.").into(), ctx.input(|i| i.time)));
            self.save();
            return;
        }
        if open && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.repo_dialog = Some(d);
        }
    }

    /// Applies the active repository's diff settings.
    pub(super) fn apply_repo_view(&self) {
        set_diff_view(self.repo_settings().tab_width, self.p.diff_font);
    }

    // ------------------------------------------------------------------ custom commands

    /// Custom commands for a target (0 repository, 1 commit, 2 branch): global then repository.
    pub(super) fn customs_for(&self, target: u8) -> Vec<(usize, CustomCmd)> {
        let mut all: Vec<CustomCmd> = self.custom_cmds.clone();
        all.extend(self.repo_settings().custom);
        all.into_iter().enumerate().filter(|(_, c)| c.target == target).collect()
    }

    pub(super) fn all_customs(&self) -> Vec<CustomCmd> {
        let mut all: Vec<CustomCmd> = self.custom_cmds.clone();
        all.extend(self.repo_settings().custom);
        all
    }

    /// Fills the placeholders and runs (or first confirms) a custom command.
    pub(super) fn run_custom(&mut self, i: usize, ctx_ref: String, ctx: &egui::Context, confirmed: bool) {
        let Some(cc) = self.all_customs().get(i).cloned() else { return };
        let Some(r) = self.tabs.get(self.active) else { return };
        if cc.confirm && !confirmed {
            self.custom_pending = Some((i, ctx_ref));
            return;
        }
        let sha = r.selected.and_then(|s| r.commits.get(s)).map(|c| c.sha.clone()).unwrap_or_default();
        let file = r.sel_file.as_ref().map(|f| f.0.clone()).unwrap_or_default();
        let line = cc
            .command
            .replace("${repo}", &r.path.display().to_string())
            .replace("${branch}", &r.head)
            .replace("${ref}", if ctx_ref.is_empty() { &r.head } else { &ctx_ref })
            .replace("${sha}", if ctx_ref.len() >= 7 && ctx_ref.chars().all(|c| c.is_ascii_hexdigit()) { &ctx_ref } else { &sha })
            .replace("${file}", &file);
        let cwd = Some(r.path.clone());
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("git ") {
            let args = crate::catalog::split_args(rest);
            self.spawn(JobSpec { title: cc.name.clone(), program: "git".into(), args, display: trimmed.to_string(), cwd, stdin: None, env: vec![], changes_repo: true, detached: false, automatic: false }, ctx);
        } else {
            let (prog, args) = if cfg!(windows) { ("cmd".to_string(), vec!["/C".to_string(), trimmed.to_string()]) } else { ("sh".to_string(), vec!["-c".to_string(), trimmed.to_string()]) };
            self.spawn(JobSpec { title: cc.name.clone(), program: prog, args, display: trimmed.to_string(), cwd, stdin: None, env: vec![], changes_repo: true, detached: false, automatic: false }, ctx);
        }
    }

    fn custom_confirm(&mut self, ctx: &egui::Context) {
        let Some((i, rf)) = self.custom_pending.take() else { return };
        let Some(cc) = self.all_customs().get(i).cloned() else { return };
        let mut keep = true;
        let mut go = false;
        egui::Window::new(&cc.name).id(Id::unique("custom_confirm")).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(tr("Run this command?", "Diesen Befehl ausführen?"));
            ui.label(RichText::new(&cc.command).monospace().size(12.0).color(theme::TEXT_DIM()));
            ui.horizontal(|ui| {
                if ui.button(tr("Run", "Ausführen")).clicked() {
                    go = true;
                }
                if ui.button(tr("Cancel", "Abbrechen")).clicked() {
                    keep = false;
                }
            });
        });
        if go {
            self.run_custom(i, rf, ctx, true);
        } else if keep && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.custom_pending = Some((i, rf));
        }
    }

    // ------------------------------------------------------------------ shortcuts & about

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }
        let mut open = true;
        egui::Window::new(RichText::new(tr("Keyboard Shortcuts", "Tastenkürzel")).font(FontId::new(14.5, theme::semibold()))).id(Id::unique("shortcuts")).open(&mut open).collapsible(false).resizable(false).default_width(460.0).show(ctx, |ui| {
            egui::Grid::new("sc_grid").num_columns(2).striped(true).spacing([24.0, 4.0]).show(ui, |ui| {
                for (k, en, de) in SHORTCUTS {
                    ui.label(RichText::new(*k).monospace().size(12.0));
                    ui.label(tr(en, de));
                    ui.end_row();
                }
            });
        });
        self.show_shortcuts = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        let mut open_updates = false;
        let git = crate::git::run(std::path::Path::new("."), &["--version"]).stdout;
        egui::Window::new(tr("About YAGUI", "Über YAGUI")).id(Id::unique("about")).open(&mut open).collapsible(false).resizable(false).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                let tex = ui.ctx().load_texture("about_icon", egui::ColorImage::from_rgba_unmultiplied([96, 96], &crate::appicon::icon_pixels(96)), egui::TextureOptions::LINEAR);
                ui.image((tex.id(), Vec2::splat(72.0)));
                ui.label(RichText::new("YAGUI").font(FontId::new(22.0, theme::semibold())));
                ui.label(RichText::new("Yet Another Git User Interface").size(13.0).color(theme::TEXT_DIM()));
                ui.label(RichText::new(format!("{} {}", tr("Version", "Version"), crate::version::VERSION)).color(theme::TEXT_DIM()));
                ui.label(RichText::new(format!("{} {} · {} · {}", tr("Commit", "Commit"), crate::version::COMMIT, crate::version::COMMIT_DATE, crate::version::platform())).size(11.5).color(theme::TEXT_FAINT()));
                ui.label(RichText::new(tr("A friendly Git client that shows every command it runs.", "Ein freundlicher Git-Client, der jeden ausgeführten Befehl zeigt.")).size(12.5));
                ui.label(RichText::new(git.trim()).size(11.5).color(theme::TEXT_FAINT()));
                ui.add_space(6.0);
                if ui.link(RichText::new(REPO_URL).size(12.0)).on_hover_text(tr("Source code on GitHub", "Quellcode auf GitHub")).clicked() {
                    super::auth::open_url(REPO_URL);
                }
                ui.add_space(4.0);
                if ui.button(tr("Check for Updates…", "Nach Updates suchen…")).clicked() {
                    open_updates = true;
                }
            });
        });
        self.show_about = open;
        if open_updates {
            self.upd.open = true;
            self.check_updates(false, ctx);
        }
    }
}

/// Where YAGUI's source code lives.
pub const REPO_URL: &str = "https://github.com/rkaradas/YAGUI";

pub const SHORTCUTS: &[(&str, &str, &str)] = &[
    ("Ctrl+T", "New tab (Repository Manager)", "Neuer Tab (Repository Manager)"),
    ("Ctrl+O", "Open repository", "Repository öffnen"),
    ("Ctrl+W", "Close tab", "Tab schließen"),
    ("Ctrl+Shift+T", "Restore closed tab", "Geschlossenen Tab wiederherstellen"),
    ("Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous tab", "Nächster / vorheriger Tab"),
    ("Ctrl+P", "Quick Launch (repositories and commands)", "Schnellstart (Repositories und Befehle)"),
    ("Ctrl+Shift+P", "Find a command", "Befehl suchen"),
    ("Ctrl+1 / Ctrl+2", "Local changes / all commits", "Lokale Änderungen / alle Commits"),
    ("Ctrl+H", "Show HEAD", "HEAD zeigen"),
    ("Ctrl+Shift+L", "Fetch", "Fetch"),
    ("Ctrl+Shift+U", "Push", "Push"),
    ("Ctrl+Shift+S", "Save stash", "Stash speichern"),
    ("Ctrl+Shift+B", "New branch", "Neuer Branch"),
    ("Ctrl+Enter", "Commit (in the message box)", "Committen (im Beschreibungsfeld)"),
    ("Ctrl+Alt+T", "Open in console", "In Konsole öffnen"),
    ("Ctrl+Alt+O", "Open in file explorer", "Im Datei-Explorer öffnen"),
    ("Ctrl+,", "Preferences", "Einstellungen"),
    ("Ctrl++ / Ctrl+- / Ctrl+0", "Zoom in / out / actual size", "Vergrößern / verkleinern / Originalgröße"),
    ("F5", "Refresh", "Neu laden"),
];

fn branch_steps(d: &BranchDialog, has_changes: bool, remote: &str) -> Vec<Vec<String>> {
    let name = d.name.trim().to_string();
    let at = d.at.trim().to_string();
    if d.tag {
        let mut t = vec!["tag".to_string()];
        if !d.message.trim().is_empty() {
            t.extend(["-a".into(), name.clone(), "-m".into(), d.message.trim().to_string()]);
        } else {
            t.push(name.clone());
        }
        t.push(at);
        let mut v = vec![t];
        if d.push {
            v.push(vec!["push".into(), remote.into(), format!("refs/tags/{name}")]);
        }
        return v;
    }
    if !d.checkout {
        return vec![vec!["branch".into(), name, at]];
    }
    match (has_changes, d.local) {
        (true, 1) => vec![
            vec!["stash".into(), "push".into(), "--include-untracked".into(), "-m".into(), format!("YAGUI: before switching to {name}")],
            vec!["switch".into(), "-c".into(), name, at],
            vec!["stash".into(), "pop".into()],
        ],
        (true, 2) => vec![vec!["switch".into(), "--discard-changes".into(), "-c".into(), name, at]],
        _ => vec![vec!["switch".into(), "-c".into(), name, at]],
    }
}

/// The list + form editor for custom commands (preferences and repository settings).
pub fn custom_cmds_editor(ui: &mut egui::Ui, list: &mut Vec<CustomCmd>, sel: &mut usize, salt: &str) {
    ui.label(RichText::new(tr("Your own commands, shown in the Commands menu and the matching right-click menus.", "Eigene Befehle, im Menü „Befehle“ und in den passenden Rechtsklick-Menüs.")).size(12.0).color(theme::TEXT_DIM()));
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            egui::Frame::new().fill(theme::PANEL_DARK()).stroke(egui::Stroke::new(1.0, theme::LINE())).show(ui, |ui| {
                ui.set_width(160.0);
                ui.set_min_height(170.0);
                for (i, c) in list.iter().enumerate() {
                    if ui.selectable_label(*sel == i, if c.name.is_empty() { tr("(unnamed)", "(ohne Namen)") } else { &c.name }).clicked() {
                        *sel = i;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.small_button("+").clicked() {
                    list.push(CustomCmd { name: tr("New command", "Neuer Befehl").into(), command: "git status".into(), target: 0, confirm: false });
                    *sel = list.len() - 1;
                }
                if !list.is_empty() && ui.small_button("−").clicked() {
                    list.remove((*sel).min(list.len() - 1));
                    *sel = 0;
                }
            });
        });
        ui.vertical(|ui| {
            if let Some(c) = list.get_mut(*sel) {
                egui::Grid::new((salt, "cc_form")).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(tr("Name:", "Name:"));
                    ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(300.0));
                    ui.end_row();
                    ui.label(tr("Target:", "Ziel:"));
                    egui::ComboBox::from_id_salt((salt, "cc_target")).selected_text(match c.target { 1 => tr("Commit", "Commit"), 2 => tr("Branch", "Branch"), _ => tr("Repository", "Repository") }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut c.target, 0, tr("Repository", "Repository"));
                        ui.selectable_value(&mut c.target, 1, tr("Commit", "Commit"));
                        ui.selectable_value(&mut c.target, 2, tr("Branch", "Branch"));
                    });
                    ui.end_row();
                    ui.label(tr("Command:", "Befehl:"));
                    ui.add(egui::TextEdit::multiline(&mut c.command).desired_rows(2).desired_width(300.0).font(egui::TextStyle::Monospace));
                    ui.end_row();
                    ui.label("");
                    ui.label(RichText::new("${repo} ${branch} ${ref} ${sha} ${file}").monospace().size(11.0).color(theme::TEXT_FAINT())).on_hover_text(tr("Placeholders: repository folder, current branch, clicked branch, selected commit, selected file", "Platzhalter: Repository-Ordner, aktueller Branch, angeklickter Branch, gewählter Commit, gewählte Datei"));
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut c.confirm, tr("Ask before running", "Vor dem Ausführen nachfragen"));
                    ui.end_row();
                });
            } else {
                ui.label(RichText::new(tr("Add a command with +", "Mit + einen Befehl hinzufügen")).color(theme::TEXT_FAINT()));
            }
        });
    });
}

pub type RepoCfg = BTreeMap<String, RepoSettings>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_rules_link_keys() {
        let rules = vec![IssueRule { name: "Jira".into(), regex: "([A-Z]+-\\d+)".into(), url: "https://x/browse/$1".into() }, IssueRule { name: "GH".into(), regex: "#(\\d+)".into(), url: "https://gh/issues/$1".into() }];
        let l = issue_links(&rules, "Fix ABC-12 and #7, see ABC-12");
        assert_eq!(l, vec![("ABC-12".to_string(), "https://x/browse/ABC-12".to_string()), ("#7".to_string(), "https://gh/issues/7".to_string())]);
    }

    #[test]
    fn branch_steps_stash_and_reapply() {
        let d = BranchDialog { at: "main".into(), name: "feature/x".into(), checkout: true, local: 1, tag: false, message: String::new(), push: false };
        let s = branch_steps(&d, true, "origin");
        assert_eq!(s.len(), 3);
        assert_eq!(s[1], vec!["switch", "-c", "feature/x", "main"]);
        let d = BranchDialog { at: "HEAD".into(), name: "v1".into(), checkout: true, local: 0, tag: true, message: "Release".into(), push: true };
        let s = branch_steps(&d, false, "origin");
        assert_eq!(s, vec![vec!["tag", "-a", "v1", "-m", "Release", "HEAD"], vec!["push", "origin", "refs/tags/v1"]]);
    }
}
