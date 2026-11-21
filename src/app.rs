//! The main window: toolbar, repository tabs, sidebar, history, local changes, the command
//! library and the activity log. Every git call the user triggers goes through a visible
//! command form or is listed in the activity log with its exact command line.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, RichText, Sense, Stroke, Vec2};

use crate::catalog::{self, Arg, Cat, Cmd, Danger, Kind, Value, CATS, P};
use crate::git::{self, Commit, CommitDetail, FileStatus, Job, JobSpec, JobState, RefInfo};
use crate::graph;
use crate::theme;

// ---------------------------------------------------------------------------------------------
// State

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Changes,
    Commits,
    Library,
    Console,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    Commit,
    Changes,
    Tree,
}

pub struct Repo {
    pub path: PathBuf,
    pub name: String,
    head: String,
    commits: Vec<Commit>,
    rows: Vec<graph::Row>,
    lanes: usize,
    local: Vec<RefInfo>,
    remote_branches: Vec<RefInfo>,
    tags: Vec<RefInfo>,
    remotes: Vec<(String, String)>,
    stashes: Vec<(String, String)>,
    submodules: Vec<String>,
    worktrees: Vec<(String, String)>,
    status: Vec<FileStatus>,
    state: Option<&'static str>,
    view: View,
    selected: Option<usize>,
    detail: Option<CommitDetail>,
    detail_tab: DetailTab,
    detail_file: Option<String>,
    detail_diff: String,
    tree: Vec<String>,
    tree_file: Option<String>,
    tree_content: String,
    filter: String,
    commit_search: String,
    sel_file: Option<(String, bool)>,
    file_diff: String,
    commit_msg: String,
    amend: bool,
    scroll_to: Option<usize>,
    error: Option<String>,
}

impl Repo {
    fn open(path: PathBuf) -> Self {
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string());
        let mut r = Repo {
            path,
            name,
            head: String::new(),
            commits: vec![],
            rows: vec![],
            lanes: 1,
            local: vec![],
            remote_branches: vec![],
            tags: vec![],
            remotes: vec![],
            stashes: vec![],
            submodules: vec![],
            worktrees: vec![],
            status: vec![],
            state: None,
            view: View::Commits,
            selected: None,
            detail: None,
            detail_tab: DetailTab::Commit,
            detail_file: None,
            detail_diff: String::new(),
            tree: vec![],
            tree_file: None,
            tree_content: String::new(),
            filter: String::new(),
            commit_search: String::new(),
            sel_file: None,
            file_diff: String::new(),
            commit_msg: String::new(),
            amend: false,
            scroll_to: None,
            error: None,
        };
        r.refresh();
        if !r.commits.is_empty() {
            r.select_commit(0);
        }
        r
    }

    fn refresh(&mut self) {
        let p = self.path.clone();
        if git::find_toplevel(&p).is_none() {
            self.error = Some("Dieser Ordner ist (nicht mehr) ein Git-Repository.".into());
            return;
        }
        self.error = None;
        self.head = git::head_name(&p);
        let sel_sha = self.selected.and_then(|i| self.commits.get(i)).map(|c| c.sha.clone());
        self.commits = git::load_commits(&p, 3000, true);
        let (rows, lanes) = graph::layout(&self.commits);
        self.rows = rows;
        self.lanes = lanes.min(14);
        let (l, r, t) = git::load_refs(&p);
        self.local = l;
        self.remote_branches = r;
        self.tags = t;
        self.remotes = git::load_remotes(&p);
        self.stashes = git::load_stashes(&p);
        self.submodules = git::load_submodules(&p);
        self.worktrees = git::load_worktrees(&p);
        self.status = git::load_status(&p);
        self.state = git::repo_state(&p);
        self.selected = sel_sha.and_then(|s| self.commits.iter().position(|c| c.sha == s));
        if let Some(i) = self.selected {
            self.select_commit(i);
        } else {
            self.detail = None;
        }
        self.reload_file_diff();
    }

    fn select_commit(&mut self, i: usize) {
        if i >= self.commits.len() {
            return;
        }
        let changed = self.selected != Some(i) || self.detail.is_none();
        self.selected = Some(i);
        if changed {
            let sha = self.commits[i].sha.clone();
            self.detail = Some(git::commit_detail(&self.path, &sha));
            self.detail_file = None;
            self.detail_diff.clear();
            self.tree.clear();
            self.tree_file = None;
            self.tree_content.clear();
            if self.detail_tab == DetailTab::Tree {
                self.tree = git::commit_tree(&self.path, &sha);
            }
        }
    }

    fn reload_file_diff(&mut self) {
        self.file_diff.clear();
        if self.sel_file.is_none() && self.view == View::Changes {
            if let Some(f) = self.status.iter().find(|f| f.unstaged()).or_else(|| self.status.first()) {
                self.sel_file = Some((f.path.clone(), !f.unstaged()));
            }
        }
        if let Some((path, staged)) = self.sel_file.clone() {
            if let Some(f) = self.status.iter().find(|f| f.path == path) {
                if (staged && f.staged()) || (!staged && f.unstaged()) {
                    self.file_diff = git::working_diff(&self.path, f, staged);
                    return;
                }
            }
            self.sel_file = None;
        }
    }

    fn sha_of(&self, i: usize) -> String {
        self.commits.get(i).map(|c| c.sha.clone()).unwrap_or_default()
    }

    fn ahead_behind(track: &str) -> String {
        let mut s = String::new();
        let t = track.trim_matches(['[', ']']);
        for part in t.split(", ") {
            if let Some(n) = part.strip_prefix("ahead ") {
                s.push_str(&format!(" ↑{n}"));
            } else if let Some(n) = part.strip_prefix("behind ") {
                s.push_str(&format!(" ↓{n}"));
            } else if part == "gone" {
                s.push_str(" (Server-Branch gelöscht)");
            }
        }
        s
    }
}

/// A filled-in command, ready to show and run.
pub struct Form {
    pub cmd: usize,
    pub values: Vec<Value>,
    pub extra: String,
    pub free: String,
    pub last_job: Option<u64>,
    pub open_after: Option<String>,
}

struct RebaseItem {
    short: String,
    subject: String,
    action: usize,
    new_msg: String,
}

struct RebaseDialog {
    base: String,
    items: Vec<RebaseItem>,
    has_merges: bool,
}

const RB_ACTIONS: &[(&str, &str, &str)] = &[
    ("pick", "Behalten", "Commit bleibt unverändert"),
    ("reword", "Umbenennen", "neue Beschreibung, Inhalt bleibt"),
    ("edit", "Anhalten & bearbeiten", "Rebase hält hier an, du kannst den Commit ändern"),
    ("squash", "Mit vorherigem zusammenfassen", "Inhalt und Beschreibung werden mit dem Commit darüber vereint"),
    ("fixup", "In vorherigen einschmelzen", "wie squash, aber diese Beschreibung wird verworfen"),
    ("drop", "Löschen", "Commit wird entfernt"),
];

enum Act {
    Open(&'static str, Vec<(&'static str, String)>),
    Quick { title: String, args: Vec<String>, explain: String },
    SelectCommit(usize),
    View(View),
    Refresh,
    OpenRepoDialog,
    OpenRepo(PathBuf),
    CloseTab(usize),
    SwitchTab(usize),
    Rebase(String),
    SelectFile(String, bool),
    DetailTab(DetailTab),
    DetailFile(String),
    TreeFile(String),
    Terminal,
    Explorer(PathBuf),
    QuickLaunch,
    JumpToRef(String),
    Copy(String),
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Saved {
    tabs: Vec<String>,
    active: usize,
    recent: Vec<String>,
}

pub struct App {
    cmds: Vec<Cmd>,
    tabs: Vec<Repo>,
    active: usize,
    jobs: Vec<Job>,
    next_job: u64,
    dialog: Option<Form>,
    confirm: Option<Form>,
    lib_search: String,
    lib_form: Option<Form>,
    rebase: Option<RebaseDialog>,
    quick: Option<String>,
    show_activity: bool,
    recent: Vec<String>,
    console: String,
    console_jobs: Vec<u64>,
    screenshot: Option<PathBuf>,
    frames: u64,
    shot_requested: bool,
    shot_done: Arc<AtomicBool>,
    was_focused: bool,
    toast: Option<(String, f64)>,
    pending_open: Vec<(u64, PathBuf)>,
}

