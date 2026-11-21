//! Application state and logic: repositories, running git, jobs, interactive rebase.
//! The window itself is drawn in `app/ui.rs` and `app/dialogs.rs`.

mod auth;
mod dialogs;
mod extra;
mod manager;
mod tree;
mod ui;
pub mod updates;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui;

use crate::catalog::{self, Arg, Cmd, Danger, Kind, Value, P};
use crate::i18n::{self, tr, trs, Lang};
use crate::git::{self, Commit, CommitDetail, FileStatus, Job, JobSpec, JobState, RefInfo};
use crate::graph;
use crate::theme;

// ---------------------------------------------------------------------------------------------
// State

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Changes,
    Commits,
}

/// The View menu's switches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewOpt {
    Details,
    Head,
    Tags,
    Stashes,
    Reflog,
    FirstParent,
    ActiveBranch,
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
    commit_desc: String,
    amend: bool,
    scroll_to: Option<usize>,
    error: Option<String>,
    log_opts: git::LogOptions,
    git_dir: Option<PathBuf>,
    snap: git::Snapshot,
    locked: Option<&'static str>,
    /// Commit search: 0 message/author/SHA, 1 path, 2 diff content.
    search_mode: usize,
    search_hits: Option<std::collections::HashSet<String>>,
    /// Local branches, most recently committed first (sidebar sort "Recently used").
    branch_recency: Vec<String>,
    /// (stash@{n}, commit) of every stash.
    stash_shas: Vec<(String, String)>,
    /// The stash picked in the sidebar (highlighted there).
    stash_sel: Option<String>,
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
            commit_desc: String::new(),
            amend: false,
            scroll_to: None,
            error: None,
            log_opts: git::LogOptions { all: true, ..Default::default() },
            git_dir: None,
            snap: vec![],
            locked: None,
            search_mode: 0,
            search_hits: None,
            branch_recency: vec![],
            stash_shas: vec![],
            stash_sel: None,
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
            self.error = Some(tr("This folder is not a Git repository (any more).", "Dieser Ordner ist (nicht mehr) ein Git-Repository.").into());
            return;
        }
        self.error = None;
        self.git_dir = git::git_dir(&p);
        self.reload(u32::MAX);
    }

    /// Reloads only what a change touched, using the reload flags of the .git folder watch.
    fn reload(&mut self, flags: u32) {
        let p = self.path.clone();
        if flags & (git::RELOAD_HEAD | git::RELOAD_REFS | git::RELOAD_REFTABLE) != 0 {
            self.head = git::head_name(&p);
            let sel_sha = self.selected.and_then(|i| self.commits.get(i)).map(|c| c.sha.clone());
            let stashes = git::stash_commits(&p);
            self.log_opts.stash_tips = stashes.iter().map(|s| s.1.clone()).collect();
            self.commits = git::load_commits(&p, 3000, &self.log_opts);
            // Show every stash as one row labelled stash@{n} and leave out
            // Git's helper commits ("index on …", "untracked files on …").
            let helpers: std::collections::HashSet<&String> = stashes.iter().flat_map(|s| s.2.iter().skip(1)).collect();
            self.commits.retain(|c| !helpers.contains(&c.sha));
            for c in self.commits.iter_mut() {
                if let Some(s) = stashes.iter().find(|s| s.1 == c.sha) {
                    c.parents.truncate(1);
                    c.refs.retain(|r| r != "refs/stash");
                    c.refs.insert(0, s.0.clone());
                }
            }
            self.stash_shas = stashes.iter().map(|s| (s.0.clone(), s.1.clone())).collect();
            let (rows, lanes) = graph::layout(&self.commits);
            self.rows = rows;
            self.lanes = lanes.min(14);
            let (l, r, t) = git::load_refs(&p);
            self.local = l;
            self.remote_branches = r;
            self.tags = t;
            self.stashes = git::load_stashes(&p);
            self.branch_recency = git::run(&p, &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads"]).stdout.lines().map(String::from).collect();
            self.worktrees = git::load_worktrees(&p);
            self.selected = sel_sha.and_then(|s| self.commits.iter().position(|c| c.sha == s));
            if let Some(i) = self.selected {
                self.detail = None;
                self.select_commit(i);
            } else {
                self.detail = None;
            }
        }
        if flags & git::RELOAD_CONFIG != 0 {
            self.remotes = git::load_remotes(&p);
            self.submodules = git::load_submodules(&p);
        }
        if flags & (git::RELOAD_INDEX | git::RELOAD_IGNORE | git::RELOAD_HEAD | git::RELOAD_STATE) != 0 {
            self.status = git::load_status(&p);
            self.reload_file_diff();
        }
        if flags & (git::RELOAD_STATE | git::RELOAD_HEAD | git::RELOAD_INDEX) != 0 {
            self.state = git::repo_state(&p);
            self.locked = git::repo_locked(&p);
        }
        if let Some(g) = &self.git_dir {
            self.snap = git::snapshot(g);
        }
    }

    /// Looks at the watched .git files and reloads the parts that changed.
    fn poll_changes(&mut self) -> bool {
        let Some(g) = self.git_dir.clone() else { return false };
        let now = git::snapshot(&g);
        let flags = git::changed_flags(&self.snap, &now);
        if flags != 0 {
            self.reload(flags);
            true
        } else {
            false
        }
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

    /// (ahead, behind, upstream gone) from `%(upstream:track)`.
    fn ahead_behind(track: &str) -> (u32, u32, bool) {
        let (mut a, mut b, mut gone) = (0, 0, false);
        let t = track.trim_matches(['[', ']']);
        for part in t.split(", ") {
            if let Some(n) = part.strip_prefix("ahead ") {
                a = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                b = n.parse().unwrap_or(0);
            } else if part == "gone" {
                gone = true;
            }
        }
        (a, b, gone)
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

const RB_ACTIONS: &[&str] = &["pick", "reword", "edit", "squash", "fixup", "drop"];

/// (label, explanation) of an interactive-rebase action in the current language.
fn rb_label(action: &str) -> (&'static str, &'static str) {
    match action {
        "pick" => (tr("Pick", "Behalten"), tr("keep the commit as it is", "Commit bleibt unverändert")),
        "reword" => (tr("Reword", "Umbenennen"), tr("new message, same content", "neue Beschreibung, Inhalt bleibt")),
        "edit" => (tr("Edit", "Bearbeiten"), tr("the rebase stops here so you can change the commit", "Rebase hält hier an, du kannst den Commit ändern")),
        "squash" => (tr("Squash", "Zusammenfassen"), tr("combine with the commit above, keep both messages", "mit dem Commit darüber vereinen, beide Beschreibungen bleiben")),
        "fixup" => (tr("Fixup", "Einschmelzen"), tr("combine with the commit above, drop this message", "mit dem Commit darüber vereinen, diese Beschreibung entfällt")),
        _ => (tr("Drop", "Löschen"), tr("remove the commit", "Commit wird entfernt")),
    }
}

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
    Lang(Lang),
    ToggleActivity,
    ToggleConsole,
    Commands(bool),
    Ssh,
    Accounts,
    Prefs,
    /// Show the Repository Manager tab.
    Manager,
    /// Close every tab, or every tab but this one.
    CloseOthers(Option<usize>),
    MoveToWorkspace(usize, usize),
    SwitchWorkspace(usize),
    OpenInWorkspace(PathBuf, usize),
    RestoreTab,
    ToggleTabList,
    CloseManager,
    ViewOpt(ViewOpt),
    NewBranch(Option<String>),
    NewTag(Option<String>),
    Stats,
    Treemap,
    Bench,
    RepoSettings,
    Custom(usize, String),
    TabStep(i32),
    Theme(bool),
    Zoom(f32),
    Layout(bool),
    Shortcuts,
    About,
    /// Branch in the sidebar: show only its history / hide it / pin it.
    FilterBranch(String),
    HideBranch(String),
    PinBranch(String),
    Snapshot,
    RenameRepo(PathBuf),
    StaleBranches,
    RenameStash(String),
    StashPatch(String),
    /// Interactive-rebase quick action on one commit: 1 reword, 2 edit, 3 squash, 4 fixup, 5 drop.
    RebaseAction(String, usize),
    /// A stash clicked in the sidebar: select it in the history and show its changes.
    SelectStash(String),
    CheckUpdates,
    Install,
    Uninstall,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Saved {
    tabs: Vec<String>,
    active: usize,
    recent: Vec<String>,
    #[serde(default)]
    lang: Lang,
    #[serde(default)]
    shell: auth::ShellTool,
    #[serde(default)]
    accounts: Vec<auth::Account>,
    #[serde(default)]
    extra_keys: Vec<String>,
    #[serde(default)]
    flat_files: bool,
    #[serde(default)]
    show_tablist: bool,
    #[serde(default)]
    known_repos: Vec<String>,
    #[serde(default)]
    repo_names: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    src_dirs: Vec<String>,
    #[serde(default)]
    hidden_repos: Vec<String>,
    #[serde(default)]
    workspaces: Vec<manager::Workspace>,
    #[serde(default)]
    ws: usize,
    #[serde(default)]
    ws_in_title: bool,
    #[serde(default)]
    hide_details: bool,
    #[serde(default)]
    custom_cmds: Vec<extra::CustomCmd>,
    #[serde(default)]
    repo_cfg: extra::RepoCfg,
    #[serde(default)]
    prefs: extra::Prefs,
    #[serde(default)]
    pinned: Vec<String>,
    #[serde(default)]
    branch_sort: u8,
}

pub struct App {
    cmds: Vec<Cmd>,
    tabs: Vec<Repo>,
    active: usize,
    jobs: Vec<Job>,
    next_job: u64,
    dialog: Option<Form>,
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
    last_poll: f64,
    show_console: bool,
    notice: Option<u64>,
    lang: Lang,
    /// The "all Git commands" view, shown instead of history / changes.
    commands_view: bool,
    lib_search: String,
    lib_form: Option<Form>,
    ssh: Option<auth::SshState>,
    creds: Option<auth::CredState>,
    prefs: Option<auth::PrefsState>,
    shell: auth::ShellTool,
    editors: Vec<auth::Editor>,
    extra_keys: Vec<PathBuf>,
    accounts: Vec<auth::Account>,
    /// Show changed files as a plain list instead of a folder tree.
    flat_files: bool,
    show_tablist: bool,
    manager_tab: bool,
    manager_active: bool,
    mgr: manager::ManagerState,
    known_repos: Vec<PathBuf>,
    repo_names: std::collections::BTreeMap<String, String>,
    src_dirs: Vec<PathBuf>,
    hidden_repos: Vec<String>,
    workspaces: Vec<manager::Workspace>,
    ws: usize,
    ws_in_title: bool,
    ws_dialog: Option<manager::WsDialog>,
    treemap: Option<manager::Treemap>,
    bench: Option<manager::Bench>,
    closed_tabs: Vec<PathBuf>,
    prefs_tab_indicator: bool,
    hide_details: bool,
    layout_vertical: bool,
    custom_cmds: Vec<extra::CustomCmd>,
    repo_cfg: extra::RepoCfg,
    p: extra::Prefs,
    show_shortcuts: bool,
    show_about: bool,
    branch_dialog: Option<extra::BranchDialog>,
    repo_dialog: Option<extra::RepoSettingsDialog>,
    custom_pending: Option<(usize, String)>,
    last_fetch: f64,
    /// Branches pinned to the top of the sidebar (path|branch).
    pinned: Vec<String>,
    /// Sidebar branch order: 0 alphabetical, 1 backward, 2 recently used.
    branch_sort: u8,
    stash_rename: Option<(String, String)>,
    stale: Option<Vec<(String, bool, String)>>,
    upd: updates::UpdateUi,
    setup: Option<updates::SetupDialog>,
    /// Don't write settings any more (after uninstalling).
    no_save: bool,
}

/// How often the .git files are checked for changes.
const POLL_SECONDS: f64 = 1.0;

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
        let lang = match std::env::var("YAGUI_LANG").as_deref() {
            Ok("de") => Lang::De,
            Ok("en") => Lang::En,
            _ => saved.lang,
        };
        i18n::set(lang);
        let cmds = catalog::all();
        let mut app = App {
            cmds,
            tabs: vec![],
            active: 0,
            jobs: vec![],
            next_job: 1,
            dialog: None,
            rebase: None,
            quick: None,
            show_activity: false,
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
            last_poll: 0.0,
            show_console: false,
            notice: None,
            lang,
            commands_view: false,
            lib_search: String::new(),
            lib_form: None,
            ssh: None,
            creds: None,
            prefs: None,
            shell: saved.shell,
            editors: auth::detect_editors(),
            extra_keys: saved.extra_keys.iter().map(PathBuf::from).collect(),
            accounts: saved.accounts.clone(),
            flat_files: saved.flat_files,
            show_tablist: saved.show_tablist,
            manager_tab: false,
            manager_active: false,
            mgr: manager::ManagerState::default(),
            known_repos: saved.known_repos.iter().map(PathBuf::from).collect(),
            repo_names: saved.repo_names.clone(),
            src_dirs: if saved.src_dirs.is_empty() {
                std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).into_iter().collect()
            } else {
                saved.src_dirs.iter().map(PathBuf::from).collect()
            },
            hidden_repos: saved.hidden_repos.clone(),
            workspaces: if saved.workspaces.is_empty() { vec![manager::Workspace::new("Home")] } else { saved.workspaces.clone() },
            ws: saved.ws,
            ws_in_title: saved.ws_in_title,
            ws_dialog: None,
            treemap: None,
            bench: None,
            closed_tabs: vec![],
            prefs_tab_indicator: saved.prefs.tab_indicator,
            hide_details: saved.hide_details,
            layout_vertical: saved.prefs.layout_vertical,
            custom_cmds: saved.custom_cmds.clone(),
            repo_cfg: saved.repo_cfg.clone(),
            p: saved.prefs.clone(),
            show_shortcuts: false,
            show_about: false,
            branch_dialog: None,
            repo_dialog: None,
            custom_pending: None,
            last_fetch: 0.0,
            pinned: saved.pinned.clone(),
            branch_sort: saved.branch_sort,
            stash_rename: None,
            stale: None,
            upd: updates::UpdateUi::default(),
            setup: None,
            no_save: false,
        };
        crate::updater::cleanup();
        match updates::START_MODE.get() {
            Some(updates::StartMode::Install) => app.setup = Some(updates::SetupDialog::install()),
            Some(updates::StartMode::Uninstall) => app.setup = Some(updates::SetupDialog::uninstall()),
            _ => {}
        }
        theme::set_light(app.p.light);
        theme::apply_visuals(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(app.p.zoom.clamp(0.6, 2.0));
        extra::set_diff_view(4, app.p.diff_font);
        app.ws = app.ws.min(app.workspaces.len() - 1);
        let cwd_repo = std::env::current_dir().ok().and_then(|d| git::find_toplevel(&d));
        if let Some(r) = repo.and_then(|r| git::find_toplevel(&r)) {
            app.add_tab(r);
        } else if screenshot.is_some() && cwd_repo.is_some() {
            app.add_tab(cwd_repo.unwrap());
        } else {
            for t in &saved.tabs {
                let p = PathBuf::from(t);
                if p.exists() && git::find_toplevel(&p).is_some() {
                    let mut r = Repo::open(p.clone());
                    r.name = app.tab_name(&p);
                    if app.p.topo {
                        r.log_opts.topo = true;
                        r.reload(git::RELOAD_REFS);
                    }
                    app.tabs.push(r);
                }
            }
            app.active = saved.active.min(app.tabs.len().saturating_sub(1));
            if app.tabs.is_empty() {
                if let Some(r) = cwd_repo {
                    app.add_tab(r);
                }
            }
        }
        // Optional start view / dialog (used for checks and pictures).
        if let Ok(v) = std::env::var("YAGUI_VIEW") {
            let view = match v.as_str() {
                "changes" => Some(View::Changes),
                "commits" => Some(View::Commits),
                _ => None,
            };
            if let (Some(view), Some(t)) = (view, app.tabs.get_mut(app.active)) {
                t.view = view;
                t.reload_file_diff();
            }
        }
        match std::env::var("YAGUI_DIALOG").as_deref() {
            Ok("ssh") => app.ssh = Some(auth::SshState::new(&app.extra_keys)),
            Ok("accounts") => app.creds = Some(auth::CredState::new()),
            Ok("prefs") => app.prefs = Some(auth::PrefsState::new()),
            Ok("manager") => app.open_manager(),
            Ok("stash") => {
                let ctx = cc.egui_ctx.clone();
                app.apply(vec![Act::SelectStash("stash@{0}".into())], &ctx);
            }
            Ok("stats") => {
                let p = app.tabs.get(app.active).map(|t| t.path.clone());
                app.open_manager();
                app.mgr.sel = p;
                app.mgr.tab = 1;
            }
            Ok("treemap") => {
                if let Some(t) = app.tabs.get(app.active) {
                    app.treemap = Some(manager::Treemap::new(t.path.clone()));
                }
            }
            Ok("reposettings") => app.open_repo_settings(),
            Ok("branch") => app.open_branch_dialog(None, false),
            Ok("shortcuts") => app.show_shortcuts = true,
            Ok("about") => app.show_about = true,
            Ok("updates") => app.upd.open = true,
            Ok("setup") => app.setup = Some(updates::SetupDialog::install()),
            Ok("tablist") => app.show_tablist = true,
            Ok("light") => {
                theme::set_light(true);
                theme::apply_visuals(&cc.egui_ctx);
            }
            Ok("vertical") => app.layout_vertical = true,
            Ok("commands") => {
                app.commands_view = true;
                let i = catalog::find(&app.cmds, "merge");
                app.lib_form = Some(app.make_form(i, &[]));
            }
            _ => {}
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
        if self.screenshot.is_some() || self.no_save {
            return;
        }
        let s = Saved {
            tabs: self.tabs.iter().map(|t| t.path.display().to_string()).collect(),
            active: self.active,
            recent: self.recent.clone(),
            lang: self.lang,
            shell: self.shell,
            accounts: self.accounts.clone(),
            extra_keys: self.extra_keys.iter().map(|p| p.display().to_string()).collect(),
            flat_files: self.flat_files,
            show_tablist: self.show_tablist,
            known_repos: self.known_repos.iter().map(|p| p.display().to_string()).collect(),
            repo_names: self.repo_names.clone(),
            src_dirs: self.src_dirs.iter().map(|p| p.display().to_string()).collect(),
            hidden_repos: self.hidden_repos.clone(),
            workspaces: {
                let mut w = self.workspaces.clone();
                if let Some(cur) = w.get_mut(self.ws) {
                    cur.tabs = self.tabs.iter().map(|t| t.path.display().to_string()).collect();
                    cur.active = self.active;
                }
                w
            },
            ws: self.ws,
            ws_in_title: self.ws_in_title,
            hide_details: self.hide_details,
            custom_cmds: self.custom_cmds.clone(),
            repo_cfg: self.repo_cfg.clone(),
            prefs: {
                let mut p = self.p.clone();
                p.layout_vertical = self.layout_vertical;
                p.tab_indicator = self.prefs_tab_indicator;
                p.light = theme::is_light();
                p
            },
            pinned: self.pinned.clone(),
            branch_sort: self.branch_sort,
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
        self.hidden_repos.retain(|h| PathBuf::from(h) != path);
        self.manager_active = false;
        if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
            self.active = i;
        } else {
            let mut r = Repo::open(path.clone());
            r.name = self.tab_name(&path);
            if self.p.topo {
                r.log_opts.topo = true;
                r.reload(git::RELOAD_REFS);
            }
            self.tabs.push(r);
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
                    help: if i == 0 { format!("git {a}  ·  git help {a}") } else { tr("entered by you", "von dir eingegeben").into() },
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

    /// Runs a filled-in form (risky commands are confirmed inside the dialog itself).
    fn run_form(&mut self, mut f: Form, ctx: &egui::Context) -> Option<u64> {
        let c = self.cmds[f.cmd].clone();
        if c.kind == Kind::InteractiveRebase {
            let base = match f.values.first() {
                Some(Value::Text(t)) => t.trim().to_string(),
                _ => String::new(),
            };
            self.open_rebase(base);
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
            self.toast = Some((tr("Open a repository first.", "Öffne zuerst ein Repository.").into(), ctx.input(|i| i.time)));
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
            title: i18n::c(c.title).to_string(),
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

    fn spawn(&mut self, mut spec: JobSpec, ctx: &egui::Context) -> u64 {
        if self.p.verbose && !spec.automatic && spec.program == "git" {
            spec.env.push(("GIT_TRACE".into(), "1".into()));
        }
        let id = self.next_job;
        self.next_job += 1;
        self.jobs.push(git::start_job(id, spec, ctx.clone()));
        if self.jobs.len() > 300 {
            self.jobs.remove(0);
        }
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
        let display = format!("git {}   {}", args.join(" "), tr("(patch via standard input)", "(Patch über Standardeingabe)"));
        let cwd = Some(r.path.clone());
        self.spawn(JobSpec { title, program: "git".into(), args, display, cwd, stdin: Some(stdin), env: vec![], changes_repo: true, detached: false, automatic: false }, ctx);
    }

    fn job(&self, id: u64) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    fn poll_jobs(&mut self) {
        let mut failed: Option<u64> = None;
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
                        if !j.automatic {
                            failed = Some(j.id);
                        }
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
        if failed.is_some() {
            self.notice = failed;
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
            let act = RB_ACTIONS[it.action];
            if act == "reword" {
                s.push_str(&format!("pick {} {}\n", it.short, it.subject));
                if let Some(Some(f)) = msg_files.get(i) {
                    s.push_str(&format!("exec git commit --amend -F '{f}'\n"));
                } else {
                    s.push_str(&format!("exec git commit --amend -F <{}>\n", tr("new-message", "neue-beschreibung")));
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
            if RB_ACTIONS[it.action] == "reword" {
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
                title: tr("Interactive rebase", "Interaktiver Rebase").into(),
                program: "git".into(),
                args: vec!["rebase".into(), "-i".into(), d.base.clone()],
                display: format!("git rebase -i {}\n{}", d.base, todo.trim_end()),
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
// The frame

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        // The taskbar icon: set again over the first frames (the window may appear late).
        if matches!(self.frames, 2 | 30 | 120) {
            crate::winicon::apply(frame);
        }
        self.poll_jobs();
        self.handle_screenshot(&ctx);

        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && !self.was_focused {
            if let Some(t) = self.tabs.get_mut(self.active) {
                t.refresh();
            }
        }
        self.was_focused = focused;
        // Watch the .git folder and reload only what changed.
        let now = ctx.input(|i| i.time);
        if now - self.last_poll >= POLL_SECONDS {
            self.last_poll = now;
            if let Some(t) = self.tabs.get_mut(self.active) {
                t.poll_changes();
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(POLL_SECONDS));
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::P)) {
            self.quick = Some(String::new());
        }
        {
            let c = egui::Modifiers::CTRL;
            let cs = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
            let mut keys: Vec<Act> = vec![];
            let mut k = |m: egui::Modifiers, key: egui::Key, a: Act| {
                if ctx.input_mut(|i| i.consume_key(m, key)) {
                    keys.push(a);
                }
            };
            k(c, egui::Key::T, Act::Manager);
            k(c, egui::Key::O, Act::OpenRepoDialog);
            k(c, egui::Key::P, Act::QuickLaunch);
            k(cs, egui::Key::T, Act::RestoreTab);
            k(c, egui::Key::W, Act::CloseTab(self.active));
            k(c, egui::Key::Tab, Act::TabStep(1));
            k(cs, egui::Key::Tab, Act::TabStep(-1));
            k(c, egui::Key::Num1, Act::View(View::Changes));
            k(c, egui::Key::Num2, Act::View(View::Commits));
            k(c, egui::Key::H, Act::ViewOpt(ViewOpt::Head));
            k(cs, egui::Key::L, Act::Open("fetch", vec![]));
            k(cs, egui::Key::U, Act::Open("push", vec![]));
            k(cs, egui::Key::S, Act::Open("stash_push", vec![]));
            k(cs, egui::Key::B, Act::NewBranch(None));
            k(c, egui::Key::Comma, Act::Prefs);
            k(c, egui::Key::Plus, Act::Zoom(0.1));
            k(c, egui::Key::Equals, Act::Zoom(0.1));
            k(c, egui::Key::Minus, Act::Zoom(-0.1));
            k(c, egui::Key::Num0, Act::Zoom(0.0));
            if !keys.is_empty() && (!self.tabs.is_empty() || keys.iter().all(|a| matches!(a, Act::Manager | Act::OpenRepoDialog | Act::QuickLaunch | Act::RestoreTab | Act::Prefs | Act::Zoom(_)))) {
                self.apply(keys, &ctx);
            }
        }
        // Fetch remotes automatically (Preferences > General), in the background.
        if self.p.auto_fetch && self.screenshot.is_none() {
            let now = ctx.input(|i| i.time);
            if self.last_fetch == 0.0 {
                self.last_fetch = now;
            } else if now - self.last_fetch > self.p.fetch_minutes.max(1) as f64 * 60.0 {
                self.last_fetch = now;
                if let Some(r) = self.tabs.get(self.active) {
                    if !r.remotes.is_empty() {
                        let mut args = vec!["fetch".to_string(), "--all".into(), "--prune".into()];
                        if self.p.fetch_tags {
                            args.push("--tags".into());
                        }
                        let display = format!("git {}", args.join(" "));
                        let cwd = Some(r.path.clone());
                        self.spawn(JobSpec { title: tr("Fetch (automatic)", "Fetch (automatisch)").into(), program: "git".into(), args, display, cwd, stdin: None, env: vec![("GIT_TERMINAL_PROMPT".into(), "0".into())], changes_repo: true, detached: false, automatic: true }, &ctx);
                    }
                }
            }
        }
        if let Some(p) = self.repo().map(|r| r.path.clone()) {
            let ca = egui::Modifiers::CTRL | egui::Modifiers::ALT;
            if ctx.input_mut(|i| i.consume_key(ca, egui::Key::T)) {
                auth::open_terminal(self.shell, &p);
            }
            if ctx.input_mut(|i| i.consume_key(ca, egui::Key::O)) {
                auth::open_explorer(&p);
            }
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
        self.status_bar(ui, &mut acts);
        self.poll_manager(&ctx);
        if self.show_tablist {
            self.tab_list(ui, &mut acts);
        }
        if self.manager_active {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL())).show(ui, |ui| {
                self.manager_view(ui, &mut acts, &ctx);
            });
        } else if self.commands_view {
            if !self.tabs.is_empty() {
                self.sidebar(ui, &mut acts);
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL())).show(ui, |ui| {
                self.commands_view(ui, &mut acts, &ctx);
            });
        } else if self.tabs.is_empty() {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL())).show(ui, |ui| {
                self.welcome(ui, &mut acts);
            });
        } else {
            self.sidebar(ui, &mut acts);
            let view = self.tabs[self.active].view;
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::PANEL())).show(ui, |ui| {
                self.notice_bar(ui, &mut acts);
                self.state_banner(ui, &mut acts);
                match view {
                    View::Commits => self.commits_view(ui, &mut acts),
                    View::Changes => self.changes_view(ui, &mut acts, &ctx),
                }
            });
        }
        self.dialogs(&ctx);
        self.activity_window(&ctx);
        self.console_window(&ctx);
        self.auth_dialogs(&ctx);
        self.manager_dialogs(&ctx);
        self.extra_dialogs(&ctx);
        self.update_window(&ctx);
        self.setup_window(&ctx);
        self.auto_update_check(&ctx);
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
                        eprintln!("could not save screenshot: {e}");
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
                        t.stash_sel = t.commits.get(i).and_then(|c| c.refs.iter().find(|r| r.starts_with("stash@")).cloned());
                        t.select_commit(i);
                    }
                }
                Act::View(v) => {
                    self.commands_view = false;
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
                    if let Some(p) = rfd::FileDialog::new().set_title(tr("Open Git repository", "Git-Repository öffnen")).pick_folder() {
                        match git::find_toplevel(&p) {
                            Some(top) => self.add_tab(top),
                            None => {
                                self.toast = Some((tr("There is no Git repository in this folder. Use New to create one.", "In diesem Ordner gibt es kein Git-Repository. Lege mit „Neu“ eines an.").into(), ctx.input(|i| i.time)));
                            }
                        }
                    }
                }
                Act::OpenRepo(p) => match git::find_toplevel(&p) {
                    Some(top) => self.add_tab(top),
                    None => self.toast = Some((tr("No Git repository found.", "Kein Git-Repository gefunden.").into(), ctx.input(|i| i.time))),
                },
                Act::CloseTab(i) => {
                    if i < self.tabs.len() {
                        let t = self.tabs.remove(i);
                        self.closed_tabs.push(t.path);
                        if self.active >= self.tabs.len() {
                            self.active = self.tabs.len().saturating_sub(1);
                        }
                        self.save();
                    }
                }
                Act::SwitchTab(i) => {
                    self.manager_active = false;
                    self.commands_view = false;
                    self.active = i;
                    self.apply_repo_view();
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
                            t.tree_content = if o.stdout.contains('\0') { tr("(binary file)", "(Binärdatei)").into() } else { o.stdout };
                            t.tree_file = Some(f);
                        }
                    }
                }
                Act::Terminal => {
                    if let Some(r) = self.repo() {
                        auth::open_terminal(self.shell, &r.path);
                    }
                }
                Act::Explorer(p) => auth::open_explorer(&p),
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
                Act::Lang(l) => {
                    self.lang = l;
                    i18n::set(l);
                    self.save();
                }
                Act::ToggleActivity => self.show_activity = !self.show_activity,
                Act::ToggleConsole => self.show_console = !self.show_console,
                Act::Commands(on) => {
                    self.commands_view = on;
                    if on && self.lib_form.is_none() {
                        let i = catalog::find(&self.cmds, "status");
                        self.lib_form = Some(self.make_form(i, &[]));
                    }
                }
                Act::Ssh => self.ssh = Some(auth::SshState::new(&self.extra_keys)),
                Act::Accounts => self.creds = Some(auth::CredState::new()),
                Act::Prefs => self.prefs = Some(auth::PrefsState::new()),
                Act::Manager => self.open_manager(),
                Act::ToggleTabList => {
                    self.show_tablist = !self.show_tablist;
                    self.save();
                }
                Act::ViewOpt(o) => {
                    if o == ViewOpt::Details {
                        self.hide_details = !self.hide_details;
                        self.save();
                    } else if let Some(t) = self.tabs.get_mut(self.active) {
                        match o {
                            ViewOpt::Head => {
                                t.view = View::Commits;
                                if let Some(i) = t.commits.iter().position(|c| c.refs.iter().any(|r| r == "HEAD" || r.starts_with("HEAD -> "))) {
                                    t.select_commit(i);
                                    t.scroll_to = Some(i);
                                }
                            }
                            ViewOpt::Tags => t.log_opts.hide_tags = !t.log_opts.hide_tags,
                            ViewOpt::Stashes => {
                                t.log_opts.hide_stashes = !t.log_opts.hide_stashes;
                                t.reload(git::RELOAD_REFS);
                            }
                            ViewOpt::Reflog => {
                                t.log_opts.reflog = !t.log_opts.reflog;
                                t.reload(git::RELOAD_REFS);
                            }
                            ViewOpt::FirstParent => {
                                t.log_opts.first_parent = !t.log_opts.first_parent;
                                t.reload(git::RELOAD_REFS);
                            }
                            ViewOpt::ActiveBranch => {
                                t.log_opts.only = if t.log_opts.only.as_deref() == Some("HEAD") { None } else { Some("HEAD".into()) };
                                t.reload(git::RELOAD_REFS);
                            }
                            ViewOpt::Details => {}
                        }
                    }
                }
                Act::RenameRepo(p) => {
                    let n = self.tab_name(&p);
                    self.mgr.rename = Some((p, n));
                }
                Act::StaleBranches => self.open_stale(),
                Act::CheckUpdates => {
                    self.upd.open = true;
                    self.check_updates(false, ctx);
                }
                Act::Install => self.setup = Some(updates::SetupDialog::install()),
                Act::Uninstall => self.setup = Some(updates::SetupDialog::uninstall()),
                Act::SelectStash(name) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.view = View::Commits;
                        t.stash_sel = Some(name.clone());
                        let sha = t.stash_shas.iter().find(|s| s.0 == name).map(|s| s.1.clone()).unwrap_or_else(|| git::run(&t.path, &["rev-parse", &name]).stdout.trim().to_string());
                        if let Some(i) = t.commits.iter().position(|c| c.sha == sha) {
                            t.select_commit(i);
                            t.scroll_to = Some(i);
                        } else if !sha.is_empty() {
                            // Hidden from the list (View > Hide Stashes): show its details anyway.
                            t.selected = None;
                            t.detail = Some(git::commit_detail(&t.path, &sha));
                            t.detail_file = None;
                            t.detail_diff.clear();
                            t.tree.clear();
                        }
                        if t.detail_tab == DetailTab::Tree {
                            t.detail_tab = DetailTab::Commit;
                        }
                    }
                }
                Act::RebaseAction(sha, action) => {
                    // Opens the rebase editor with this commit already set (squash/fixup also
                    // list the parent it goes into); nothing runs until it is confirmed there.
                    let base = if action == 3 || action == 4 { format!("{sha}~2") } else { format!("{sha}~1") };
                    let base = self.tabs.get(self.active).map(|t| git::run(&t.path, &["rev-parse", "--verify", "-q", &base]).stdout.trim().to_string()).filter(|b| !b.is_empty()).unwrap_or_else(|| "--root".into());
                    self.open_rebase(base);
                    if let Some(d) = self.rebase.as_mut() {
                        if let Some(it) = d.items.iter_mut().find(|it| sha.starts_with(&it.short)) {
                            it.action = action;
                        }
                    }
                }
                Act::RenameStash(name) => self.stash_rename = Some((name, String::new())),
                Act::StashPatch(name) => {
                    if let Some(t) = self.tabs.get(self.active) {
                        let patch = git::run(&t.path, &["stash", "show", "-p", "--include-untracked", &name]).stdout;
                        if let Some(f) = rfd::FileDialog::new().set_file_name(format!("{}.patch", name.replace(['{', '}', '@'], ""))).save_file() {
                            let ok = std::fs::write(&f, patch).is_ok();
                            self.toast = Some((if ok { trs(format!("Saved {}", f.display()), format!("Gespeichert: {}", f.display())) } else { tr("Could not save the patch.", "Patch konnte nicht gespeichert werden.").into() }, ctx.input(|i| i.time)));
                        }
                    }
                }
                Act::Snapshot => {
                    if let Some(t) = self.tabs.get(self.active) {
                        let msg = format!("Snapshot {}", git::run(&t.path, &["log", "-1", "--format=%h"]).stdout.trim());
                        let sha = git::run(&t.path, &["stash", "create", &msg]).stdout.trim().to_string();
                        if sha.is_empty() {
                            self.toast = Some((tr("Nothing to save: there are no local changes.", "Nichts zu speichern: Es gibt keine lokalen Änderungen.").into(), ctx.input(|i| i.time)));
                        } else {
                            self.quick(tr("Save snapshot", "Schnappschuss speichern").into(), vec!["stash".into(), "store".into(), "-m".into(), msg, sha], ctx);
                        }
                    }
                }
                Act::NewBranch(at) => self.open_branch_dialog(at, false),
                Act::NewTag(at) => self.open_branch_dialog(at, true),
                Act::Stats => {
                    let p = self.tabs.get(self.active).map(|t| t.path.clone());
                    self.open_manager();
                    if let Some(p) = p {
                        self.mgr.sel = Some(p);
                    }
                    self.mgr.tab = 1;
                }
                Act::Treemap => {
                    if let Some(t) = self.tabs.get(self.active) {
                        self.treemap = Some(manager::Treemap::new(t.path.clone()));
                    }
                }
                Act::Bench => {
                    if let Some(t) = self.tabs.get(self.active) {
                        self.bench = Some(manager::Bench { path: t.path.clone(), results: vec![], rx: None });
                    }
                }
                Act::RepoSettings => self.open_repo_settings(),
                Act::Custom(i, r) => self.run_custom(i, r, ctx, false),
                Act::TabStep(d) => {
                    let n = self.tabs.len() as i32;
                    if n > 0 {
                        self.manager_active = false;
                        self.active = ((self.active as i32 + d).rem_euclid(n)) as usize;
                        self.apply_repo_view();
                    }
                }
                Act::Theme(light) => {
                    theme::set_light(light);
                    theme::apply_visuals(ctx);
                    self.save();
                }
                Act::Zoom(dz) => {
                    self.p.zoom = if dz == 0.0 { 1.0 } else { (self.p.zoom + dz).clamp(0.6, 2.0) };
                    ctx.set_zoom_factor(self.p.zoom);
                    self.save();
                }
                Act::Layout(v) => {
                    self.layout_vertical = v;
                    self.save();
                }
                Act::Shortcuts => self.show_shortcuts = true,
                Act::About => self.show_about = true,
                Act::FilterBranch(b) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.log_opts.only = if t.log_opts.only.as_deref() == Some(b.as_str()) { None } else { Some(b) };
                        t.view = View::Commits;
                        t.reload(git::RELOAD_REFS);
                    }
                }
                Act::HideBranch(b) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        if let Some(i) = t.log_opts.hidden.iter().position(|h| *h == b) {
                            t.log_opts.hidden.remove(i);
                        } else {
                            t.log_opts.hidden.push(b);
                        }
                        t.reload(git::RELOAD_REFS);
                    }
                }
                Act::PinBranch(b) => {
                    if let Some(t) = self.tabs.get(self.active) {
                        let key = format!("{}|{b}", t.path.display());
                        if let Some(i) = self.pinned.iter().position(|p| *p == key) {
                            self.pinned.remove(i);
                        } else {
                            self.pinned.push(key);
                        }
                        self.save();
                    }
                }
                Act::CloseManager => {
                    self.manager_tab = false;
                    self.manager_active = false;
                }
                Act::CloseOthers(keep) => {
                    let kept = keep.and_then(|k| self.tabs.get(k).map(|t| t.path.clone()));
                    let mut i = 0;
                    while i < self.tabs.len() {
                        if Some(&self.tabs[i].path) == kept.as_ref() {
                            i += 1;
                        } else {
                            let t = self.tabs.remove(i);
                            self.closed_tabs.push(t.path);
                        }
                    }
                    self.active = 0;
                    if self.tabs.is_empty() {
                        self.open_manager();
                    }
                    self.save();
                }
                Act::MoveToWorkspace(i, wi) => {
                    if i < self.tabs.len() && wi < self.workspaces.len() {
                        let t = self.tabs.remove(i);
                        let ps = t.path.display().to_string();
                        if !self.workspaces[wi].tabs.contains(&ps) {
                            self.workspaces[wi].tabs.push(ps);
                        }
                        self.active = self.active.min(self.tabs.len().saturating_sub(1));
                        self.save();
                    }
                }
                Act::OpenInWorkspace(p, wi) => {
                    if wi == self.ws {
                        self.add_tab(p);
                    } else if wi < self.workspaces.len() {
                        let ps = p.display().to_string();
                        if !self.workspaces[wi].tabs.contains(&ps) {
                            self.workspaces[wi].tabs.push(ps);
                        }
                        self.save();
                    }
                }
                Act::SwitchWorkspace(wi) => self.switch_workspace(wi, ctx),
                Act::RestoreTab => {
                    if let Some(p) = self.closed_tabs.pop() {
                        if p.exists() {
                            self.add_tab(p);
                        }
                    }
                }
            }
        }
    }
}