fn state_file() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("YAGUI").join("state.json"))
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, repo: Option<PathBuf>, screenshot: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let saved: Saved = state_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let cmds = catalog::all();
        let mut app = App {
            cmds,
            tabs: vec![],
            active: 0,
            jobs: vec![],
            next_job: 1,
            dialog: None,
            confirm: None,
            lib_search: String::new(),
            lib_form: None,
            rebase: None,
            quick: None,
            show_activity: true,
            recent: saved.recent.clone(),
            console: String::new(),
            console_jobs: vec![],
            screenshot: screenshot.clone(),
            frames: 0,
            shot_requested: false,
            shot_done: Arc::new(AtomicBool::new(false)),
            was_focused: true,
            toast: None,
            pending_open: vec![],
        };
        let cwd_repo = std::env::current_dir().ok().and_then(|d| git::find_toplevel(&d));
        if let Some(r) = repo.and_then(|r| git::find_toplevel(&r)) {
            app.add_tab(r);
        } else if screenshot.is_some() && cwd_repo.is_some() {
            app.add_tab(cwd_repo.unwrap());
        } else {
            for t in &saved.tabs {
                let p = PathBuf::from(t);
                if p.exists() && git::find_toplevel(&p).is_some() {
                    app.tabs.push(Repo::open(p));
                }
            }
            app.active = saved.active.min(app.tabs.len().saturating_sub(1));
            if app.tabs.is_empty() {
                if let Some(r) = cwd_repo {
                    app.add_tab(r);
                }
            }
        }
        let lib = catalog::find(&app.cmds, "merge");
        app.lib_form = Some(app.make_form(lib, &[]));
        // Optional start view / dialog (used for checks and pictures).
        if let Ok(v) = std::env::var("YAGUI_VIEW") {
            let view = match v.as_str() {
                "changes" => Some(View::Changes),
                "library" => Some(View::Library),
                "console" => Some(View::Console),
                "commits" => Some(View::Commits),
                _ => None,
            };
            if let (Some(view), Some(t)) = (view, app.tabs.get_mut(app.active)) {
                t.view = view;
                t.reload_file_diff();
            }
        }
        if let Ok(id) = std::env::var("YAGUI_OPEN") {
            if app.cmds.iter().any(|c| c.id == id) {
                let id: &'static str = app.cmds.iter().find(|c| c.id == id).unwrap().id;
                app.open_cmd(id, vec![]);
            }
        }
        app
    }

    fn save(&self) {
        if self.screenshot.is_some() {
            return;
        }
        let s = Saved {
            tabs: self.tabs.iter().map(|t| t.path.display().to_string()).collect(),
            active: self.active,
            recent: self.recent.clone(),
        };
        if let Some(p) = state_file() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, serde_json::to_string_pretty(&s).unwrap_or_default());
        }
    }

    fn add_tab(&mut self, path: PathBuf) {
        let ps = path.display().to_string();
        self.recent.retain(|r| r != &ps);
        self.recent.insert(0, ps);
        self.recent.truncate(12);
        if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
            self.active = i;
        } else {
            self.tabs.push(Repo::open(path));
            self.active = self.tabs.len() - 1;
        }
        self.save();
    }

    fn repo(&self) -> Option<&Repo> {
        self.tabs.get(self.active)
    }

    fn make_form(&self, cmd: usize, fills: &[(&'static str, String)]) -> Form {
        let c = &self.cmds[cmd];
        let mut values = catalog::default_values(c);
        if let Some(r) = self.repo() {
            if catalog::has_text_key(c, "remote") && !matches!(c.id, "fetch" | "pull" | "push" | "ls_remote" | "lfs_pull" | "lfs_fetch") {
                if let Some((n, _)) = r.remotes.first() {
                    catalog::set_text(c, &mut values, "remote", n);
                }
            }
        }
        for (k, v) in fills {
            if k.starts_with("flag:") {
                catalog::set_flag(c, &mut values, &k[5..], v == "1");
            } else if k.starts_with("choice:") {
                catalog::set_choice(c, &mut values, v);
            } else {
                catalog::set_text(c, &mut values, k, v);
            }
        }
        Form { cmd, values, extra: String::new(), free: String::new(), last_job: None, open_after: None }
    }

    fn open_cmd(&mut self, id: &str, fills: Vec<(&'static str, String)>) {
        let idx = catalog::find(&self.cmds, id);
        if self.cmds[idx].kind == Kind::InteractiveRebase {
            if let Some((_, v)) = fills.iter().find(|(k, _)| *k == "commit") {
                self.open_rebase(v.clone());
                return;
            }
        }
        self.dialog = Some(self.make_form(idx, &fills));
    }

    // -----------------------------------------------------------------------------------------
    // Running commands

    fn form_args(&self, f: &Form) -> Vec<Arg> {
        let c = &self.cmds[f.cmd];
        if c.kind == Kind::Free {
            let mut s = f.free.trim();
            if let Some(rest) = s.strip_prefix("git ") {
                s = rest;
            }
            return catalog::split_args(s)
                .into_iter()
                .enumerate()
                .map(|(i, a)| Arg {
                    help: if i == 0 { format!("Git-Befehl „{a}“ (Hilfe: git help {a})") } else { "von dir eingegeben".into() },
                    text: a,
                })
                .collect();
        }
        catalog::build_args(c, &f.values, &f.extra)
    }

    fn display_line(&self, f: &Form) -> String {
        let args = self.form_args(f);
        let line = catalog::command_line(&args);
        match line.strip_prefix("git !") {
            Some(rest) => rest.to_string(),
            None => line,
        }
    }

    /// Runs a filled-in form. Risky commands ask once more first.
    fn run_form(&mut self, mut f: Form, ctx: &egui::Context, confirmed: bool) -> Option<u64> {
        let c = self.cmds[f.cmd].clone();
        if c.kind == Kind::InteractiveRebase {
            let base = match f.values.first() {
                Some(Value::Text(t)) => t.trim().to_string(),
                _ => String::new(),
            };
            self.open_rebase(base);
            return None;
        }
        if c.danger == Danger::Risky && !confirmed {
            self.confirm = Some(f);
            return None;
        }
        let mut args: Vec<String> = self.form_args(&f).into_iter().map(|a| a.text).collect();
        if args.is_empty() {
            return None;
        }
        let mut program = "git".to_string();
        if let Some(p) = args[0].strip_prefix('!') {
            program = p.to_string();
            args.remove(0);
        }
        let cwd = if c.needs_repo || self.repo().is_some() { self.repo().map(|r| r.path.clone()) } else { None };
        if c.needs_repo && cwd.is_none() {
            self.toast = Some(("Dafür muss zuerst ein Repository geöffnet sein.".into(), ctx.input(|i| i.time)));
            return None;
        }
        if c.id == "clone" || c.id == "init" || c.id == "svn_clone" {
            if let Some(Value::Text(p)) = c.params.iter().zip(f.values.iter()).find(|(p, _)| matches!(p, P::Text { key: "path", .. })).map(|(_, v)| v) {
                if !p.trim().is_empty() {
                    f.open_after = Some(p.trim().to_string());
                }
            }
        }
        let display = self.display_line(&f);
        let id = self.spawn(JobSpec {
            title: c.title.to_string(),
            program,
            args,
            display,
            cwd,
            stdin: catalog::stdin_of(&c, &f.values),
            env: vec![],
            changes_repo: c.danger != Danger::Read,
            detached: c.kind == Kind::External,
            automatic: false,
        }, ctx);
        f.last_job = Some(id);
        if let Some(p) = f.open_after.take() {
            self.pending_open.push((id, PathBuf::from(p)));
        }
        Some(id)
    }

    fn spawn(&mut self, spec: JobSpec, ctx: &egui::Context) -> u64 {
        let id = self.next_job;
        self.next_job += 1;
        self.jobs.push(git::start_job(id, spec, ctx.clone()));
        if self.jobs.len() > 300 {
            self.jobs.remove(0);
        }
        self.show_activity = true;
        id
    }

    fn quick(&mut self, title: String, args: Vec<String>, ctx: &egui::Context) {
        let Some(r) = self.repo() else { return };
        let display = format!("git {}", args.iter().map(|a| catalog::quote_for_display(a)).collect::<Vec<_>>().join(" "));
        let cwd = Some(r.path.clone());
        self.spawn(JobSpec { title, program: "git".into(), args, display, cwd, stdin: None, env: vec![], changes_repo: true, detached: false, automatic: false }, ctx);
    }

    fn quick_stdin(&mut self, title: String, args: Vec<String>, stdin: String, ctx: &egui::Context) {
        let Some(r) = self.repo() else { return };
        let display = format!("git {}   (Patch über Standardeingabe)", args.join(" "));
        let cwd = Some(r.path.clone());
        self.spawn(JobSpec { title, program: "git".into(), args, display, cwd, stdin: Some(stdin), env: vec![], changes_repo: true, detached: false, automatic: false }, ctx);
    }

    fn job(&self, id: u64) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    fn poll_jobs(&mut self) {
        let mut refresh: Vec<PathBuf> = vec![];
        let mut open: Vec<PathBuf> = vec![];
        for j in self.jobs.iter_mut() {
            if j.handled {
                continue;
            }
            match j.state() {
                JobState::Running => {}
                JobState::Done(code) => {
                    j.handled = true;
                    if code != 0 {
                        j.expanded = true;
                    }
                    if j.changes_repo {
                        if let Some(c) = &j.cwd {
                            refresh.push(c.clone());
                        }
                    }
                    if code == 0 {
                        if let Some(pos) = self.pending_open.iter().position(|(id, _)| *id == j.id) {
                            let (_, p) = self.pending_open.remove(pos);
                            let p = if p.is_absolute() { p } else { j.cwd.clone().or_else(|| std::env::current_dir().ok()).unwrap_or_default().join(p) };
                            open.push(p);
                        }
                    }
                }
                JobState::Started => j.handled = true,
            }
        }
        for p in refresh {
            for t in self.tabs.iter_mut() {
                if t.path == p || p.starts_with(&t.path) {
                    t.refresh();
                }
            }
        }
        for p in open {
            if let Some(top) = git::find_toplevel(&p) {
                self.add_tab(top);
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Interactive rebase

    fn open_rebase(&mut self, base: String) {
        let Some(r) = self.repo() else { return };
        let range = format!("{base}..HEAD");
        let o = git::run(&r.path, &["log", "--reverse", "--format=%h%x1f%p%x1f%s", &range]);
        let mut items = vec![];
        let mut has_merges = false;
        for l in o.stdout.lines() {
            let f: Vec<&str> = l.split('\x1f').collect();
            if f.len() < 3 {
                continue;
            }
            if f[1].split_whitespace().count() > 1 {
                has_merges = true;
                continue;
            }
            items.push(RebaseItem { short: f[0].into(), subject: f[2].into(), action: 0, new_msg: f[2].into() });
        }
        self.rebase = Some(RebaseDialog { base, items, has_merges });
    }

    fn rebase_todo(d: &RebaseDialog, msg_files: &[Option<String>]) -> String {
        let mut s = String::new();
        for (i, it) in d.items.iter().enumerate() {
            let act = RB_ACTIONS[it.action].0;
            if act == "reword" {
                s.push_str(&format!("pick {} {}\n", it.short, it.subject));
                if let Some(Some(f)) = msg_files.get(i) {
                    s.push_str(&format!("exec git commit --amend -F '{f}'\n"));
                } else {
                    s.push_str("exec git commit --amend -F <neue-beschreibung>\n");
                }
            } else {
                s.push_str(&format!("{act} {} {}\n", it.short, it.subject));
            }
        }
        s
    }

    fn run_rebase(&mut self, ctx: &egui::Context) {
        let Some(d) = self.rebase.take() else { return };
        let Some(r) = self.repo() else { return };
        let dir = std::env::temp_dir().join(format!("yagui-rebase-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fwd = |p: &Path| p.display().to_string().replace('\\', "/");
        let mut files = vec![];
        for (i, it) in d.items.iter().enumerate() {
            if RB_ACTIONS[it.action].0 == "reword" {
                let p = dir.join(format!("msg-{i}.txt"));
                let _ = std::fs::write(&p, &it.new_msg);
                files.push(Some(fwd(&p)));
            } else {
                files.push(None);
            }
        }
        let todo = Self::rebase_todo(&d, &files);
        let todo_path = dir.join("todo.txt");
        let _ = std::fs::write(&todo_path, &todo);
        let path = r.path.clone();
        self.spawn(
            JobSpec {
                title: "Interaktiver Rebase".into(),
                program: "git".into(),
                args: vec!["rebase".into(), "-i".into(), d.base.clone()],
                display: format!("git rebase -i {}   (Liste aus dem Editor dieser App:)\n{}", d.base, todo.trim_end()),
                cwd: Some(path),
                stdin: None,
                env: vec![("GIT_SEQUENCE_EDITOR".into(), format!("cp '{}'", fwd(&todo_path)))],
                changes_repo: true,
                detached: false,
                automatic: false,
            },
            ctx,
        );
    }
}


// ---------------------------------------------------------------------------------------------
// Small UI helpers

fn danger_badge(ui: &mut egui::Ui, d: Danger) {
    let (txt, col) = match d {
        Danger::Read => ("Nur lesen – ändert nichts", theme::GREEN),
        Danger::Change => ("Ändert das Repository", theme::YELLOW),
        Danger::Risky => ("Vorsicht – kann Arbeit verwerfen oder Geschichte umschreiben", theme::RED),
    };
    egui::Frame::new()
        .fill(col.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0, col.gamma_multiply(0.7)))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(txt).color(col).size(12.0));
        });
}

fn toolbar_button(ui: &mut egui::Ui, icon: &str, label: &str, tip_title: &str, tip_cmd: &str, enabled: bool) -> egui::Response {
    let size = Vec2::new(58.0, 44.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter_at(rect);
    if resp.hovered() && enabled {
        painter.rect_filled(rect.shrink(1.0), 5, Color32::from_rgb(0x34, 0x34, 0x36));
    }
    let col = if enabled { theme::TEXT } else { theme::TEXT_DIM.gamma_multiply(0.6) };
    painter.text(rect.center_top() + Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, icon, FontId::proportional(17.0), col);
    painter.text(rect.center_bottom() - Vec2::new(0.0, 9.0), Align2::CENTER_CENTER, label, FontId::proportional(11.5), col);
    let tip_title = tip_title.to_string();
    let tip_cmd = tip_cmd.to_string();
    resp.on_hover_ui(move |ui| {
        ui.set_max_width(320.0);
        ui.label(RichText::new(tip_title).strong());
        if !tip_cmd.is_empty() {
            ui.label(RichText::new(tip_cmd).monospace().color(theme::ACCENT));
        }
    })
}

fn status_color(c: char) -> Color32 {
    match c {
        'A' | '?' => theme::GREEN,
        'D' => theme::RED,
        'R' | 'C' => theme::ACCENT,
        'U' => theme::ORANGE,
        _ => theme::YELLOW,
    }
}

fn status_badge(ui: &mut egui::Ui, c: char) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
    let col = status_color(c);
    ui.painter().rect_filled(rect.shrink(1.0), 3, col);
    let letter = if c == '?' { 'N' } else { c };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, letter, FontId::monospace(10.5), Color32::BLACK);
}

fn status_word(c: char) -> &'static str {
    match c {
        'M' => "geändert",
        'A' => "neu hinzugefügt",
        'D' => "gelöscht",
        'R' => "umbenannt",
        'C' => "kopiert",
        '?' => "neu, noch nicht verfolgt",
        'U' => "Konflikt",
        'T' => "Typ geändert",
        _ => "geändert",
    }
}

fn diff_view(ui: &mut egui::Ui, text: &str, id: &str) {
    egui::ScrollArea::both().id_salt(id).auto_shrink([false; 2]).show(ui, |ui| {
        diff_lines(ui, text);
    });
}

fn diff_lines(ui: &mut egui::Ui, text: &str) {
    if text.trim().is_empty() {
        ui.label(RichText::new("Keine Unterschiede (oder Binärdatei).").color(theme::TEXT_DIM));
        return;
    }
    let font = FontId::monospace(12.5);
    let w = ui.available_width().max(600.0);
    for line in text.lines().take(6000) {
        let (bg, fg) = if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") || line.starts_with("index ") {
            (Color32::TRANSPARENT, theme::TEXT_DIM)
        } else if line.starts_with('+') {
            (theme::DIFF_ADD, Color32::from_rgb(0xb5, 0xe8, 0xbd))
        } else if line.starts_with('-') {
            (theme::DIFF_DEL, Color32::from_rgb(0xf2, 0xb8, 0xb8))
        } else if line.starts_with("@@") {
            (theme::DIFF_HUNK, Color32::from_rgb(0x9c, 0xc4, 0xf0))
        } else {
            (Color32::TRANSPARENT, theme::TEXT)
        };
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 17.0), Sense::hover());
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 0, bg);
        }
        ui.painter().text(rect.left_center() + Vec2::new(4.0, 0.0), Align2::LEFT_CENTER, line.replace('\t', "    "), font.clone(), fg);
    }
}

fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).size(11.0).color(theme::TEXT_DIM).strong());
}

struct Suggest {
    branches: Vec<String>,
    remotes: Vec<String>,
    tags: Vec<String>,
    stashes: Vec<String>,
    commits: Vec<(String, String)>,
    files: Vec<String>,
}

impl Suggest {
    fn from(r: Option<&Repo>) -> Self {
        let Some(r) = r else {
            return Suggest { branches: vec![], remotes: vec![], tags: vec![], stashes: vec![], commits: vec![], files: vec![] };
        };
        let mut branches: Vec<String> = r.local.iter().map(|b| b.name.clone()).collect();
        branches.extend(r.remote_branches.iter().map(|b| b.name.clone()));
        Suggest {
            branches,
            remotes: r.remotes.iter().map(|x| x.0.clone()).collect(),
            tags: r.tags.iter().map(|t| t.name.clone()).collect(),
            stashes: r.stashes.iter().map(|s| s.0.clone()).collect(),
            commits: r.commits.iter().take(40).map(|c| (c.short.clone(), c.subject.clone())).collect(),
            files: r.status.iter().map(|f| f.path.clone()).collect(),
        }
    }

    fn for_key(&self, key: &str) -> Vec<(String, String)> {
        let pairs = |v: &Vec<String>| v.iter().map(|x| (x.clone(), x.clone())).collect::<Vec<_>>();
        match key {
            "branch" | "upstream" | "onto" | "a" | "b" | "start" | "head" | "bad" | "good" | "range" | "branches" => {
                let mut v = pairs(&self.branches);
                v.extend(pairs(&self.tags));
                v
            }
            "remote" => pairs(&self.remotes),
            "tag" => pairs(&self.tags),
            "stash" => pairs(&self.stashes),
            "commit" | "commits" | "object" | "ref" => {
                let mut v: Vec<(String, String)> = self.commits.iter().map(|(s, m)| (s.clone(), format!("{s}  {m}"))).collect();
                v.extend(pairs(&self.branches));
                v.extend(pairs(&self.tags));
                v
            }
            "paths" | "file" | "files" => pairs(&self.files),
            _ => vec![],
        }
    }
}

#[derive(PartialEq)]
enum FormAction {
    None,
    Run,
    Cancel,
}

/// Draws a command form: what it does, its options, the exact command line and its result.
fn form_ui(ui: &mut egui::Ui, cmds: &[Cmd], f: &mut Form, sug: &Suggest, jobs: &[Job], in_dialog: bool, line: &str, args: &[Arg]) -> FormAction {
    let c = &cmds[f.cmd];
    let mut action = FormAction::None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(c.title).heading().strong());
    });
    ui.horizontal(|ui| {
        danger_badge(ui, c.danger);
        if c.kind == Kind::External {
            ui.label(RichText::new("öffnet ein eigenes Fenster").color(theme::TEXT_DIM).size(12.0));
        }
    });
    ui.add_space(2.0);
    ui.label(RichText::new(c.summary).size(14.0));
    ui.add_space(6.0);

    if c.kind == Kind::Free {
        ui.horizontal(|ui| {
            ui.label(RichText::new("git").monospace().strong());
            ui.add(egui::TextEdit::singleline(&mut f.free).hint_text("z.B. log --oneline -5").desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
        });
    }

    let mut any_param = false;
    egui::Grid::new(("form", f.cmd)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        for (pi, (p, v)) in c.params.iter().zip(f.values.iter_mut()).enumerate() {
            match (p, v) {
                (P::Text { key, label, hint, help, required, prefix, .. }, Value::Text(t)) => {
                    any_param = true;
                    let lab = if *required { format!("{label} *") } else { label.to_string() };
                    ui.label(RichText::new(lab).strong()).on_hover_text(*help);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let w = (ui.available_width() - 34.0).max(120.0).min(460.0);
                            ui.add(egui::TextEdit::singleline(t).hint_text(*hint).desired_width(w));
                            let opts = sug.for_key(key);
                            if !opts.is_empty() {
                                ui.menu_button("▾", |ui| {
                                    ui.set_max_height(320.0);
                                    egui::ScrollArea::vertical().show(ui, |ui| {
                                        for (val, lab) in opts.iter().take(200) {
                                            if ui.button(lab).clicked() {
                                                let multi = matches!(p, P::Text { multi: true, .. });
                                                if multi && !t.trim().is_empty() {
                                                    t.push(' ');
                                                    t.push_str(val);
                                                } else {
                                                    *t = val.clone();
                                                }
                                                ui.close();
                                            }
                                        }
                                    });
                                })
                                .response
                                .on_hover_text("Vorschläge aus dem Repository");
                            }
                        });
                        let mut h = help.to_string();
                        if !prefix.is_empty() && prefix != &"--" {
                            h = format!("{h}  →  wird als {prefix}… übergeben").trim_start_matches("  →  ").to_string();
                        }
                        if !h.is_empty() {
                            ui.label(RichText::new(h).size(11.5).color(theme::TEXT_DIM));
                        }
                    });
                    ui.end_row();
                }
                (P::Flag { args, label, help, .. }, Value::Bool(b)) => {
                    any_param = true;
                    ui.label("");
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(b, RichText::new(*label)).on_hover_text(format!("fügt {args} hinzu"));
                        ui.label(RichText::new(*args).monospace().size(11.5).color(theme::ACCENT));
                        if !help.is_empty() {
                            ui.label(RichText::new(format!("– {help}")).size(11.5).color(theme::TEXT_DIM));
                        }
                    });
                    ui.end_row();
                }
                (P::Choice { label, options, .. }, Value::Choice(sel)) => {
                    any_param = true;
                    ui.label(RichText::new(*label).strong());
                    ui.vertical(|ui| {
                        egui::ComboBox::from_id_salt(("choice", f.cmd, pi))
                            .width(300.0)
                            .selected_text(options.get(*sel).map(|o| o.1).unwrap_or(""))
                            .show_ui(ui, |ui| {
                                for (i, (a, l, h)) in options.iter().enumerate() {
                                    let txt = if a.is_empty() { l.to_string() } else { format!("{l}   ({a})") };
                                    ui.selectable_value(sel, i, txt).on_hover_text(*h);
                                }
                            });
                        if let Some(o) = options.get(*sel) {
                            ui.label(RichText::new(o.2).size(11.5).color(theme::TEXT_DIM));
                        }
                    });
                    ui.end_row();
                }
                (P::Lit(a, help), _) => {
                    ui.label(RichText::new("Immer dabei").color(theme::TEXT_DIM));
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(*a).monospace().color(theme::ACCENT));
                        ui.label(RichText::new(format!("– {help}")).size(11.5).color(theme::TEXT_DIM));
                    });
                    ui.end_row();
                }
                (P::Stdin { label, hint }, Value::Text(t)) => {
                    any_param = true;
                    ui.label(RichText::new(*label).strong()).on_hover_text("Dieser Text wird dem Befehl als Eingabe übergeben");
                    ui.add(egui::TextEdit::multiline(t).hint_text(*hint).desired_rows(4).desired_width(460.0).font(egui::TextStyle::Monospace));
                    ui.end_row();
                }
                _ => {}
            }
        }
        if c.kind != Kind::Free && c.kind != Kind::InteractiveRebase {
            ui.label(RichText::new("Weitere Optionen").color(theme::TEXT_DIM))
                .on_hover_text("Hier kannst du jede andere Option dieses Befehls selbst eintragen.");
            ui.vertical(|ui| {
                ui.add(egui::TextEdit::singleline(&mut f.extra).hint_text("optional, z.B. --verbose").desired_width(300.0).font(egui::TextStyle::Monospace));
                let first = catalog::split_args(c.base).into_iter().next().unwrap_or_default();
                ui.label(RichText::new(format!("Alle Optionen erklärt: git help {}", first.trim_start_matches('!'))).size(11.5).color(theme::TEXT_DIM));
            });
            ui.end_row();
        }
    });
    if !any_param && c.kind != Kind::Free {
        ui.label(RichText::new("Dieser Befehl braucht keine Angaben.").color(theme::TEXT_DIM));
    }

    ui.add_space(8.0);
    // ---- exact command line with explanation of each part
    egui::Frame::new().fill(Color32::from_rgb(0x1a, 0x1a, 0x1a)).corner_radius(6).inner_margin(10).stroke(Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3a, 0x3a))).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("DAS WIRD AUSGEFÜHRT").size(11.0).color(theme::TEXT_DIM).strong());
        if c.kind == Kind::InteractiveRebase {
            ui.label(RichText::new(line).monospace().size(14.0).color(Color32::from_rgb(0x9f, 0xd1, 0xff)));
            ui.label(RichText::new("Danach öffnet sich der Rebase-Editor dieser App, in dem du jeden Commit einzeln einstellst.").size(12.0).color(theme::TEXT_DIM));
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let prog = if args.first().map(|a| a.text.starts_with('!')).unwrap_or(false) { "" } else { "git" };
                if !prog.is_empty() {
                    ui.label(RichText::new("git").monospace().size(14.0).color(theme::ORANGE)).on_hover_text("Das Programm Git");
                }
                for (i, a) in args.iter().enumerate() {
                    let col = if i == 0 { Color32::from_rgb(0x9f, 0xd1, 0xff) } else if a.text.starts_with('-') { theme::YELLOW } else { theme::TEXT };
                    let t = catalog::quote_for_display(a.text.trim_start_matches('!'));
                    ui.label(RichText::new(t).monospace().size(14.0).color(col)).on_hover_text(&a.help);
                }
            });
            ui.add_space(4.0);
            egui::Grid::new(("explain", f.cmd)).num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                for a in args.iter() {
                    ui.label(RichText::new(catalog::quote_for_display(a.text.trim_start_matches('!'))).monospace().size(12.0).color(theme::TEXT_DIM));
                    ui.label(RichText::new(&a.help).size(12.0).color(theme::TEXT_DIM));
                    ui.end_row();
                }
            });
        }
    });

    let missing = catalog::missing(c, &f.values);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let can = missing.is_empty() && (c.kind != Kind::Free || !f.free.trim().is_empty());
        let label = if c.danger == Danger::Risky { "▶ Ausführen …" } else { "▶ Ausführen" };
        let btn = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE)).fill(if can { Color32::from_rgb(0x2c, 0x6f, 0xbb) } else { Color32::from_rgb(0x3a, 0x3a, 0x3d) });
        if ui.add_enabled(can, btn).on_hover_text(line).clicked() {
            action = FormAction::Run;
        }
        if ui.button("Befehl kopieren").on_hover_text("Kopiert die Befehlszeile, z.B. für ein Terminal").clicked() {
            ui.ctx().copy_text(line.to_string());
        }
        if in_dialog && ui.button("Schließen").clicked() {
            action = FormAction::Cancel;
        }
        if !missing.is_empty() {
            ui.label(RichText::new(format!("Bitte ausfüllen: {}", missing.join(", "))).color(theme::RED));
        }
    });

    if let Some(j) = f.last_job.and_then(|id| jobs.iter().find(|j| j.id == id)) {
        ui.add_space(6.0);
        job_result(ui, j, 220.0);
    }
    action
}

fn job_status_text(j: &Job) -> (String, Color32) {
    match j.state() {
        JobState::Running => ("läuft …".into(), theme::ACCENT),
        JobState::Started => ("gestartet".into(), theme::GREEN),
        JobState::Done(0) => ("erfolgreich".into(), theme::GREEN),
        JobState::Done(c) => (format!("fehlgeschlagen (Code {c})"), theme::RED),
    }
}

fn job_result(ui: &mut egui::Ui, j: &Job, max_h: f32) {
    let (txt, col) = job_status_text(j);
    ui.horizontal(|ui| {
        if j.state() == JobState::Running {
            ui.spinner();
        }
        ui.label(RichText::new(format!("Ergebnis: {txt}")).color(col).strong());
    });
    let out = j.output();
    let out = if out.trim().is_empty() && j.state() != JobState::Running { "(keine Ausgabe)".to_string() } else { out };
    egui::Frame::new().fill(Color32::from_rgb(0x18, 0x18, 0x18)).corner_radius(4).inner_margin(6).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt(("jobout", j.id)).max_height(max_h).stick_to_bottom(true).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(out).monospace().size(12.0)).wrap());
        });
    });
}

// ---------------------------------------------------------------------------------------------
// The frame

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        self.poll_jobs();
        self.handle_screenshot(&ctx);

        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && !self.was_focused {
            if let Some(t) = self.tabs.get_mut(self.active) {
                t.refresh();
            }
        }
        self.was_focused = focused;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::P)) {
            self.quick = Some(String::new());
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5)) {
            if let Some(t) = self.tabs.get_mut(self.active) {
                t.refresh();
            }
        }
        if self.jobs.iter().any(|j| j.state() == JobState::Running) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        let mut acts: Vec<Act> = vec![];
        self.menu_bar(ui, &mut acts);
        self.toolbar(ui, &mut acts);
        self.tab_bar(ui, &mut acts);
        self.status_bar(ui);
        if self.show_activity {
            self.activity_panel(ui);
        }
        if self.tabs.is_empty() {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL)).show(ui, |ui| {
                self.welcome(ui, &mut acts);
            });
        } else {
            self.sidebar(ui, &mut acts);
            let view = self.tabs[self.active].view;
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL)).show(ui, |ui| {
                self.state_banner(ui, &mut acts);
                match view {
                    View::Commits => self.commits_view(ui, &mut acts),
                    View::Changes => self.changes_view(ui, &mut acts, &ctx),
                    View::Library => self.library_view(ui, &ctx),
                    View::Console => self.console_view(ui, &ctx),
                }
            });
        }
        self.dialogs(&ctx);
        self.apply(acts, &ctx);
    }
}

impl App {
    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        ctx.request_repaint();
        if self.shot_done.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frames >= 120 && !self.shot_requested {
            self.shot_requested = true;
            let done = self.shot_done.clone();
            let ctx2 = ctx.clone();
            ctx.request_screenshot(move |img| {
                let w = img.size[0] as u32;
                let h = img.size[1] as u32;
                let mut buf = Vec::with_capacity((w * h * 4) as usize);
                for p in &img.pixels {
                    let [r, g, b, a] = p.to_srgba_unmultiplied();
                    buf.extend_from_slice(&[r, g, b, a]);
                }
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Some(im) = image::RgbaImage::from_raw(w, h, buf) {
                    if let Err(e) = im.save(&path) {
                        eprintln!("Bild konnte nicht gespeichert werden: {e}");
                    }
                }
                done.store(true, Ordering::SeqCst);
                ctx2.request_repaint();
            });
        }
        if self.frames > 2000 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn apply(&mut self, acts: Vec<Act>, ctx: &egui::Context) {
        for a in acts {
            match a {
                Act::Open(id, fills) => self.open_cmd(id, fills),
                Act::Quick { title, args, explain } => {
                    let _ = explain;
                    self.quick(title, args, ctx);
                }
                Act::SelectCommit(i) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.select_commit(i);
                    }
                }
                Act::View(v) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.view = v;
                        if v == View::Changes {
                            t.status = git::load_status(&t.path);
                            t.reload_file_diff();
                        }
                    }
                }
                Act::Refresh => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.refresh();
                    }
                }
                Act::OpenRepoDialog => {
                    if let Some(p) = rfd::FileDialog::new().set_title("Git-Repository öffnen").pick_folder() {
                        match git::find_toplevel(&p) {
                            Some(top) => self.add_tab(top),
                            None => {
                                self.toast = Some(("In diesem Ordner gibt es kein Git-Repository. Lege mit „Neu“ eines an.".into(), ctx.input(|i| i.time)));
                            }
                        }
                    }
                }
                Act::OpenRepo(p) => match git::find_toplevel(&p) {
                    Some(top) => self.add_tab(top),
                    None => self.toast = Some(("Kein Git-Repository gefunden.".into(), ctx.input(|i| i.time))),
                },
                Act::CloseTab(i) => {
                    if i < self.tabs.len() {
                        self.tabs.remove(i);
                        if self.active >= self.tabs.len() {
                            self.active = self.tabs.len().saturating_sub(1);
                        }
                        self.save();
                    }
                }
                Act::SwitchTab(i) => {
                    self.active = i;
                    if let Some(t) = self.tabs.get_mut(i) {
                        t.refresh();
                    }
                    self.save();
                }
                Act::Rebase(base) => self.open_rebase(base),
                Act::SelectFile(p, staged) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.sel_file = Some((p, staged));
                        t.reload_file_diff();
                    }
                }
                Act::DetailTab(dt) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.detail_tab = dt;
                        if dt == DetailTab::Tree && t.tree.is_empty() {
                            if let Some(i) = t.selected {
                                let sha = t.sha_of(i);
                                t.tree = git::commit_tree(&t.path, &sha);
                            }
                        }
                        if dt == DetailTab::Changes && t.detail_file.is_none() {
                            if let Some(f) = t.detail.as_ref().and_then(|d| d.files.first()).map(|f| f.1.clone()) {
                                let sha = t.detail.as_ref().unwrap().sha.clone();
                                t.detail_diff = git::commit_file_diff(&t.path, &sha, &f);
                                t.detail_file = Some(f);
                            }
                        }
                    }
                }
                Act::DetailFile(f) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        if let Some(d) = &t.detail {
                            t.detail_diff = git::commit_file_diff(&t.path, &d.sha, &f);
                            t.detail_file = Some(f);
                            t.detail_tab = DetailTab::Changes;
                        }
                    }
                }
                Act::TreeFile(f) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        if let Some(d) = &t.detail {
                            let spec = format!("{}:{}", d.sha, f);
                            let o = git::run(&t.path, &["show", &spec]);
                            t.tree_content = if o.stdout.contains('\0') { "(Binärdatei)".into() } else { o.stdout };
                            t.tree_file = Some(f);
                        }
                    }
                }
                Act::Terminal => {
                    if let Some(r) = self.repo() {
                        open_terminal(&r.path);
                    }
                }
                Act::Explorer(p) => open_explorer(&p),
                Act::QuickLaunch => self.quick = Some(String::new()),
                Act::JumpToRef(name) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        let o = git::run(&t.path, &["rev-parse", &name]);
                        let sha = o.stdout.trim().to_string();
                        if let Some(i) = t.commits.iter().position(|c| c.sha == sha) {
                            t.view = View::Commits;
                            t.select_commit(i);
                            t.scroll_to = Some(i);
                        }
                    }
                }
                Act::Copy(s) => ctx.copy_text(s),
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Bars

    fn menu_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        egui::Panel::top("menu").frame(egui::Frame::new().fill(theme::TOOLBAR).inner_margin(egui::Margin::symmetric(6, 2))).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Datei", |ui| {
                    if ui.button("Repository öffnen …").clicked() {
                        acts.push(Act::OpenRepoDialog);
                        ui.close();
                    }
                    if ui.button("Repository klonen …   (git clone)").clicked() {
                        acts.push(Act::Open("clone", vec![]));
                        ui.close();
                    }
                    if ui.button("Neues Repository …   (git init)").clicked() {
                        acts.push(Act::Open("init", vec![]));
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Zuletzt geöffnet", |ui| {
                        for r in self.recent.clone() {
                            if ui.button(&r).clicked() {
                                acts.push(Act::OpenRepo(PathBuf::from(&r)));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("Beenden").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Ansicht", |ui| {
                    if ui.button("Lokale Änderungen").clicked() {
                        acts.push(Act::View(View::Changes));
                        ui.close();
                    }
                    if ui.button("Alle Commits").clicked() {
                        acts.push(Act::View(View::Commits));
                        ui.close();
                    }
                    if ui.button("Alle Git-Befehle").clicked() {
                        acts.push(Act::View(View::Library));
                        ui.close();
                    }
                    if ui.button("Konsole").clicked() {
                        acts.push(Act::View(View::Console));
                        ui.close();
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_activity, "Aktivitätsprotokoll anzeigen");
                    if ui.button("Neu laden (F5)").clicked() {
                        acts.push(Act::Refresh);
                        ui.close();
                    }
                });
                // One menu per command group: every git command is reachable from here.
                ui.menu_button("Git-Befehle", |ui| {
                    for (cat, name, _) in CATS {
                        ui.menu_button(*name, |ui| {
                            ui.set_max_height(520.0);
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                for c in self.cmds.iter().filter(|c| c.cat == *cat) {
                                    if ui.button(c.title).on_hover_text(c.summary).clicked() {
                                        acts.push(Act::Open(c.id, vec![]));
                                        ui.close();
                                    }
                                }
                            });
                        });
                    }
                });
                ui.menu_button("Hilfe", |ui| {
                    ui.label("Fahre mit der Maus über einen Knopf,");
                    ui.label("um zu sehen, welcher Git-Befehl dahintersteckt.");
                    ui.separator();
                    if ui.button("Schnellsuche nach Befehlen (Strg+Umschalt+P)").clicked() {
                        acts.push(Act::QuickLaunch);
                        ui.close();
                    }
                    if ui.button("Git-Version anzeigen").clicked() {
                        acts.push(Act::Open("version", vec![]));
                        ui.close();
                    }
                });
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let has = !self.tabs.is_empty();
        let (rname, head) = self.repo().map(|r| (r.name.clone(), r.head.clone())).unwrap_or_default();
        let changes = self.repo().map(|r| r.status.len()).unwrap_or(0);
        egui::Panel::top("toolbar").exact_size(52.0).frame(egui::Frame::new().fill(theme::TOOLBAR).inner_margin(egui::Margin::symmetric(8, 3))).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if toolbar_button(ui, "📂", "Öffnen", "Repository öffnen", "", true).clicked() {
                    acts.push(Act::OpenRepoDialog);
                }
                if toolbar_button(ui, "⬇", "Klonen", "Repository von einem Server herunterladen", "git clone <adresse> <ordner>", true).clicked() {
                    acts.push(Act::Open("clone", vec![]));
                }
                if toolbar_button(ui, "🔍", "Suchen", "Schnellsuche: jeden Git-Befehl finden (Strg+Umschalt+P)", "", true).clicked() {
                    acts.push(Act::QuickLaunch);
                }
                ui.add_space(10.0);
                if toolbar_button(ui, "⟲", "Fetch", "Neues vom Server holen – ändert deine Dateien nicht", "git fetch --prune", has).clicked() {
                    acts.push(Act::Open("fetch", vec![]));
                }
                if toolbar_button(ui, "⬇", "Pull", "Neues holen und in deinen Branch einarbeiten", "git pull", has).clicked() {
                    acts.push(Act::Open("pull", vec![]));
                }
                if toolbar_button(ui, "⬆", "Push", "Deine Commits auf den Server hochladen", "git push", has).clicked() {
                    acts.push(Act::Open("push", vec![]));
                }
                ui.add_space(10.0);
                if toolbar_button(ui, "📥", "Stash", "Änderungen zwischenlagern", "git stash push", has).clicked() {
                    acts.push(Act::Open("stash_push", vec![]));
                }
                if toolbar_button(ui, "📤", "Pop", "Zwischengelagerte Änderungen zurückholen", "git stash pop", has).clicked() {
                    acts.push(Act::Open("stash_pop", vec![]));
                }
                ui.add_space(10.0);

                // Centre: repository and branch, click to switch branch
                let avail = ui.available_width();
                let right_w = 6.0 * 60.0;
                let centre_w = 250.0f32.min((avail - right_w).max(120.0));
                let pad = ((avail - right_w - centre_w) / 2.0 - 60.0).max(0.0);
                ui.add_space(pad);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(centre_w, 40.0), Sense::click());
                ui.painter().rect_filled(rect, 6, if resp.hovered() { Color32::from_rgb(0x36, 0x36, 0x38) } else { Color32::from_rgb(0x2c, 0x2c, 0x2e) });
                if has {
                    ui.painter().text(rect.center_top() + Vec2::new(0.0, 12.0), Align2::CENTER_CENTER, &rname, FontId::proportional(13.5), theme::TEXT);
                    ui.painter().text(rect.center_bottom() - Vec2::new(0.0, 11.0), Align2::CENTER_CENTER, format!("Branch: {head}"), FontId::proportional(12.0), theme::TEXT_DIM);
                } else {
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, "Kein Repository geöffnet", FontId::proportional(13.0), theme::TEXT_DIM);
                }
                let resp = resp.on_hover_text("Aktueller Branch – klicken zum Wechseln (git switch)");
                if has {
                    let locals: Vec<String> = self.repo().map(|r| r.local.iter().map(|b| b.name.clone()).collect()).unwrap_or_default();
                    egui::Popup::menu(&resp).show(|ui| {
                        ui.label(RichText::new("Wechseln zu … (git switch)").color(theme::TEXT_DIM));
                        for b in locals {
                            if ui.button(&b).clicked() {
                                acts.push(Act::Open("switch", vec![("branch", b.clone())]));
                                ui.close();
                            }
                        }
                    });
                }
                ui.add_space(8.0);
                if toolbar_button(ui, "🌿", "Branch", "Neuen Branch anlegen und dorthin wechseln", "git switch -c <name>", has).clicked() {
                    acts.push(Act::Open("switch_create", vec![]));
                }
                if toolbar_button(ui, "🔀", "Merge", "Anderen Branch in den aktuellen holen", "git merge <branch>", has).clicked() {
                    acts.push(Act::Open("merge", vec![]));
                }
                if toolbar_button(ui, "✔", "Commit", &format!("Änderungen speichern ({changes} Dateien geändert)"), "git commit -m \"…\"", has).clicked() {
                    acts.push(Act::View(View::Changes));
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if toolbar_button(ui, "🧰", "Befehle", "Alle Git-Befehle mit Erklärung", "", true).clicked() {
                        acts.push(Act::View(View::Library));
                    }
                    if toolbar_button(ui, "⌨", "Konsole", "Eigene Git-Befehle eintippen", "", has).clicked() {
                        acts.push(Act::View(View::Console));
                    }
                    if toolbar_button(ui, "🖥", "Terminal", "Terminal in diesem Ordner öffnen", "", has).clicked() {
                        acts.push(Act::Terminal);
                    }
                    if toolbar_button(ui, "📁", "Explorer", "Ordner im Datei-Explorer öffnen", "", has).clicked() {
                        if let Some(r) = self.repo() {
                            acts.push(Act::Explorer(r.path.clone()));
                        }
                    }
                });
            });
        });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        egui::Panel::top("tabs").exact_size(26.0).frame(egui::Frame::new().fill(Color32::from_rgb(0x19, 0x19, 0x19))).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                let n = self.tabs.len().max(1);
                let w = ((ui.available_width() - 30.0) / n as f32).clamp(120.0, 300.0);
                for (i, t) in self.tabs.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::click());
                    let active = i == self.active;
                    let bg = if active { theme::PANEL } else if resp.hovered() { Color32::from_rgb(0x26, 0x26, 0x26) } else { Color32::from_rgb(0x1d, 0x1d, 0x1d) };
                    ui.painter().rect_filled(rect, 0, bg);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, &t.name, FontId::proportional(12.5), if active { theme::TEXT } else { theme::TEXT_DIM });
                    let close = egui::Rect::from_center_size(rect.right_center() - Vec2::new(12.0, 0.0), Vec2::splat(14.0));
                    let hover_close = resp.hover_pos().map(|p| close.contains(p)).unwrap_or(false);
                    if resp.hovered() || active {
                        ui.painter().text(close.center(), Align2::CENTER_CENTER, "×", FontId::proportional(14.0), if hover_close { theme::RED } else { theme::TEXT_DIM });
                    }
                    if resp.clicked() {
                        if hover_close {
                            acts.push(Act::CloseTab(i));
                        } else {
                            acts.push(Act::SwitchTab(i));
                        }
                    }
                    resp.on_hover_text(t.path.display().to_string());
                }
                if ui.add(egui::Button::new("+").frame(false)).on_hover_text("Weiteres Repository öffnen").clicked() {
                    acts.push(Act::OpenRepoDialog);
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let now = ui.ctx().input(|i| i.time);
        egui::Panel::bottom("statusbar").exact_size(24.0).frame(egui::Frame::new().fill(theme::TOOLBAR).inner_margin(egui::Margin::symmetric(8, 2))).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let running = self.jobs.iter().filter(|j| j.state() == JobState::Running).count();
                if running > 0 {
                    ui.spinner();
                    ui.label(RichText::new(format!("{running} Befehl(e) laufen")).color(theme::ACCENT));
                }
                if let Some(j) = self.jobs.iter().rev().find(|j| !j.automatic) {
                    let (t, c) = job_status_text(j);
                    ui.label(RichText::new("Zuletzt:").color(theme::TEXT_DIM));
                    ui.label(RichText::new(j.display.lines().next().unwrap_or("")).monospace().size(12.0));
                    ui.label(RichText::new(t).color(c));
                } else {
                    ui.label(RichText::new("Tipp: Jeder Knopf zeigt beim Darüberfahren, welcher Git-Befehl ausgeführt wird.").color(theme::TEXT_DIM));
                }
                if let Some((msg, t)) = &self.toast {
                    if now - t < 6.0 {
                        ui.label(RichText::new(msg).color(theme::YELLOW));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if self.show_activity { "▼ Aktivität ausblenden" } else { "▲ Aktivität anzeigen" };
                    if ui.add(egui::Button::new(label).frame(false)).clicked() {
                        self.show_activity = !self.show_activity;
                    }
                    ui.label(RichText::new(format!("{} Befehle ausgeführt", self.jobs.len())).color(theme::TEXT_DIM));
                });
            });
        });
    }

    fn activity_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("activity").resizable(true).default_size(120.0).min_size(60.0).frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("AKTIVITÄT – jeder ausgeführte Git-Befehl mit Ergebnis").size(11.0).strong().color(theme::TEXT_DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Liste leeren").clicked() {
                        self.jobs.retain(|j| j.state() == JobState::Running);
                    }
                });
            });
            egui::ScrollArea::vertical().id_salt("activity_scroll").auto_shrink([false; 2]).stick_to_bottom(true).show(ui, |ui| {
                if self.jobs.is_empty() {
                    ui.label(RichText::new("Noch keine Befehle ausgeführt. Alles, was du hier tust, erscheint mit genauer Befehlszeile und Ausgabe.").color(theme::TEXT_DIM));
                }
                for j in self.jobs.iter_mut() {
                    let (t, c) = job_status_text(j);
                    let dur = j.finished.lock().unwrap().map(|f| f.duration_since(j.started).as_secs_f32()).unwrap_or_else(|| j.started.elapsed().as_secs_f32());
                    let resp = ui
                        .horizontal(|ui| {
                            let icon = match j.state() {
                                JobState::Running => "⏳",
                                JobState::Done(0) | JobState::Started => "✔",
                                _ => "✖",
                            };
                            ui.label(RichText::new(icon).color(c));
                            ui.label(RichText::new(&j.title).strong());
                            ui.label(RichText::new(j.display.lines().next().unwrap_or("")).monospace().size(12.0).color(Color32::from_rgb(0x9f, 0xd1, 0xff)));
                            ui.label(RichText::new(format!("{t} · {dur:.1} s")).color(c).size(12.0));
                            if j.state() == JobState::Running && ui.small_button("Abbrechen").clicked() {
                                j.cancel.store(true, Ordering::Relaxed);
                            }
                            let lbl = if j.expanded { "Ausgabe ▲" } else { "Ausgabe ▼" };
                            if ui.small_button(lbl).clicked() {
                                j.expanded = !j.expanded;
                            }
                        })
                        .response;
                    let _ = resp;
                    if j.expanded || j.state() == JobState::Running {
                        ui.indent(("ind", j.id), |ui| {
                            if j.display.contains('\n') {
                                ui.label(RichText::new(&j.display).monospace().size(11.5).color(theme::TEXT_DIM));
                            }
                            let out = j.output();
                            ui.add(egui::Label::new(RichText::new(if out.trim().is_empty() { "(keine Ausgabe)".to_string() } else { out }).monospace().size(12.0)).wrap());
                        });
                    }
                }
            });
        });
    }

    // -----------------------------------------------------------------------------------------
    // Sidebar

    fn sidebar(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let r = &mut self.tabs[self.active];
        egui::Panel::left("sidebar").resizable(true).default_size(270.0).min_size(200.0).frame(egui::Frame::new().fill(theme::SIDEBAR).inner_margin(egui::Margin::symmetric(6, 6))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&r.name).strong().size(14.5));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new("⟳").frame(false)).on_hover_text("Neu laden (F5) – liest Status, Branches und Verlauf neu ein").clicked() {
                        acts.push(Act::Refresh);
                    }
                });
            });
            ui.add_space(4.0);
            let n = r.status.len();
            let nav = [
                (View::Changes, format!("📝  Lokale Änderungen{}", if n > 0 { format!("  ({n})") } else { String::new() }), "Geänderte Dateien ansehen, vormerken und committen"),
                (View::Commits, "🕘  Alle Commits".to_string(), "Verlauf aller Branches als Graph"),
                (View::Library, "🧰  Alle Git-Befehle".to_string(), "Jeder Git-Befehl mit Erklärung und Vorschau"),
                (View::Console, "⌨  Konsole".to_string(), "Git-Befehle selbst eintippen"),
            ];
            for (v, label, tip) in nav {
                let sel = r.view == v;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 25.0), Sense::click());
                if sel {
                    ui.painter().rect_filled(rect, 4, Color32::from_rgb(0x37, 0x3a, 0x3f));
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 4, theme::ROW_HOVER);
                }
                ui.painter().text(rect.left_center() + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(13.5), if sel { Color32::WHITE } else { theme::TEXT });
                if resp.on_hover_text(tip).clicked() {
                    acts.push(Act::View(v));
                }
            }
            ui.add_space(6.0);
            ui.add(egui::TextEdit::singleline(&mut r.filter).hint_text("🔍 Filter").desired_width(f32::INFINITY));
            ui.add_space(4.0);
            let filter = r.filter.to_lowercase();
            let matches = |s: &str| filter.is_empty() || s.to_lowercase().contains(&filter);

            egui::ScrollArea::vertical().id_salt("side_scroll").auto_shrink([false; 2]).show(ui, |ui| {
                egui::CollapsingHeader::new(RichText::new(format!("Branches ({})", r.local.len())).strong()).default_open(true).show(ui, |ui| {
                    for b in r.local.iter().filter(|b| matches(&b.name)) {
                        let label = format!("{}{}{}", if b.is_head { "✔ " } else { "    " }, b.name, Repo::ahead_behind(&b.track));
                        let txt = if b.is_head { RichText::new(label).strong() } else { RichText::new(label) };
                        let resp = ui.add(egui::Button::new(txt).frame(false));
                        let resp = resp.on_hover_text(format!(
                            "Branch {}{}\nKlick: im Verlauf zeigen · Doppelklick: wechseln (git switch {}) · Rechtsklick: alle Aktionen",
                            b.name,
                            if b.upstream.is_empty() { String::new() } else { format!(" ↔ {}", b.upstream) },
                            b.name
                        ));
                        if resp.double_clicked() {
                            acts.push(Act::Open("switch", vec![("branch", b.name.clone())]));
                        } else if resp.clicked() {
                            acts.push(Act::JumpToRef(b.name.clone()));
                        }
                        let name = b.name.clone();
                        let up = b.upstream.clone();
                        let head = r.head.clone();
                        resp.context_menu(|ui| branch_menu(ui, acts, &name, &up, &head, false));
                    }
                });
                egui::CollapsingHeader::new(RichText::new(format!("Remotes ({})", r.remotes.len())).strong()).default_open(true).show(ui, |ui| {
                    for (rn, url) in &r.remotes {
                        let resp = egui::CollapsingHeader::new(format!("☁ {rn}")).id_salt(("remote", rn)).default_open(false).show(ui, |ui| {
                            for b in r.remote_branches.iter().filter(|b| b.name.starts_with(&format!("{rn}/")) && matches(&b.name)) {
                                let resp = ui.add(egui::Button::new(b.name.trim_start_matches(&format!("{rn}/"))).frame(false));
                                let resp = resp.on_hover_text(format!("Server-Branch {}\nDoppelklick: lokal auschecken (git checkout --track {})", b.name, b.name));
                                if resp.double_clicked() {
                                    acts.push(Act::Open("checkout_track", vec![("branch", b.name.clone())]));
                                } else if resp.clicked() {
                                    acts.push(Act::JumpToRef(b.name.clone()));
                                }
                                let name = b.name.clone();
                                let head = r.head.clone();
                                resp.context_menu(|ui| branch_menu(ui, acts, &name, "", &head, true));
                            }
                        });
                        let rn2 = rn.clone();
                        resp.header_response.on_hover_text(url).context_menu(|ui| {
                            if ui.button(format!("Fetch von {rn2}   (git fetch {rn2})")).clicked() {
                                acts.push(Act::Open("fetch", vec![("remote", rn2.clone())]));
                                ui.close();
                            }
                            if ui.button("Adresse ändern …   (git remote set-url)").clicked() {
                                acts.push(Act::Open("remote_set_url", vec![("remote", rn2.clone())]));
                                ui.close();
                            }
                            if ui.button("Umbenennen …   (git remote rename)").clicked() {
                                acts.push(Act::Open("remote_rename", vec![("remote", rn2.clone())]));
                                ui.close();
                            }
                            if ui.button("Veraltete Branches aufräumen   (git remote prune)").clicked() {
                                acts.push(Act::Open("remote_prune", vec![("remote", rn2.clone())]));
                                ui.close();
                            }
                            if ui.button("Remote entfernen …   (git remote remove)").clicked() {
                                acts.push(Act::Open("remote_remove", vec![("remote", rn2.clone())]));
                                ui.close();
                            }
                        });
                    }
                    if ui.add(egui::Button::new(RichText::new("+ Remote hinzufügen").color(theme::TEXT_DIM)).frame(false)).on_hover_text("git remote add <name> <adresse>").clicked() {
                        acts.push(Act::Open("remote_add", vec![]));
                    }
                });
                egui::CollapsingHeader::new(RichText::new(format!("Tags ({})", r.tags.len())).strong()).default_open(false).show(ui, |ui| {
                    for t in r.tags.iter().rev().filter(|t| matches(&t.name)) {
                        let resp = ui.add(egui::Button::new(format!("🏷 {}", t.name)).frame(false));
                        if resp.clicked() {
                            acts.push(Act::JumpToRef(t.name.clone()));
                        }
                        let name = t.name.clone();
                        resp.context_menu(|ui| {
                            if ui.button(format!("Auschecken   (git checkout {name})")).clicked() {
                                acts.push(Act::Open("checkout", vec![("branch", name.clone())]));
                                ui.close();
                            }
                            if ui.button("Hochladen …   (git push origin <tag>)").clicked() {
                                acts.push(Act::Open("push_tag", vec![("tag", name.clone())]));
                                ui.close();
                            }
                            if ui.button("Details   (git show)").clicked() {
                                acts.push(Act::Open("show", vec![("commit", name.clone())]));
                                ui.close();
                            }
                            if ui.button("Lokal löschen …   (git tag -d)").clicked() {
                                acts.push(Act::Open("tag_delete", vec![("tag", name.clone())]));
                                ui.close();
                            }
                            if ui.button("Auf dem Server löschen …   (git push --delete)").clicked() {
                                acts.push(Act::Open("push_tag_delete", vec![("tag", name.clone())]));
                                ui.close();
                            }
                        });
                    }
                    if ui.add(egui::Button::new(RichText::new("+ Tag erstellen").color(theme::TEXT_DIM)).frame(false)).on_hover_text("git tag <name>").clicked() {
                        acts.push(Act::Open("tag_create", vec![]));
                    }
                });
                egui::CollapsingHeader::new(RichText::new(format!("Stashes ({})", r.stashes.len())).strong()).default_open(false).show(ui, |ui| {
                    for (name, msg) in r.stashes.iter().filter(|s| matches(&s.1)) {
                        let resp = ui.add(egui::Button::new(format!("📦 {msg}")).frame(false)).on_hover_text(format!("{name}\nRechtsklick: anwenden, zurückholen, löschen"));
                        let name = name.clone();
                        resp.context_menu(|ui| {
                            if ui.button(format!("Anwenden (behalten)   (git stash apply {name})")).clicked() {
                                acts.push(Act::Open("stash_apply", vec![("stash", name.clone())]));
                                ui.close();
                            }
                            if ui.button(format!("Zurückholen & entfernen   (git stash pop {name})")).clicked() {
                                acts.push(Act::Open("stash_pop", vec![("stash", name.clone())]));
                                ui.close();
                            }
                            if ui.button("Inhalt ansehen   (git stash show -p)").clicked() {
                                acts.push(Act::Open("stash_show", vec![("stash", name.clone())]));
                                ui.close();
                            }
                            if ui.button("Branch daraus erstellen …   (git stash branch)").clicked() {
                                acts.push(Act::Open("stash_branch", vec![("stash", name.clone())]));
                                ui.close();
                            }
                            if ui.button("Löschen …   (git stash drop)").clicked() {
                                acts.push(Act::Open("stash_drop", vec![("stash", name.clone())]));
                                ui.close();
                            }
                        });
                    }
                    if ui.add(egui::Button::new(RichText::new("+ Änderungen zwischenlagern").color(theme::TEXT_DIM)).frame(false)).on_hover_text("git stash push").clicked() {
                        acts.push(Act::Open("stash_push", vec![]));
                    }
                });
                egui::CollapsingHeader::new(RichText::new(format!("Submodule ({})", r.submodules.len())).strong()).default_open(false).show(ui, |ui| {
                    for s in &r.submodules {
                        let resp = ui.add(egui::Button::new(format!("🧩 {s}")).frame(false));
                        let s2 = s.clone();
                        let full = r.path.join(s);
                        if resp.double_clicked() {
                            acts.push(Act::OpenRepo(full.clone()));
                        }
                        resp.on_hover_text("Doppelklick: als eigenes Repository öffnen").context_menu(|ui| {
                            if ui.button("Öffnen").clicked() {
                                acts.push(Act::OpenRepo(full.clone()));
                                ui.close();
                            }
                            if ui.button("Aktualisieren   (git submodule update)").clicked() {
                                acts.push(Act::Open("submodule_update", vec![("paths", s2.clone())]));
                                ui.close();
                            }
                            if ui.button("Abmelden …   (git submodule deinit)").clicked() {
                                acts.push(Act::Open("submodule_deinit", vec![("paths", s2.clone())]));
                                ui.close();
                            }
                        });
                    }
                    if ui.add(egui::Button::new(RichText::new("+ Submodul hinzufügen").color(theme::TEXT_DIM)).frame(false)).on_hover_text("git submodule add <adresse>").clicked() {
                        acts.push(Act::Open("submodule_add", vec![]));
                    }
                });
                egui::CollapsingHeader::new(RichText::new(format!("Worktrees ({})", r.worktrees.len().saturating_sub(1))).strong()).default_open(false).show(ui, |ui| {
                    for (p, b) in r.worktrees.iter().skip(1) {
                        let resp = ui.add(egui::Button::new(format!("🗂 {b}")).frame(false)).on_hover_text(p);
                        let pb = PathBuf::from(p);
                        let p2 = p.clone();
                        if resp.double_clicked() {
                            acts.push(Act::OpenRepo(pb.clone()));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Öffnen").clicked() {
                                acts.push(Act::OpenRepo(pb.clone()));
                                ui.close();
                            }
                            if ui.button("Entfernen …   (git worktree remove)").clicked() {
                                acts.push(Act::Open("worktree_remove", vec![("path", p2.clone())]));
                                ui.close();
                            }
                        });
                    }
                    if ui.add(egui::Button::new(RichText::new("+ Arbeitsordner anlegen").color(theme::TEXT_DIM)).frame(false)).on_hover_text("git worktree add <ordner> <branch>").clicked() {
                        acts.push(Act::Open("worktree_add", vec![]));
                    }
                });
            });
        });
    }

    fn state_banner(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let r = &self.tabs[self.active];
        if let Some(e) = &r.error {
            ui.colored_label(theme::RED, e);
        }
        let Some(st) = r.state else { return };
        let (text, cont, abort, extra): (&str, Option<&'static str>, Option<&'static str>, Option<&'static str>) = match st {
            "merge" => ("Ein Merge ist im Gange. Löse die Konflikte, merke die Dateien vor und schließe den Merge ab.", Some("merge_continue"), Some("merge_abort"), None),
            "rebase" => ("Ein Rebase ist angehalten (Konflikt oder „bearbeiten“). Danach fortsetzen.", Some("rebase_continue"), Some("rebase_abort"), Some("rebase_skip")),
            "cherry-pick" => ("Ein Cherry-Pick ist im Gange.", Some("cherry_pick_continue"), Some("cherry_pick_abort"), Some("cherry_pick_skip")),
            "revert" => ("Ein Revert ist im Gange.", Some("revert_continue"), Some("revert_abort"), None),
            "am" => ("Patches werden angewendet (git am).", Some("am_continue"), Some("am_abort"), Some("am_skip")),
            "bisect" => ("Fehlersuche (Bisect) läuft: Teste den aktuellen Stand und sag, ob er gut oder schlecht ist.", Some("bisect_good"), Some("bisect_reset"), Some("bisect_bad")),
            _ => return,
        };
        egui::Frame::new().fill(Color32::from_rgb(0x4a, 0x3a, 0x16)).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("⚠ {text}")).color(theme::YELLOW));
                for id in [cont, extra, abort].into_iter().flatten() {
                    let c = &self.cmds[catalog::find(&self.cmds, id)];
                    if ui.button(c.title).on_hover_text(format!("git {}", c.base)).clicked() {
                        acts.push(Act::Open(c.id, vec![]));
                    }
                }
            });
        });
    }

    // -----------------------------------------------------------------------------------------
    // Commits view

    fn commits_view(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        let r = &mut self.tabs[self.active];
        egui::Panel::bottom("detail").resizable(true).default_size(360.0).min_size(120.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(0)).show(ui, |ui| {
            detail_pane(ui, r, acts);
        });

        // header row
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(&mut r.commit_search).hint_text("🔍 Commits durchsuchen (Text, Autor, SHA)").desired_width(320.0));
            ui.label(RichText::new(format!("{} Commits", r.commits.len())).color(theme::TEXT_DIM));
            ui.label(RichText::new("· Rechtsklick auf einen Commit zeigt alle Aktionen").color(theme::TEXT_DIM).size(12.0));
        });
        let q = r.commit_search.to_lowercase();
        let visible: Vec<usize> = if q.is_empty() {
            (0..r.commits.len()).collect()
        } else {
            r.commits.iter().enumerate().filter(|(_, c)| c.subject.to_lowercase().contains(&q) || c.author.to_lowercase().contains(&q) || c.sha.starts_with(&q)).map(|(i, _)| i).collect()
        };
        let row_h = 23.0;
        let mut scroll = egui::ScrollArea::vertical().id_salt("commits").auto_shrink([false; 2]);
        if let Some(i) = r.scroll_to.take() {
            if let Some(pos) = visible.iter().position(|v| *v == i) {
                scroll = scroll.vertical_scroll_offset((pos as f32 * row_h - 120.0).max(0.0));
            }
        }
        let graph_on = q.is_empty();
        let lane_w = 14.0;
        let graph_w = if graph_on { r.lanes as f32 * lane_w + 12.0 } else { 6.0 };
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
                    p.rect_filled(rect, 3, theme::ROW_SEL);
                } else if resp.hovered() {
                    p.rect_filled(rect, 3, theme::ROW_HOVER);
                }
                // graph
                if graph_on {
                    if let Some(row) = r.rows.get(i) {
                        let x = |l: usize| rect.left() + 10.0 + l as f32 * lane_w;
                        let top = rect.top();
                        let mid = rect.center().y;
                        let bot = rect.bottom();
                        for &l in &row.through {
                            p.line_segment([egui::pos2(x(l), top), egui::pos2(x(l), bot)], Stroke::new(2.0, theme::lane_color(l)));
                        }
                        for &l in &row.incoming {
                            p.line_segment([egui::pos2(x(l), top), egui::pos2(x(row.lane), mid)], Stroke::new(2.0, theme::lane_color(l)));
                        }
                        for &l in &row.outgoing {
                            let col = theme::lane_color(if l == row.lane { row.lane } else { l });
                            p.line_segment([egui::pos2(x(row.lane), mid), egui::pos2(x(l), bot)], Stroke::new(2.0, col));
                        }
                        let col = theme::lane_color(row.lane);
                        if c.parents.len() > 1 {
                            p.circle_filled(egui::pos2(x(row.lane), mid), 3.5, col);
                        } else {
                            p.circle_filled(egui::pos2(x(row.lane), mid), 4.5, col);
                            p.circle_filled(egui::pos2(x(row.lane), mid), 2.0, theme::PANEL);
                        }
                    }
                }
                // refs + subject
                let date_w = 120.0;
                let sha_w = 70.0;
                let author_w = 170.0;
                let right = rect.right() - 8.0;
                let subj_right = right - date_w - sha_w - author_w - 12.0;
                let mut x = rect.left() + graph_w + 6.0;
                for rf in &c.refs {
                    let (label, fill, fg) = if let Some(b) = rf.strip_prefix("HEAD -> ") {
                        (format!("✔ {b}"), Color32::from_rgb(0x3b, 0x3b, 0x3e), Color32::WHITE)
                    } else if let Some(t) = rf.strip_prefix("tag: ") {
                        (format!("🏷 {t}"), Color32::from_rgb(0x4a, 0x3d, 0x1c), theme::YELLOW)
                    } else if rf == "HEAD" {
                        ("HEAD".to_string(), Color32::from_rgb(0x3b, 0x3b, 0x3e), Color32::WHITE)
                    } else if r.local.iter().any(|b| &b.name == rf) {
                        (rf.clone(), Color32::from_rgb(0x2a, 0x45, 0x63), Color32::WHITE)
                    } else {
                        (rf.clone(), Color32::from_rgb(0x33, 0x33, 0x33), theme::TEXT_DIM)
                    };
                    let galley = p.layout_no_wrap(label, FontId::proportional(12.0), fg);
                    let bw = galley.size().x + 10.0;
                    if x + bw > subj_right - 40.0 {
                        break;
                    }
                    let br = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 3.5), Vec2::new(bw, row_h - 7.0));
                    p.rect_filled(br, 4, fill);
                    if rf.starts_with("HEAD -> ") {
                        p.rect_stroke(br, 4, Stroke::new(1.0, theme::ORANGE), egui::StrokeKind::Inside);
                    }
                    p.galley(egui::pos2(x + 5.0, br.center().y - galley.size().y / 2.0), galley, fg);
                    x += bw + 4.0;
                }
                let subj_font = FontId::proportional(13.0);
                let subj_rect = egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(subj_right, rect.bottom()));
                let ps = ui.painter_at(subj_rect);
                ps.text(egui::pos2(x + 2.0, rect.center().y), Align2::LEFT_CENTER, &c.subject, subj_font, theme::TEXT);
                // author with avatar
                let ax = right - date_w - sha_w - author_w;
                let ac = theme::avatar_color(&c.author);
                let av = egui::Rect::from_center_size(egui::pos2(ax + 8.0, rect.center().y), Vec2::splat(15.0));
                p.rect_filled(av, 3, ac);
                p.text(av.center(), Align2::CENTER_CENTER, initials(&c.author), FontId::proportional(8.5), Color32::WHITE);
                let pa = ui.painter_at(egui::Rect::from_min_max(egui::pos2(ax + 19.0, rect.top()), egui::pos2(ax + author_w - 4.0, rect.bottom())));
                pa.text(egui::pos2(ax + 20.0, rect.center().y), Align2::LEFT_CENTER, &c.author, FontId::proportional(12.5), theme::TEXT);
                p.text(egui::pos2(right - date_w - sha_w, rect.center().y), Align2::LEFT_CENTER, &c.short, FontId::monospace(12.0), theme::TEXT_DIM);
                p.text(egui::pos2(right - date_w, rect.center().y), Align2::LEFT_CENTER, &c.date, FontId::proportional(12.5), theme::TEXT_DIM);

                if resp.clicked() {
                    acts.push(Act::SelectCommit(i));
                }
                let sha = c.sha.clone();
                let short = c.short.clone();
                let subject = c.subject.clone();
                let parents = c.parents.len();
                let head2 = head.clone();
                resp.context_menu(|ui| commit_menu(ui, acts, &sha, &short, &subject, parents, &head2));
            }
        });
    }

    // -----------------------------------------------------------------------------------------
    // Local changes

    fn changes_view(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>, ctx: &egui::Context) {
        let mut stage_hunk: Option<(String, bool, bool)> = None; // (patch, reverse, cached)
        let mut do_commit = false;
        {
            let r = &mut self.tabs[self.active];
            let unstaged: Vec<FileStatus> = r.status.iter().filter(|f| f.unstaged()).cloned().collect();
            let staged: Vec<FileStatus> = r.status.iter().filter(|f| f.staged()).cloned().collect();
            let has_head = !r.commits.is_empty();

            egui::Panel::left("changes_files").resizable(true).default_size(420.0).min_size(260.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(8)).show(ui, |ui| {
                // commit box at the bottom
                egui::Panel::bottom("commit_box").resizable(false).frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 8, bottom: 0 })).show(ui, |ui| {
                    ui.label(RichText::new("COMMIT – vorgemerkte Änderungen speichern").size(11.0).strong().color(theme::TEXT_DIM));
                    ui.add(egui::TextEdit::multiline(&mut r.commit_msg).hint_text("Beschreibung: Was hast du geändert und warum?").desired_rows(4).desired_width(f32::INFINITY));
                    ui.checkbox(&mut r.amend, "Letzten Commit ändern (amend)").on_hover_text("Statt eines neuen Commits wird der letzte ersetzt. Nicht verwenden, wenn er schon gepusht ist.");
                    let preview = if r.amend {
                        if r.commit_msg.trim().is_empty() { "git commit --amend --no-edit".to_string() } else { format!("git commit --amend -m {}", catalog::quote_for_display(r.commit_msg.trim())) }
                    } else {
                        if r.commit_msg.trim().is_empty() { "git commit -m \"<Beschreibung>\"".to_string() } else { format!("git commit -m {}", catalog::quote_for_display(r.commit_msg.trim())) }
                    };
                    ui.label(RichText::new(preview.lines().next().unwrap_or("")).monospace().size(12.0).color(Color32::from_rgb(0x9f, 0xd1, 0xff)));
                    let can = (!r.commit_msg.trim().is_empty() || r.amend) && (!staged.is_empty() || r.amend);
                    let label = if r.amend { "✔ Letzten Commit ändern".to_string() } else { format!("✔ Commit ({} Dateien)", staged.len()) };
                    let btn = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE)).fill(if can { Color32::from_rgb(0x2c, 0x6f, 0xbb) } else { Color32::from_rgb(0x3a, 0x3a, 0x3d) });
                    if ui.add_enabled(can, btn).on_hover_text(preview.clone()).on_disabled_hover_text("Erst Dateien vormerken und eine Beschreibung eingeben").clicked() {
                        do_commit = true;
                    }
                });

                egui::ScrollArea::vertical().id_salt("files").auto_shrink([false; 2]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("NICHT VORGEMERKT ({})", unstaged.len())).size(11.0).strong().color(theme::TEXT_DIM));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !unstaged.is_empty() && ui.small_button("Alle vormerken").on_hover_text("git add -A\nMerkt alle Änderungen für den nächsten Commit vor").clicked() {
                                acts.push(Act::Quick { title: "Alle Änderungen vormerken".into(), args: vec!["add".into(), "-A".into()], explain: String::new() });
                            }
                        });
                    });
                    if unstaged.is_empty() {
                        ui.label(RichText::new("Keine Änderungen.").color(theme::TEXT_DIM));
                    }
                    for f in &unstaged {
                        file_row(ui, acts, f, false, r.sel_file.as_ref() == Some(&(f.path.clone(), false)), has_head);
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("VORGEMERKT ({})", staged.len())).size(11.0).strong().color(theme::TEXT_DIM));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !staged.is_empty() && ui.small_button("Alle zurücknehmen").on_hover_text("git reset\nNimmt alles aus der Vormerkung, Dateien bleiben unverändert").clicked() {
                                let args = if has_head { vec!["reset".into(), "-q".into()] } else { vec!["rm".into(), "--cached".into(), "-r".into(), "-q".into(), ".".into()] };
                                acts.push(Act::Quick { title: "Vormerkung aufheben".into(), args, explain: String::new() });
                            }
                        });
                    });
                    if staged.is_empty() {
                        ui.label(RichText::new("Noch nichts vorgemerkt. Klicke bei einer Datei auf +.").color(theme::TEXT_DIM));
                    }
                    for f in &staged {
                        file_row(ui, acts, f, true, r.sel_file.as_ref() == Some(&(f.path.clone(), true)), has_head);
                    }
                });
            });

            // diff of selected file with hunk buttons
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(8)).show(ui, |ui| {
                let Some((path, staged_sel)) = r.sel_file.clone() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Wähle links eine Datei, um ihre Änderungen zu sehen.\n\nGrün = hinzugefügte Zeilen, Rot = entfernte Zeilen.").color(theme::TEXT_DIM).size(14.0));
                    });
                    return;
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&path).strong());
                    ui.label(RichText::new(if staged_sel { "(vorgemerkt)" } else { "(nicht vorgemerkt)" }).color(theme::TEXT_DIM));
                });
                let pd = git::parse_diff(&r.file_diff);
                let untracked = r.status.iter().any(|f| f.path == path && f.index == '?');
                egui::ScrollArea::both().id_salt("wdiff").auto_shrink([false; 2]).show(ui, |ui| {
                    if pd.hunks.is_empty() {
                        diff_lines(ui, &r.file_diff);
                    }
                    for (hi, h) in pd.hunks.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Abschnitt {}", hi + 1)).color(theme::TEXT_DIM).size(12.0));
                            if !untracked {
                                let patch = format!("{}{}", pd.header, h);
                                if staged_sel {
                                    if ui.small_button("− Abschnitt zurücknehmen").on_hover_text("git apply --cached --reverse\nNimmt nur diesen Abschnitt aus der Vormerkung").clicked() {
                                        stage_hunk = Some((patch.clone(), true, true));
                                    }
                                } else {
                                    if ui.small_button("+ Abschnitt vormerken").on_hover_text("git apply --cached\nMerkt nur diesen Abschnitt für den Commit vor").clicked() {
                                        stage_hunk = Some((patch.clone(), false, true));
                                    }
                                    if ui.small_button("Abschnitt verwerfen").on_hover_text("git apply --reverse\nVerwirft nur diese Änderung in der Datei – nicht rückgängig zu machen!").clicked() {
                                        stage_hunk = Some((patch.clone(), true, false));
                                    }
                                }
                            }
                        });
                        diff_lines(ui, h);
                        ui.add_space(6.0);
                    }
                });
            });
        }
        if let Some((patch, reverse, cached)) = stage_hunk {
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
                (false, true) => "Abschnitt vormerken",
                (true, true) => "Abschnitt aus Vormerkung nehmen",
                _ => "Abschnitt verwerfen",
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
            }
            r.commit_msg.clear();
            r.amend = false;
            self.quick("Commit erstellen".into(), args, ctx);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Library

    fn library_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let sug = Suggest::from(self.repo());
        let mut run = false;
        egui::Panel::left("lib_list").resizable(true).default_size(340.0).min_size(240.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(8)).show(ui, |ui| {
            ui.label(RichText::new(format!("ALLE GIT-BEFEHLE ({})", self.cmds.len())).size(11.0).strong().color(theme::TEXT_DIM));
            ui.add(egui::TextEdit::singleline(&mut self.lib_search).hint_text("🔍 Was möchtest du tun? z.B. „rückgängig“, „branch“").desired_width(f32::INFINITY));
            let q = self.lib_search.to_lowercase();
            egui::ScrollArea::vertical().id_salt("lib_scroll").auto_shrink([false; 2]).show(ui, |ui| {
                for (cat, name, desc) in CATS {
                    let items: Vec<usize> = self
                        .cmds
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.cat == *cat)
                        .filter(|(_, c)| q.is_empty() || c.title.to_lowercase().contains(&q) || c.base.to_lowercase().contains(&q) || c.summary.to_lowercase().contains(&q))
                        .map(|(i, _)| i)
                        .collect();
                    if items.is_empty() {
                        continue;
                    }
                    let open = !q.is_empty() || self.lib_form.as_ref().map(|f| self.cmds[f.cmd].cat == *cat).unwrap_or(false);
                    egui::CollapsingHeader::new(RichText::new(format!("{name} ({})", items.len())).strong())
                        .id_salt(("cat", *name))
                        .open(if !q.is_empty() { Some(true) } else { None })
                        .default_open(open)
                        .show(ui, |ui| {
                            ui.label(RichText::new(*desc).size(11.5).color(theme::TEXT_DIM));
                            for i in items {
                                let c = &self.cmds[i];
                                let sel = self.lib_form.as_ref().map(|f| f.cmd == i).unwrap_or(false);
                                let dot = match c.danger {
                                    Danger::Read => theme::GREEN,
                                    Danger::Change => theme::YELLOW,
                                    Danger::Risky => theme::RED,
                                };
                                let resp = ui
                                    .horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 16.0), Sense::hover());
                                        ui.painter().circle_filled(rect.center(), 3.0, dot);
                                        let base = if c.base.is_empty() { "…".to_string() } else { c.base.trim_start_matches('!').to_string() };
                                        ui.add(egui::Button::selectable(sel, RichText::new(c.title)))
                                            .on_hover_text(format!("git {base}\n\n{}", c.summary))
                                    })
                                    .inner;
                                if resp.clicked() {
                                    self.lib_form = Some(self.make_form(i, &[]));
                                }
                            }
                        });
                }
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(16)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("lib_form").auto_shrink([false; 2]).show(ui, |ui| {
                ui.set_max_width(820.0);
                if let Some(mut f) = self.lib_form.take() {
                    let args = self.form_args(&f);
                    let line = self.display_line(&f);
                    if form_ui(ui, &self.cmds, &mut f, &sug, &self.jobs, false, &line, &args) == FormAction::Run {
                        run = true;
                    }
                    self.lib_form = Some(f);
                }
            });
        });
        if run {
            if let Some(f) = self.lib_form.take() {
                let cmd = f.cmd;
                let values = f.values.clone();
                let extra = f.extra.clone();
                let free = f.free.clone();
                let id = self.run_form(f, ctx, false);
                self.lib_form = Some(Form { cmd, values, extra, free, last_job: id, open_after: None });
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Console

    fn console_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut run = false;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(12)).show(ui, |ui| {
            ui.label(RichText::new("KONSOLE – tippe einen beliebigen Git-Befehl").size(11.0).strong().color(theme::TEXT_DIM));
            ui.label(RichText::new("Hier kannst du jeden Befehl selbst eingeben. Tipp: Unter „Alle Git-Befehle“ findest du jeden Befehl auch als Formular mit Erklärung.").color(theme::TEXT_DIM));
            ui.horizontal(|ui| {
                ui.label(RichText::new("git").monospace().strong().color(theme::ORANGE));
                let resp = ui.add(egui::TextEdit::singleline(&mut self.console).hint_text("z.B. log --oneline --graph -15").desired_width(ui.available_width() - 110.0).font(egui::TextStyle::Monospace));
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    run = true;
                    resp.request_focus();
                }
                if ui.button("▶ Ausführen").clicked() {
                    run = true;
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().id_salt("console_out").auto_shrink([false; 2]).stick_to_bottom(true).show(ui, |ui| {
                for id in &self.console_jobs {
                    if let Some(j) = self.jobs.iter().find(|j| j.id == *id) {
                        ui.label(RichText::new(format!("> {}", j.display)).monospace().color(Color32::from_rgb(0x9f, 0xd1, 0xff)));
                        let (t, c) = job_status_text(j);
                        ui.add(egui::Label::new(RichText::new(j.output()).monospace().size(12.0)).wrap());
                        ui.label(RichText::new(t).color(c).size(11.5));
                        ui.separator();
                    }
                }
            });
        });
        if run && !self.console.trim().is_empty() {
            let idx = catalog::find(&self.cmds, "free");
            let mut f = self.make_form(idx, &[]);
            f.free = self.console.clone();
            if let Some(id) = self.run_form(f, ctx, true) {
                self.console_jobs.push(id);
            }
            self.console.clear();
        }
    }

    // -----------------------------------------------------------------------------------------
    // Welcome

    fn welcome(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Act>) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(RichText::new("YAGUI").size(40.0).strong().color(theme::ACCENT));
            ui.label(RichText::new("Git einfach sichtbar – jeder Befehl als Knopf, jede Aktion erklärt.").size(16.0));
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                let w = 3.0 * 190.0 + 20.0;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                for (icon, title, desc, act) in [
                    ("📂", "Repository öffnen", "Einen Ordner mit Git-Projekt öffnen", 0),
                    ("⬇", "Repository klonen", "git clone – von GitHub & Co. laden", 1),
                    ("✚", "Neues Repository", "git init – neues Projekt beginnen", 2),
                ] {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(190.0, 110.0), Sense::click());
                    ui.painter().rect_filled(rect, 8, if resp.hovered() { Color32::from_rgb(0x3a, 0x3a, 0x3e) } else { Color32::from_rgb(0x33, 0x33, 0x36) });
                    ui.painter().text(rect.center_top() + Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, icon, FontId::proportional(28.0), theme::ACCENT);
                    ui.painter().text(rect.center() + Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, title, FontId::proportional(14.5), theme::TEXT);
                    ui.painter().text(rect.center() + Vec2::new(0.0, 34.0), Align2::CENTER_CENTER, desc, FontId::proportional(11.0), theme::TEXT_DIM);
                    if resp.clicked() {
                        acts.push(match act {
                            0 => Act::OpenRepoDialog,
                            1 => Act::Open("clone", vec![]),
                            _ => Act::Open("init", vec![]),
                        });
                    }
                }
            });
            ui.add_space(24.0);
            if !self.recent.is_empty() {
                ui.label(RichText::new("ZULETZT GEÖFFNET").size(11.0).strong().color(theme::TEXT_DIM));
                for r in self.recent.clone() {
                    if ui.link(&r).clicked() {
                        acts.push(Act::OpenRepo(PathBuf::from(&r)));
                    }
                }
            }
            ui.add_space(16.0);
            if ui.button(format!("🧰  Alle {} Git-Befehle ansehen", self.cmds.len())).clicked() {
                acts.push(Act::QuickLaunch);
            }
        });
    }

    // -----------------------------------------------------------------------------------------
    // Dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        let sug = Suggest::from(self.repo());
        // Command dialog
        if let Some(mut f) = self.dialog.take() {
            let mut open = true;
            let mut act = FormAction::None;
            let args = self.form_args(&f);
            let line = self.display_line(&f);
            egui::Window::new(RichText::new("Git-Befehl").strong())
                .id(Id::new("cmd_dialog"))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_size([700.0, 680.0])
                .max_height(ctx.content_rect().height() - 80.0)
                .pivot(Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().id_salt("dlg_scroll").show(ui, |ui| {
                        act = form_ui(ui, &self.cmds, &mut f, &sug, &self.jobs, true, &line, &args);
                    });
                });
            match act {
                FormAction::Cancel => open = false,
                FormAction::Run => {
                    let cmd = f.cmd;
                    let values = f.values.clone();
                    let extra = f.extra.clone();
                    let free = f.free.clone();
                    let id = self.run_form(f, ctx, false);
                    f = Form { cmd, values, extra, free, last_job: id, open_after: None };
                    if self.rebase.is_some() {
                        open = false;
                    }
                }
                FormAction::None => {}
            }
            if open && self.confirm.is_none() {
                self.dialog = Some(f);
            } else if open {
                self.dialog = Some(f);
            }
        }

        // Confirmation for risky commands
        if let Some(f) = self.confirm.take() {
            let c = &self.cmds[f.cmd];
            let line = self.display_line(&f);
            let mut decision: Option<bool> = None;
            let modal = egui::Modal::new(Id::new("confirm")).show(ctx, |ui| {
                ui.set_max_width(520.0);
                ui.label(RichText::new("⚠ Bist du sicher?").heading().color(theme::RED));
                ui.add_space(4.0);
                ui.label(c.summary);
                ui.add_space(6.0);
                ui.label(RichText::new(&line).monospace().color(Color32::from_rgb(0x9f, 0xd1, 0xff)));
                ui.add_space(6.0);
                ui.label(RichText::new("Tipp: Mit „Reflog“ (Verlauf & Suche) lassen sich Commits oft wiederfinden – nicht gespeicherte Dateiänderungen aber nicht.").size(12.0).color(theme::TEXT_DIM));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Ja, ausführen").color(Color32::WHITE).strong()).fill(Color32::from_rgb(0xa8, 0x3a, 0x3a))).clicked() {
                        decision = Some(true);
                    }
                    if ui.button("Abbrechen").clicked() {
                        decision = Some(false);
                    }
                });
            });
            if modal.should_close() && decision.is_none() {
                decision = Some(false);
            }
            match decision {
                Some(true) => {
                    let id = self.run_form(Form { cmd: f.cmd, values: f.values.clone(), extra: f.extra.clone(), free: f.free.clone(), last_job: None, open_after: None }, ctx, true);
                    if let Some(d) = self.dialog.as_mut() {
                        if d.cmd == f.cmd {
                            d.last_job = id;
                        }
                    }
                    if let Some(d) = self.lib_form.as_mut() {
                        if d.cmd == f.cmd {
                            d.last_job = id;
                        }
                    }
                }
                Some(false) => {}
                None => self.confirm = Some(f),
            }
        }

        // Interactive rebase editor
        if self.rebase.is_some() {
            let mut open = true;
            let mut run = false;
            let mut cancel = false;
            egui::Window::new(RichText::new("Interaktiver Rebase – Commits bearbeiten").strong())
                .id(Id::new("rebase_dialog"))
                .open(&mut open)
                .collapsible(false)
                .default_width(760.0)
                .pivot(Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    let d = self.rebase.as_mut().unwrap();
                    ui.label("Lege für jeden Commit fest, was passieren soll. Oben ist der älteste Commit. Mit ▲▼ änderst du die Reihenfolge.");
                    danger_badge(ui, Danger::Risky);
                    if d.has_merges {
                        ui.colored_label(theme::YELLOW, "Hinweis: Merge-Commits in diesem Bereich werden ausgelassen (Git glättet sie beim Rebase).");
                    }
                    if d.items.is_empty() {
                        ui.colored_label(theme::YELLOW, "Nach diesem Commit gibt es keine Commits im aktuellen Branch.");
                    }
                    ui.add_space(6.0);
                    let mut swap: Option<(usize, usize)> = None;
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        let n = d.items.len();
                        for (i, it) in d.items.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                if ui.add_enabled(i > 0, egui::Button::new("▲").small()).clicked() {
                                    swap = Some((i, i - 1));
                                }
                                if ui.add_enabled(i + 1 < n, egui::Button::new("▼").small()).clicked() {
                                    swap = Some((i, i + 1));
                                }
                                let col = match RB_ACTIONS[it.action].0 {
                                    "drop" => theme::RED,
                                    "squash" | "fixup" => theme::YELLOW,
                                    "reword" | "edit" => theme::ACCENT,
                                    _ => theme::GREEN,
                                };
                                egui::ComboBox::from_id_salt(("rb", i, &it.short)).width(230.0).selected_text(RichText::new(format!("{} ({})", RB_ACTIONS[it.action].1, RB_ACTIONS[it.action].0)).color(col)).show_ui(ui, |ui| {
                                    for (ai, (a, l, h)) in RB_ACTIONS.iter().enumerate() {
                                        if i == 0 && (*a == "squash" || *a == "fixup") {
                                            continue;
                                        }
                                        ui.selectable_value(&mut it.action, ai, format!("{l} ({a})")).on_hover_text(*h);
                                    }
                                });
                                ui.label(RichText::new(&it.short).monospace().color(theme::TEXT_DIM));
                                if RB_ACTIONS[it.action].0 == "reword" {
                                    ui.add(egui::TextEdit::singleline(&mut it.new_msg).desired_width(300.0));
                                } else {
                                    let s = RichText::new(&it.subject);
                                    ui.label(if RB_ACTIONS[it.action].0 == "drop" { s.strikethrough().color(theme::TEXT_DIM) } else { s });
                                }
                            });
                        }
                    });
                    if let Some((a, b)) = swap {
                        d.items.swap(a, b);
                    }
                    ui.add_space(6.0);
                    egui::Frame::new().fill(Color32::from_rgb(0x1a, 0x1a, 0x1a)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("DAS WIRD AUSGEFÜHRT").size(11.0).color(theme::TEXT_DIM).strong());
                        ui.label(RichText::new(format!("git rebase -i {}", d.base)).monospace().size(14.0).color(Color32::from_rgb(0x9f, 0xd1, 0xff)));
                        ui.label(RichText::new("mit dieser Aufgabenliste (statt eines Texteditors):").size(12.0).color(theme::TEXT_DIM));
                        ui.label(RichText::new(App::rebase_todo(d, &[])).monospace().size(12.0));
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let can = !d.items.is_empty();
                        if ui.add_enabled(can, egui::Button::new(RichText::new("▶ Rebase starten").strong().color(Color32::WHITE)).fill(Color32::from_rgb(0x2c, 0x6f, 0xbb))).clicked() {
                            run = true;
                        }
                        if ui.button("Abbrechen").clicked() {
                            cancel = true;
                        }
                    });
                });
            if run {
                self.run_rebase(ctx);
            } else if cancel || !open {
                self.rebase = None;
            }
        }

        // Quick launch (command palette)
        if let Some(mut q) = self.quick.take() {
            let mut chosen: Option<usize> = None;
            let mut close = false;
            let modal = egui::Modal::new(Id::new("quick")).show(ctx, |ui| {
                ui.set_width(620.0);
                ui.label(RichText::new("Was möchtest du tun?").heading());
                let resp = ui.add(egui::TextEdit::singleline(&mut q).hint_text("z.B. „branch löschen“, „rückgängig“, „push“, „stash“").desired_width(f32::INFINITY));
                resp.request_focus();
                let ql = q.to_lowercase();
                let words: Vec<&str> = ql.split_whitespace().collect();
                let hits: Vec<usize> = self
                    .cmds
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| {
                        let hay = format!("{} {} {}", c.title, c.base, c.summary).to_lowercase();
                        words.iter().all(|w| hay.contains(w))
                    })
                    .map(|(i, _)| i)
                    .take(14)
                    .collect();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    chosen = hits.first().copied();
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for i in hits {
                        let c = &self.cmds[i];
                        let resp = ui.add_sized([ui.available_width(), 22.0], egui::Button::new(RichText::new(c.title)).frame(false));
                        let resp = resp.on_hover_text(c.summary);
                        ui.label(RichText::new(format!("git {} – {}", c.base.trim_start_matches('!'), c.summary)).size(11.5).color(theme::TEXT_DIM));
                        if resp.clicked() {
                            chosen = Some(i);
                        }
                    }
                });
            });
            if modal.should_close() {
                close = true;
            }
            if let Some(i) = chosen {
                let id = self.cmds[i].id;
                self.open_cmd(id, vec![]);
            } else if !close {
                self.quick = Some(q);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Context menus and rows

fn initials(name: &str) -> String {
    let mut s: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    if s.is_empty() {
        s = "?".into();
    }
    s.to_uppercase()
}

fn menu_item(ui: &mut egui::Ui, acts: &mut Vec<Act>, label: &str, cmd_hint: &str, act: Act) {
    let text = if cmd_hint.is_empty() { label.to_string() } else { format!("{label}") };
    let resp = ui.button(text);
    let resp = if cmd_hint.is_empty() { resp } else { resp.on_hover_text(RichText::new(cmd_hint).monospace()) };
    if resp.clicked() {
        acts.push(act);
        ui.close();
    }
}

fn branch_menu(ui: &mut egui::Ui, acts: &mut Vec<Act>, name: &str, upstream: &str, head: &str, remote: bool) {
    let n = name.to_string();
    ui.label(RichText::new(name).strong());
    ui.separator();
    if remote {
        menu_item(ui, acts, "Lokal auschecken …", &format!("git checkout --track {n}"), Act::Open("checkout_track", vec![("branch", n.clone())]));
        menu_item(ui, acts, &format!("In „{head}“ zusammenführen …"), &format!("git merge {n}"), Act::Open("merge", vec![("branch", n.clone())]));
        menu_item(ui, acts, &format!("„{head}“ darauf neu aufsetzen …"), &format!("git rebase {n}"), Act::Open("rebase", vec![("branch", n.clone())]));
        menu_item(ui, acts, "Neuen Branch von hier …", "git branch <name> <start>", Act::Open("branch_create", vec![("commit", n.clone())]));
        if let Some((r, b)) = n.split_once('/') {
            menu_item(ui, acts, "Auf dem Server löschen …", &format!("git push {r} --delete {b}"), Act::Open("push_delete", vec![("remote", r.to_string()), ("branch", b.to_string())]));
        }
    } else {
        let is_head = n == head;
        if !is_head {
            menu_item(ui, acts, "Wechseln zu diesem Branch …", &format!("git switch {n}"), Act::Open("switch", vec![("branch", n.clone())]));
            menu_item(ui, acts, &format!("In „{head}“ zusammenführen …"), &format!("git merge {n}"), Act::Open("merge", vec![("branch", n.clone())]));
            menu_item(ui, acts, &format!("„{head}“ auf „{n}“ neu aufsetzen …"), &format!("git rebase {n}"), Act::Open("rebase", vec![("branch", n.clone())]));
        }
        menu_item(ui, acts, "Neuen Branch von hier …", "git branch <name> <start>", Act::Open("branch_create", vec![("commit", n.clone())]));
        menu_item(ui, acts, "Umbenennen …", &format!("git branch -m {n} <neu>"), Act::Open("branch_rename", vec![("branch", n.clone())]));
        menu_item(ui, acts, "Hochladen (Push) …", &format!("git push origin {n}"), Act::Open("push", vec![("branch", n.clone())]));
        if !upstream.is_empty() {
            menu_item(ui, acts, &format!("Neues von {upstream} holen (Pull) …"), "git pull", Act::Open("pull", vec![]));
        }
        menu_item(ui, acts, "Server-Branch zuordnen …", "git branch --set-upstream-to=<remote/branch>", Act::Open("branch_upstream", vec![("branch", n.clone())]));
        menu_item(ui, acts, "Tag hier erstellen …", "git tag <name> <branch>", Act::Open("tag_create", vec![("commit", n.clone())]));
        menu_item(ui, acts, "Verlauf dieses Branches   (git log)", &format!("git log --oneline {n}"), Act::Open("log", vec![("range", n.clone())]));
        if !is_head {
            ui.separator();
            menu_item(ui, acts, "Branch löschen …", &format!("git branch -d {n}"), Act::Open("branch_delete", vec![("branch", n.clone())]));
        }
    }
    ui.separator();
    menu_item(ui, acts, "Namen kopieren", "", Act::Copy(n));
}

fn commit_menu(ui: &mut egui::Ui, acts: &mut Vec<Act>, sha: &str, short: &str, subject: &str, parents: usize, head: &str) {
    let s = sha.to_string();
    ui.label(RichText::new(format!("{short}  {}", subject.chars().take(50).collect::<String>())).strong());
    ui.separator();
    menu_item(ui, acts, "Diesen Stand auschecken …", &format!("git checkout {short}"), Act::Open("checkout", vec![("branch", s.clone())]));
    menu_item(ui, acts, "Neuen Branch hier …", &format!("git branch <name> {short}"), Act::Open("branch_create", vec![("commit", s.clone())]));
    menu_item(ui, acts, "Tag hier erstellen …", &format!("git tag <name> {short}"), Act::Open("tag_create", vec![("commit", s.clone())]));
    ui.separator();
    menu_item(ui, acts, &format!("In „{head}“ übernehmen (Cherry-Pick) …"), &format!("git cherry-pick {short}"), Act::Open("cherry_pick", vec![("commits", s.clone())]));
    let mut rv: Vec<(&'static str, String)> = vec![("commits", s.clone())];
    if parents > 1 {
        rv.push(("parent", "1".into()));
    }
    menu_item(ui, acts, "Rückgängig machen (Revert) …", &format!("git revert {short}"), Act::Open("revert", rv));
    menu_item(ui, acts, &format!("„{head}“ hierher zurücksetzen (Reset) …"), &format!("git reset --mixed {short}"), Act::Open("reset", vec![("commit", s.clone())]));
    menu_item(ui, acts, "Commits danach bearbeiten (Interaktiver Rebase) …", &format!("git rebase -i {short}"), Act::Rebase(s.clone()));
    menu_item(ui, acts, &format!("„{head}“ hierauf neu aufsetzen (Rebase) …"), &format!("git rebase {short}"), Act::Open("rebase", vec![("branch", s.clone())]));
    menu_item(ui, acts, &format!("In „{head}“ zusammenführen (Merge) …"), &format!("git merge {short}"), Act::Open("merge", vec![("branch", s.clone())]));
    ui.separator();
    menu_item(ui, acts, "Als Patch-Datei speichern …", &format!("git format-patch -1 {short}"), Act::Open("format_patch", vec![("range", format!("-1 {short}"))]));
    menu_item(ui, acts, "Mit Arbeitsordner vergleichen", &format!("git diff {short}"), Act::Open("diff", vec![("a", s.clone())]));
    menu_item(ui, acts, "Beschreiben (describe)", &format!("git describe --tags {short}"), Act::Open("describe", vec![("commit", s.clone()), ("flag:--always", "1".into())]));
    menu_item(ui, acts, "Notiz anhängen …", "git notes add", Act::Open("notes_add", vec![("commit", s.clone())]));
    ui.separator();
    menu_item(ui, acts, "SHA kopieren", "", Act::Copy(s.clone()));
    menu_item(ui, acts, "Beschreibung kopieren", "", Act::Copy(subject.to_string()));
}

fn file_row(ui: &mut egui::Ui, acts: &mut Vec<Act>, f: &FileStatus, staged: bool, selected: bool, has_head: bool) {
    let code = if staged { f.index } else if f.index == '?' { '?' } else { f.worktree };
    let conflict = f.conflicted();
    ui.horizontal(|ui| {
        let btn = if staged { "−" } else { "+" };
        let (tip, args): (String, Vec<String>) = if staged {
            if has_head {
                (format!("Aus Vormerkung nehmen\ngit restore --staged -- {}", f.path), vec!["restore".into(), "--staged".into(), "--".into(), f.path.clone()])
            } else {
                (format!("Aus Vormerkung nehmen\ngit rm --cached -- {}", f.path), vec!["rm".into(), "--cached".into(), "-q".into(), "--".into(), f.path.clone()])
            }
        } else {
            (format!("Für den Commit vormerken\ngit add -- {}", f.path), vec!["add".into(), "--".into(), f.path.clone()])
        };
        if ui.small_button(btn).on_hover_text(&tip).clicked() {
            acts.push(Act::Quick { title: if staged { format!("Vormerkung aufheben: {}", f.path) } else { format!("Vormerken: {}", f.path) }, args, explain: tip.clone() });
        }
        status_badge(ui, if conflict { 'U' } else { code });
        let label = match &f.orig {
            Some(o) => format!("{o} → {}", f.path),
            None => f.path.clone(),
        };
        let resp = ui.add(egui::Button::selectable(selected, RichText::new(label).size(13.0)));
        let resp = resp.on_hover_text(format!("{} ({})\nRechtsklick: weitere Aktionen", f.path, status_word(if conflict { 'U' } else { code })));
        if resp.clicked() {
            acts.push(Act::SelectFile(f.path.clone(), staged));
        }
        let p = f.path.clone();
        let untracked = f.index == '?';
        resp.context_menu(|ui| {
            ui.label(RichText::new(&p).strong());
            ui.separator();
            if conflict {
                menu_item(ui, acts, "Konflikt: meine Version nehmen", &format!("git checkout --ours -- {p}"), Act::Quick { title: format!("Meine Version: {p}"), args: vec!["checkout".into(), "--ours".into(), "--".into(), p.clone()], explain: String::new() });
                menu_item(ui, acts, "Konflikt: andere Version nehmen", &format!("git checkout --theirs -- {p}"), Act::Quick { title: format!("Andere Version: {p}"), args: vec!["checkout".into(), "--theirs".into(), "--".into(), p.clone()], explain: String::new() });
                menu_item(ui, acts, "Als gelöst markieren", &format!("git add -- {p}"), Act::Quick { title: format!("Konflikt gelöst: {p}"), args: vec!["add".into(), "--".into(), p.clone()], explain: String::new() });
                menu_item(ui, acts, "Im Merge-Programm öffnen …", &format!("git mergetool {p}"), Act::Open("mergetool", vec![("paths", p.clone())]));
                ui.separator();
            }
            if staged {
                menu_item(ui, acts, "Aus Vormerkung nehmen", &format!("git restore --staged -- {p}"), Act::Open("restore", vec![("paths", p.clone()), ("flag:--staged", "1".into())]));
            } else {
                menu_item(ui, acts, "Vormerken", &format!("git add -- {p}"), Act::Quick { title: format!("Vormerken: {p}"), args: vec!["add".into(), "--".into(), p.clone()], explain: String::new() });
                if untracked {
                    menu_item(ui, acts, "Neue Datei löschen …", &format!("git clean -f -- {p}"), Act::Open("clean", vec![("paths", p.clone())]));
                } else {
                    menu_item(ui, acts, "Änderungen verwerfen …", &format!("git restore -- {p}"), Act::Open("restore", vec![("paths", p.clone())]));
                }
            }
            if !untracked {
                menu_item(ui, acts, "Nicht mehr verfolgen (Datei bleibt) …", &format!("git rm --cached -- {p}"), Act::Open("rm", vec![("paths", p.clone()), ("flag:--cached", "1".into())]));
                menu_item(ui, acts, "Wer hat was geändert? (Blame)", &format!("git blame -- {p}"), Act::Open("blame", vec![("file", p.clone())]));
                menu_item(ui, acts, "Verlauf dieser Datei", &format!("git log --follow -- {p}"), Act::Open("log", vec![("paths", p.clone()), ("flag:--follow", "1".into())]));
                menu_item(ui, acts, "Änderungen vorübergehend ignorieren …", &format!("git update-index --assume-unchanged {p}"), Act::Open("assume_unchanged", vec![("paths", p.clone())]));
            } else {
                menu_item(ui, acts, "Warum ignoriert / nicht ignoriert?", &format!("git check-ignore -v {p}"), Act::Open("check_ignore", vec![("paths", p.clone())]));
            }
            menu_item(ui, acts, "Pfad kopieren", "", Act::Copy(p.clone()));
        });
    });
}

fn detail_pane(ui: &mut egui::Ui, r: &mut Repo, acts: &mut Vec<Act>) {
    // tab header
    egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
        ui.horizontal(|ui| {
            for (t, label) in [(DetailTab::Commit, "Commit"), (DetailTab::Changes, "Änderungen"), (DetailTab::Tree, "Dateibaum")] {
                let sel = r.detail_tab == t;
                let txt = RichText::new(label).size(14.0).strong().color(if sel { theme::ACCENT } else { theme::TEXT });
                let resp = ui.add(egui::Button::new(txt).frame(false));
                if sel {
                    let rr = resp.rect;
                    ui.painter().line_segment([rr.left_bottom() + Vec2::new(0.0, 3.0), rr.right_bottom() + Vec2::new(0.0, 3.0)], Stroke::new(2.0, theme::ACCENT));
                }
                if resp.clicked() {
                    acts.push(Act::DetailTab(t));
                }
                ui.add_space(14.0);
            }
        });
    });
    ui.separator();
    let Some(d) = &r.detail else {
        ui.label(RichText::new("Wähle oben einen Commit aus.").color(theme::TEXT_DIM));
        return;
    };
    match r.detail_tab {
        DetailTab::Commit => {
            egui::ScrollArea::vertical().id_salt("detail_commit").auto_shrink([false; 2]).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 8)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(42.0, 42.0), Sense::hover());
                        ui.painter().rect_filled(rect, 5, theme::avatar_color(&d.author));
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, initials(&d.author), FontId::proportional(18.0), Color32::WHITE);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("AUTOR").size(11.0).color(theme::TEXT_DIM));
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&d.author).strong());
                                ui.label(RichText::new(&d.email).color(theme::TEXT_DIM).size(12.0));
                            });
                            ui.label(RichText::new(&d.date).size(12.0));
                        });
                    });
                    ui.add_space(6.0);
                    egui::Grid::new("meta").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                        if !d.refs.is_empty() {
                            ui.label(RichText::new("REFS").size(11.0).color(theme::TEXT_DIM));
                            ui.label(&d.refs);
                            ui.end_row();
                        }
                        ui.label(RichText::new("SHA").size(11.0).color(theme::TEXT_DIM));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&d.sha).monospace());
                            if ui.small_button("Kopieren").clicked() {
                                acts.push(Act::Copy(d.sha.clone()));
                            }
                        });
                        ui.end_row();
                        ui.label(RichText::new("ELTERN").size(11.0).color(theme::TEXT_DIM));
                        ui.horizontal(|ui| {
                            for p in &d.parents {
                                if ui.link(&p[..p.len().min(8)]).on_hover_text("Zu diesem Commit springen").clicked() {
                                    acts.push(Act::JumpToRef(p.clone()));
                                }
                            }
                        });
                        ui.end_row();
                        if d.committer != d.author && !d.committer.is_empty() {
                            ui.label(RichText::new("COMMITTER").size(11.0).color(theme::TEXT_DIM));
                            ui.label(&d.committer);
                            ui.end_row();
                        }
                    });
                    ui.separator();
                    let mut lines = d.message.lines();
                    ui.label(RichText::new(lines.next().unwrap_or("")).strong().size(15.0));
                    let rest: String = lines.collect::<Vec<_>>().join("\n");
                    if !rest.trim().is_empty() {
                        ui.label(RichText::new(rest.trim()).monospace().size(12.0));
                    }
                    ui.separator();
                    ui.label(RichText::new(format!("{} GEÄNDERTE DATEIEN", d.files.len())).size(11.0).color(theme::TEXT_DIM));
                    for (st, path) in &d.files {
                        ui.horizontal(|ui| {
                            status_badge(ui, st.chars().next().unwrap_or('M'));
                            if ui.add(egui::Button::new(path).frame(false)).on_hover_text(format!("{} – klicken für die Änderungen", status_word(st.chars().next().unwrap_or('M')))).clicked() {
                                acts.push(Act::DetailFile(path.clone()));
                            }
                        });
                    }
                });
            });
        }
        DetailTab::Changes => {
            egui::Panel::left("detail_files").resizable(true).default_size(280.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(6)).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("dfiles").auto_shrink([false; 2]).show(ui, |ui| {
                    for (st, path) in &d.files {
                        ui.horizontal(|ui| {
                            status_badge(ui, st.chars().next().unwrap_or('M'));
                            let sel = r.detail_file.as_deref() == Some(path.as_str());
                            if ui.add(egui::Button::selectable(sel, path.as_str())).clicked() {
                                acts.push(Act::DetailFile(path.clone()));
                            }
                        });
                    }
                });
            });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(6)).show(ui, |ui| {
                if r.detail_file.is_some() {
                    diff_view(ui, &r.detail_diff, "ddiff");
                } else {
                    ui.label(RichText::new("Wähle links eine Datei.").color(theme::TEXT_DIM));
                }
            });
        }
        DetailTab::Tree => {
            egui::Panel::left("tree_files").resizable(true).default_size(320.0).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(6)).show(ui, |ui| {
                ui.label(RichText::new(format!("{} Dateien in diesem Stand   (git ls-tree -r)", r.tree.len())).size(11.0).color(theme::TEXT_DIM));
                egui::ScrollArea::vertical().id_salt("tfiles").auto_shrink([false; 2]).show_rows(ui, 20.0, r.tree.len(), |ui, range| {
                    for i in range {
                        let p = &r.tree[i];
                        let sel = r.tree_file.as_deref() == Some(p.as_str());
                        if ui.add(egui::Button::selectable(sel, p.as_str())).clicked() {
                            acts.push(Act::TreeFile(p.clone()));
                        }
                    }
                });
            });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(6)).show(ui, |ui| {
                egui::ScrollArea::both().id_salt("tcontent").auto_shrink([false; 2]).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&r.tree_content).monospace().size(12.0)).extend());
                });
            });
        }
    }
}

fn open_terminal(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("cmd").args(["/c", "start", "", "cmd"]).current_dir(path).creation_flags(0x0800_0000).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").args(["-a", "Terminal"]).arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("x-terminal-emulator").current_dir(path).spawn();
    }
}

fn open_explorer(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}
