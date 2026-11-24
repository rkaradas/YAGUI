//! Application state and logic: repositories, running git, jobs, interactive rebase.
//! The window itself is drawn in `app/ui.rs` and `app/dialogs.rs`.

mod auth;
mod dialogs;
mod diffmodel;
mod diffview;
mod extra;
mod manager;
pub mod perf;
mod tree;
mod ui;
pub mod updates;

/// The window's egui context, so background loaders can ask for a repaint when they are done.
pub(crate) static REPAINT: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui;

use diffmodel::Diff;

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
    detail_diff: Arc<Diff>,
    /// Which (commit, file) `detail_diff` shows.
    detail_diff_key: Option<(String, String)>,
    tree: Vec<String>,
    tree_file: Option<String>,
    tree_content: String,
    /// The file `tree_content` belongs to (a newly picked one may still be loading).
    tree_content_for: Option<String>,
    filter: String,
    commit_search: String,
    sel_file: Option<(String, bool)>,
    file_diff: Arc<Diff>,
    /// Which (file, staged) `file_diff` shows, and the options it was made with.
    file_diff_key: Option<(String, bool)>,
    file_diff_opts: git::DiffOpts,
    commit_msg: String,
    commit_desc: String,
    amend: bool,
    scroll_to: Option<usize>,
    error: Option<String>,
    log_opts: git::LogOptions,
    git_dir: Option<PathBuf>,
    /// The repository's common folder (differs from `git_dir` in a linked worktree).
    common_dir: Option<PathBuf>,
    snap: git::Snapshot,
    locked: Option<&'static str>,
    /// Commit search: 0 message/author/SHA, 1 path, 2 diff content.
    search_mode: usize,
    search_hits: Option<std::collections::HashSet<String>>,
    /// When the path/content search is to ask git (window time; set while typing).
    search_due: Option<f64>,
    /// A running path/content search, tagged with (mode, text) it was made for.
    bg_search: Bg<((usize, String), std::collections::HashSet<String>)>,
    /// Local branches, most recently committed first (sidebar sort "Recently used").
    branch_recency: Vec<String>,
    /// (stash@{n}, commit) of every stash.
    stash_shas: Vec<(String, String)>,
    /// The stash picked in the sidebar (highlighted there).
    stash_sel: Option<String>,
    /// Diff options of the diff toolbar (context, whitespace, whole file).
    diff_opts: git::DiffOpts,
    /// The options changed while the tab was in the background: reload its diffs when shown.
    diff_stale: bool,
    /// Loading has been asked for at least once (a tab loads when it is first shown).
    started: bool,
    /// The history has been shown at least once (then the first commit gets selected).
    shown_once: bool,
    // Background work. Git never runs on the window's thread for these; the results are taken
    // over by `poll_bg` at the start of a frame, until then the old state stays on screen.
    bg_load: Bg<Loaded>,
    load_started: Option<std::time::Instant>,
    /// Reload flags asked for while a load runs; they start when it ends.
    queued: u32,
    /// The commit whose details are wanted (late results for others are dropped).
    wanted_detail: Option<String>,
    bg_detail: Bg<(String, CommitDetail, Option<Vec<String>>)>,
    /// The Changes tab opens the first file of the commit whose details are loading.
    open_first_file: bool,
    /// (file, staged, options, request number, diff).
    bg_file: Bg<(String, bool, git::DiffOpts, u64, Arc<Diff>)>,
    /// Requests for `file_diff` so far, and the one on screen.
    file_req: u64,
    file_diff_req: u64,
    /// Running commands that change this repository (hunk actions, staging files, commits …);
    /// until they ended and a diff requested after that (number `hunk_ok_from` or later) is on
    /// screen, hunk buttons wait: the diff before no longer matches the files, and applying a
    /// hunk from it would apply twice or in the wrong place.
    jobs_pending: Vec<u64>,
    hunk_ok_from: u64,
    /// The repository's tab width (Repository Settings), for its diffs' column counts.
    tab_width: usize,
    /// Checking whether Discard Hunk would change the shown file elsewhere: (diff request, yes).
    bg_eol: Bg<(u64, bool)>,
    discard_checked: Option<(u64, bool)>,
    /// A watched lock file has existed since (see `poll_changes`).
    busy_since: Option<std::time::Instant>,
    /// A quick status of a tab not shown yet (for the changes dot in the tab list).
    bg_peek: Bg<Vec<FileStatus>>,
    peeked: bool,
    /// Finding the commit of a branch, tag or id that the loaded commits don't name.
    bg_jump: Bg<String>,
    bg_ddiff: Bg<(String, String, git::DiffOpts, Arc<Diff>)>,
    bg_tree: Bg<(String, Vec<String>)>,
    bg_tree_file: Bg<(String, String, String)>,
    /// Diffs of files opened in the Commit tab, by (commit, path), made with `inline_opts`.
    inline: std::collections::HashMap<(String, String), Arc<Diff>>,
    inline_opts: git::DiffOpts,
    /// The tab width `inline` was made with.
    inline_tab: usize,
    bg_inline: Vec<(String, String, Bg<Arc<Diff>>)>,
}

/// A full reload: also checks that the folder is (still) a repository.
const FULL: u32 = u32::MAX;

/// A command makes the hunk buttons wait (see `Repo::jobs_pending`) when it can change the
/// files or the index and YAGUI learns when it ends: not a program started on its own
/// (merge tool, `citool`), not an automatic fetch, and not git commands that only touch refs,
/// remotes or the network.
fn blocks_hunks(spec: &JobSpec) -> bool {
    if !spec.changes_repo || spec.automatic || spec.detached {
        return false;
    }
    if spec.program != "git" {
        return true;
    }
    // The git command and its subcommand: the first arguments that aren't options (or the
    // value of a `-c`).
    let mut args = spec.args.iter();
    let mut words: Vec<&str> = vec![];
    while let Some(a) = args.next() {
        if a == "-c" || a == "-C" {
            args.next();
        } else if !a.starts_with('-') {
            words.push(a);
            if words.len() == 2 {
                break;
            }
        }
    }
    let cmd = words.first().copied().unwrap_or("");
    let sub = words.get(1).copied().unwrap_or("");
    match cmd {
        "push" | "fetch" | "clone" | "gc" | "maintenance" | "remote" | "tag" | "notes" | "config" | "prune" | "repack" | "fsck" | "count-objects" | "bundle" | "archive" | "ls-remote" | "branch" | "worktree" | "reflog" | "describe" | "log" | "show" | "shortlog" | "blame" => false,
        // Tools with subcommands: only those that update the work tree count.
        "lfs" => matches!(sub, "pull" | "checkout" | "migrate" | "install" | "uninstall" | "update"),
        "svn" => matches!(sub, "rebase" | "dcommit" | "reset" | ""),
        "p4" => matches!(sub, "rebase" | "submit" | "unshelve" | ""),
        _ => true,
    }
}

/// Work done in a background thread. Its result is taken over at the start of a later frame.
/// Starting again, `stop` or dropping it stops the previous work: its result is dropped and
/// the git process it waits for is ended (`git::set_thread_cancel`), so replaced loads don't
/// pile up on a slow computer.
struct Bg<T> {
    rx: Option<std::sync::mpsc::Receiver<T>>,
    cancel: Option<Arc<AtomicBool>>,
}

impl<T> Default for Bg<T> {
    fn default() -> Self {
        Bg { rx: None, cancel: None }
    }
}

impl<T> Drop for Bg<T> {
    fn drop(&mut self) {
        self.stop();
    }
}

impl<T> Bg<T> {
    fn stop(&mut self) {
        if let Some(c) = self.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        self.rx = None;
    }

    fn busy(&self) -> bool {
        self.rx.is_some()
    }
}

impl<T: Send + 'static> Bg<T> {
    fn start(&mut self, f: impl FnOnce() -> T + Send + 'static) {
        self.stop();
        let (tx, rx) = std::sync::mpsc::channel();
        let flag = Arc::new(AtomicBool::new(false));
        let mine = flag.clone();
        std::thread::spawn(move || {
            git::set_thread_cancel(Some(mine));
            let v = f();
            if tx.send(v).is_ok() {
                if let Some(c) = REPAINT.get() {
                    c.request_repaint();
                }
            }
        });
        self.rx = Some(rx);
        self.cancel = Some(flag);
    }

    fn take(&mut self) -> Option<T> {
        let got = self.rx.as_ref()?.try_recv();
        match got {
            Ok(v) => {
                self.rx = None;
                self.cancel = None;
                Some(v)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                self.cancel = None;
                None
            }
        }
    }
}

/// What a background reload found; `Repo::apply` takes it over in one go.
#[derive(Default)]
struct Loaded {
    flags: u32,
    not_repo: bool,
    /// (.git folder, common folder), found by a full load.
    dirs: Option<(PathBuf, PathBuf)>,
    history: Option<History>,
    config: Option<(Vec<(String, String)>, Vec<String>)>,
    status: Option<Vec<FileStatus>>,
    /// The reload flags whose watched files this load read (their snapshot entries count).
    covered: u32,
    state: Option<(Option<&'static str>, Option<&'static str>)>,
    snap: Option<git::Snapshot>,
}

struct History {
    head: String,
    commits: Vec<Commit>,
    rows: Vec<graph::Row>,
    lanes: usize,
    local: Vec<RefInfo>,
    remote_branches: Vec<RefInfo>,
    tags: Vec<RefInfo>,
    stashes: Vec<(String, String)>,
    stash_shas: Vec<(String, String)>,
    stash_tips: Vec<String>,
    branch_recency: Vec<String>,
    worktrees: Vec<(String, String)>,
}

/// Runs the git commands a reload with `flags` needs (the reload flags of the .git folder
/// watch). Runs in a background thread.
fn load(path: PathBuf, dirs: Option<(PathBuf, PathBuf)>, flags: u32, mut log_opts: git::LogOptions) -> Loaded {
    let t0 = std::time::Instant::now();
    let p = &path;
    let mut l = Loaded { flags, ..Default::default() };
    // (The .git folder and the repository's common folder, see `git::common_dir`.)
    let mut dirs = dirs;
    if flags == FULL {
        if git::find_toplevel(p).is_none() {
            l.not_repo = true;
            return l;
        }
        dirs = git::git_dir(p).map(|g| {
            let common = git::common_dir(p).unwrap_or_else(|| g.clone());
            (g, common)
        });
        l.dirs = dirs.clone();
    }
    let git_dir = dirs.as_ref().map(|d| d.0.clone());
    // Taken first: a change during the load is then seen by the next look at .git.
    l.snap = dirs.as_ref().map(|(g, c)| git::snapshot(g, c));
    if flags != FULL && (!p.exists() || git_dir.as_ref().is_some_and(|g| !g.exists())) {
        // The folder was moved or deleted while open: say so instead of showing it empty.
        l.not_repo = true;
        return l;
    }
    if flags & (git::RELOAD_HEAD | git::RELOAD_REFS | git::RELOAD_REFTABLE) != 0 {
        let head = git::head_name(p);
        let stashes = git::stash_commits(p);
        log_opts.stash_tips = stashes.iter().map(|s| s.1.clone()).collect();
        let mut commits = git::load_commits(p, 3000, &log_opts);
        // Show every stash as one row labelled stash@{n} and leave out
        // Git's helper commits ("index on …", "untracked files on …").
        let helpers: std::collections::HashSet<&String> = stashes.iter().flat_map(|s| s.2.iter().skip(1)).collect();
        commits.retain(|c| !helpers.contains(&c.sha));
        for c in commits.iter_mut() {
            if let Some(s) = stashes.iter().find(|s| s.1 == c.sha) {
                c.parents.truncate(1);
                c.refs.retain(|r| r != "refs/stash");
                c.refs.insert(0, s.0.clone());
            }
        }
        let (rows, lanes) = graph::layout(&commits);
        let (local, remote_branches, tags) = git::load_refs(p);
        l.history = Some(History {
            head,
            rows,
            lanes: lanes.min(14),
            local,
            remote_branches,
            tags,
            stashes: git::load_stashes(p),
            stash_shas: stashes.iter().map(|s| (s.0.clone(), s.1.clone())).collect(),
            stash_tips: log_opts.stash_tips.clone(),
            branch_recency: git::run(p, &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads"]).stdout.lines().map(String::from).collect(),
            worktrees: git::load_worktrees(p),
            commits,
        });
    }
    if flags & git::RELOAD_CONFIG != 0 {
        l.config = Some((git::load_remotes(p), git::load_submodules(p)));
    }
    if flags & (git::RELOAD_INDEX | git::RELOAD_IGNORE | git::RELOAD_HEAD | git::RELOAD_STATE) != 0 {
        l.status = Some(git::load_status(p));
    }
    if flags & (git::RELOAD_STATE | git::RELOAD_HEAD | git::RELOAD_INDEX) != 0 {
        l.state = Some(match git_dir.as_deref() {
            Some(g) => (git::repo_state_at(g), git::repo_locked_at(g)),
            None => (None, None),
        });
    }
    // Which watched files this load has read the current state of (see `Repo::apply`).
    l.covered = if flags == FULL {
        FULL
    } else {
        let mut c = 0;
        if l.history.is_some() {
            c |= git::RELOAD_HEAD | git::RELOAD_REFS | git::RELOAD_REFTABLE;
        }
        if l.config.is_some() {
            c |= git::RELOAD_CONFIG;
        }
        if l.status.is_some() {
            c |= git::RELOAD_INDEX | git::RELOAD_IGNORE;
        }
        if l.state.is_some() {
            c |= git::RELOAD_STATE;
        }
        c
    };
    perf::took(&format!("load {flags:#x} {}", path.display()), t0);
    l
}

impl Repo {
    /// A tab for `path`; nothing is loaded until `refresh` (tabs load when first shown).
    fn new(path: PathBuf) -> Self {
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string());
        Repo {
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
            detail_diff: Arc::new(Diff::default()),
            detail_diff_key: None,
            tree: vec![],
            tree_file: None,
            tree_content: String::new(),
            tree_content_for: None,
            filter: String::new(),
            commit_search: String::new(),
            sel_file: None,
            file_diff: Arc::new(Diff::default()),
            file_diff_key: None,
            file_diff_opts: git::DiffOpts::default(),
            commit_msg: String::new(),
            commit_desc: String::new(),
            amend: false,
            scroll_to: None,
            error: None,
            log_opts: git::LogOptions { all: true, ..Default::default() },
            git_dir: None,
            common_dir: None,
            snap: vec![],
            locked: None,
            search_mode: 0,
            search_hits: None,
            search_due: None,
            bg_search: Bg::default(),
            branch_recency: vec![],
            stash_shas: vec![],
            stash_sel: None,
            diff_opts: git::DiffOpts::default(),
            diff_stale: false,
            started: false,
            shown_once: false,
            bg_load: Bg::default(),
            load_started: None,
            queued: 0,
            wanted_detail: None,
            bg_detail: Bg::default(),
            open_first_file: false,
            bg_file: Bg::default(),
            file_req: 0,
            file_diff_req: 0,
            jobs_pending: vec![],
            hunk_ok_from: 0,
            tab_width: 4,
            bg_eol: Bg::default(),
            discard_checked: None,
            busy_since: None,
            bg_peek: Bg::default(),
            peeked: false,
            bg_jump: Bg::default(),
            bg_ddiff: Bg::default(),
            bg_tree: Bg::default(),
            bg_tree_file: Bg::default(),
            inline: Default::default(),
            inline_opts: git::DiffOpts::default(),
            inline_tab: 0,
            bg_inline: vec![],
        }
    }

    /// Loads everything again (in the background). A tab that was never shown waits until it
    /// is (`show`): many tabs loading at once would keep a slow computer busy for long.
    fn refresh(&mut self) {
        if self.started {
            self.request(FULL);
        }
    }

    /// Reloads only what a change touched (in the background); see `refresh`.
    fn reload(&mut self, flags: u32) {
        if self.started {
            self.request(flags);
        }
    }

    fn request(&mut self, flags: u32) {
        self.started = true;
        if self.bg_load.busy() {
            self.queued |= flags;
            return;
        }
        // Until a full load has found the .git folder (or when the folder is no repository),
        // any reload is a full one: only that sets up the watch, the state and the history.
        let flags = if self.git_dir.is_none() { FULL } else { flags };
        let (path, git_dir, log_opts) = (self.path.clone(), self.git_dirs(), self.log_opts.clone());
        self.load_started = Some(std::time::Instant::now());
        self.bg_load.start(move || load(path, git_dir, flags, log_opts));
    }

    /// The tab is shown: load it the first time, later catch up with what changed meanwhile
    /// (its .git folder is only watched while it is the active tab).
    fn show(&mut self) {
        if !self.started {
            self.request(FULL);
            return;
        }
        if self.diff_stale {
            self.reload_diffs();
        }
        let changed = self.git_dirs().map(|(g, c)| git::changed_flags(&self.snap, &git::snapshot(&g, &c))).unwrap_or(0);
        self.reload(changed | git::RELOAD_INDEX);
    }

    /// (.git folder, common folder) once a full load found them.
    fn git_dirs(&self) -> Option<(PathBuf, PathBuf)> {
        let g = self.git_dir.clone()?;
        let c = self.common_dir.clone().unwrap_or_else(|| g.clone());
        Some((g, c))
    }

    /// How long the running background load has been at work (for a hint in the status bar).
    fn loading_for(&self) -> Option<std::time::Duration> {
        self.bg_load.busy().then(|| self.load_started.map(|t| t.elapsed())).flatten()
    }

    /// Takes over what background work has finished.
    fn poll_bg(&mut self) {
        if let Some(l) = self.bg_load.take() {
            self.apply(l);
            if self.queued != 0 {
                let q = std::mem::take(&mut self.queued);
                self.request(q);
            }
        }
        if let Some((sha, d, tree)) = self.bg_detail.take() {
            if self.wanted_detail.as_deref() == Some(sha.as_str()) {
                let first_file = d.files.first().map(|f| f.1.clone());
                self.detail = Some(d);
                if let Some(t) = tree {
                    self.tree = t;
                } else if self.detail_tab == DetailTab::Tree && self.tree.is_empty() {
                    // The File Tree tab was opened while the details were loading.
                    self.load_tree();
                }
                // The Changes tab shows the first file right away (as when switching to it);
                // a refresh of the same commit leaves what is open.
                if std::mem::take(&mut self.open_first_file) && self.detail_tab == DetailTab::Changes && self.detail_file.is_none() {
                    if let Some(f) = first_file {
                        self.load_detail_diff(f);
                    }
                }
            }
        }
        if let Some((path, staged, opts, req, d)) = self.bg_file.take() {
            if self.sel_file.as_ref() == Some(&(path.clone(), staged)) && opts == self.diff_opts {
                // Discard Hunk waits until it is known whether git apply would change the file
                // outside the hunk (line ends, encoding, filters); only for a tracked file's
                // unstaged hunks.
                let tracked = self.status.iter().any(|f| f.path == path && f.index != '?');
                if !staged && tracked && d.hunks_applicable() && !d.hunks.is_empty() {
                    let (dir, p) = (self.path.clone(), path.clone());
                    self.bg_eol.start(move || (req, git::discard_changes_file(&dir, &p)));
                } else {
                    self.bg_eol.stop();
                }
                self.file_diff = d;
                self.file_diff_key = Some((path, staged));
                self.file_diff_opts = opts;
                self.file_diff_req = req;
            }
        }
        if let Some((req, risky)) = self.bg_eol.take() {
            self.discard_checked = Some((req, risky));
        }
        if let Some(s) = self.bg_peek.take() {
            if !self.started {
                self.status = s;
            }
        }
        if let Some(sha) = self.bg_jump.take() {
            self.jump_to(&sha);
        }
        if let Some((sha, path, opts, d)) = self.bg_ddiff.take() {
            if self.detail.as_ref().map(|x| x.sha.as_str()) == Some(sha.as_str()) && self.detail_file.as_deref() == Some(path.as_str()) && opts == self.diff_opts {
                self.detail_diff = d;
                self.detail_diff_key = Some((sha, path));
            }
        }
        if let Some((sha, t)) = self.bg_tree.take() {
            if self.detail.as_ref().map(|x| x.sha.as_str()) == Some(sha.as_str()) {
                self.tree = t;
            }
        }
        if let Some((sha, path, content)) = self.bg_tree_file.take() {
            if self.detail.as_ref().map(|x| x.sha.as_str()) == Some(sha.as_str()) && self.tree_file.as_deref() == Some(path.as_str()) {
                self.tree_content = content;
                self.tree_content_for = Some(path);
            }
        }
        if let Some((key, hits)) = self.bg_search.take() {
            if key == (self.search_mode, self.commit_search.trim().to_string()) {
                self.search_hits = Some(hits);
            }
        }
        let mut done = vec![];
        for (sha, path, bg) in self.bg_inline.iter_mut() {
            if let Some(d) = bg.take() {
                done.push((sha.clone(), path.clone(), d));
            }
        }
        self.bg_inline.retain(|(_, _, bg)| bg.busy());
        for (sha, path, d) in done {
            self.inline.insert((sha, path), d);
        }
    }

    fn apply(&mut self, l: Loaded) {
        if l.not_repo {
            self.error = Some(tr("This folder is not a Git repository (any more).", "Dieser Ordner ist (nicht mehr) ein Git-Repository.").into());
            // The watch compares with this, so a missing folder isn't reloaded every second.
            if let Some(s) = l.snap {
                self.snap = s;
            }
            return;
        }
        self.error = None;
        if l.flags == FULL {
            self.git_dir = l.dirs.as_ref().map(|d| d.0.clone());
            self.common_dir = l.dirs.map(|d| d.1);
        }
        if let Some(h) = l.history {
            let sel_sha = self.selected.and_then(|i| self.commits.get(i)).map(|c| c.sha.clone());
            self.head = h.head;
            self.log_opts.stash_tips = h.stash_tips;
            self.commits = h.commits;
            self.rows = h.rows;
            self.lanes = h.lanes;
            self.local = h.local;
            self.remote_branches = h.remote_branches;
            self.tags = h.tags;
            self.stashes = h.stashes;
            self.stash_shas = h.stash_shas;
            self.branch_recency = h.branch_recency;
            self.worktrees = h.worktrees;
            let first = !self.shown_once;
            self.shown_once = true;
            self.selected = sel_sha.and_then(|s| self.commits.iter().position(|c| c.sha == s));
            match self.selected {
                // Still there: refresh its details (its refs may have moved), keep what is open.
                Some(i) => {
                    let sha = self.commits[i].sha.clone();
                    self.start_detail(sha, true);
                }
                None if first && !self.commits.is_empty() => self.select_commit(0),
                None => self.detail = None,
            }
        }
        if let Some((remotes, subs)) = l.config {
            self.remotes = remotes;
            self.submodules = subs;
        }
        if let Some(s) = l.status {
            self.status = s;
            self.reload_file_diff();
        }
        if let Some((state, locked)) = l.state {
            self.state = state;
            self.locked = locked;
        }
        // Only the parts this load read count as seen; a change elsewhere (a commit made while
        // only the status was read) is still found by the next look.
        if let Some(s) = l.snap {
            self.snap = git::merge_snapshot(&self.snap, s, l.covered);
        }
    }

    /// Looks at the watched .git files and reloads (in the background) the parts that changed.
    /// While git is still at work (its lock files exist) it waits for the next look.
    fn poll_changes(&mut self) {
        // While a load runs it brings its own snapshot (taken when it started); what changes
        // after that is found at the first look after it. Looking now would see a change the
        // load already covers and load it a second time.
        if self.bg_load.busy() {
            return;
        }
        let Some((g, common)) = self.git_dirs() else { return };
        // The banner shows a lock as soon as it is there (only a file check).
        self.locked = git::repo_locked_at(&g);
        let busy = git::repo_busy_at(&g, &common);
        if !busy {
            self.busy_since = None;
        }
        let now = git::snapshot(&g, &common);
        let flags = git::changed_flags(&self.snap, &now);
        if flags == 0 {
            return;
        }
        if busy {
            // Git is still writing: wait for it. A lock that stays was probably left behind
            // by a git that was stopped; after a few seconds load anyway.
            let since = *self.busy_since.get_or_insert_with(std::time::Instant::now);
            if since.elapsed() < std::time::Duration::from_secs(5) {
                return;
            }
        }
        self.snap = now;
        self.reload(flags);
    }

    fn select_commit(&mut self, i: usize) {
        if i >= self.commits.len() {
            return;
        }
        let changed = self.selected != Some(i) || self.detail.is_none();
        self.selected = Some(i);
        if changed {
            let sha = self.commits[i].sha.clone();
            self.clear_detail();
            self.start_detail(sha, false);
        }
    }

    /// Forgets what is shown of the selected commit (another one is about to load).
    fn clear_detail(&mut self) {
        self.detail = None;
        self.detail_file = None;
        self.detail_diff = Arc::new(Diff::default());
        self.detail_diff_key = None;
        self.tree.clear();
        self.tree_file = None;
        self.tree_content.clear();
        self.tree_content_for = None;
        // Diffs opened in the Commit tab are kept only for the shown commit.
        self.inline.clear();
        self.bg_inline.clear();
    }

    /// Loads a commit's details (and its file tree when that tab is open) in the background.
    /// `refresh_only`: the same commit again, keep what is shown until the new data is there.
    /// (A refresh that replaces a newly picked commit's first load still opens its first file.)
    fn start_detail(&mut self, sha: String, refresh_only: bool) {
        let path = self.path.clone();
        let want_tree = self.detail_tab == DetailTab::Tree && self.tree.is_empty();
        if !refresh_only {
            self.open_first_file = true;
        }
        self.wanted_detail = Some(sha.clone());
        self.bg_detail.start(move || {
            let d = git::commit_detail(&path, &sha);
            let t = if want_tree { Some(git::commit_tree(&path, &sha)) } else { None };
            (sha, d, t)
        });
    }

    /// Loads the diff of the selected local change in the background.
    fn reload_file_diff(&mut self) {
        if self.sel_file.is_none() && self.view == View::Changes {
            if let Some(f) = self.status.iter().find(|f| f.unstaged()).or_else(|| self.status.first()) {
                self.sel_file = Some((f.path.clone(), !f.unstaged()));
            }
        }
        if let Some((path, staged)) = self.sel_file.clone() {
            if let Some(f) = self.status.iter().find(|f| f.path == path).cloned() {
                if (staged && f.staged()) || (!staged && f.unstaged()) {
                    let (dir, opts, tab) = (self.path.clone(), self.diff_opts, self.tab_width);
                    self.file_req += 1;
                    let req = self.file_req;
                    self.bg_file.start(move || {
                        let o = git::working_diff(&dir, &f, staged, &opts);
                        let mut d = diffmodel::parse(&o.stdout, tab);
                        d.lossy = o.lossy;
                        (path, staged, opts, req, Arc::new(d))
                    });
                    return;
                }
            }
            self.sel_file = None;
        }
        self.file_diff = Arc::new(Diff::default());
        self.file_diff_key = None;
    }

    /// Loads the diff of one file of the shown commit (Changes tab) in the background.
    fn load_detail_diff(&mut self, file: String) {
        let Some(d) = &self.detail else { return };
        let (dir, sha, parents, orig, opts, tab) = (self.path.clone(), d.sha.clone(), d.parents.clone(), d.renamed_from.get(&file).cloned(), self.diff_opts, self.tab_width);
        self.detail_file = Some(file.clone());
        self.bg_ddiff.start(move || {
            let text = git::commit_file_diff(&dir, &sha, &parents, &file, orig.as_deref(), &opts);
            (sha, file, opts, Arc::new(diffmodel::parse(&text, tab)))
        });
    }

    /// Loads the shown commit's whole file tree (File Tree tab) in the background.
    fn load_tree(&mut self) {
        let Some(d) = &self.detail else { return };
        let (dir, sha) = (self.path.clone(), d.sha.clone());
        self.bg_tree.start(move || {
            let t = git::commit_tree(&dir, &sha);
            (sha, t)
        });
    }

    /// Loads one file of the shown commit (File Tree tab) in the background.
    fn load_tree_file(&mut self, file: String) {
        let Some(d) = &self.detail else { return };
        let (dir, sha) = (self.path.clone(), d.sha.clone());
        self.tree_file = Some(file.clone());
        self.bg_tree_file.start(move || {
            let o = git::run(&dir, &["show", &format!("{sha}:{file}")]);
            let content = if o.stdout.contains('\0') { tr("(binary file)", "(Binärdatei)").into() } else { o.stdout };
            (sha, file, content)
        });
    }

    /// Runs a history search that asks git (`git log` with a path or `-G`) in the background.
    fn start_search(&mut self, args: Vec<String>) {
        let (dir, key) = (self.path.clone(), (self.search_mode, self.commit_search.trim().to_string()));
        self.bg_search.start(move || {
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let out = git::run(&dir, &refs).stdout;
            (key, out.lines().map(|l| l.trim().to_string()).collect())
        });
    }

    /// Stops a running history search (its git process too).
    fn stop_search(&mut self) {
        self.bg_search.stop();
    }

    /// Shows the commit a ref (refs/heads/…, refs/remotes/…, refs/tags/…) or a full commit id
    /// points to. One that the loaded commits' labels name unambiguously needs no git call;
    /// others are looked up in the background.
    fn jump(&mut self, name: String) {
        let full_id = name.len() == 40 && name.bytes().all(|b| b.is_ascii_hexdigit());
        // The label `git log --decorate` gives it (a branch's short name could also be a
        // remote-tracking branch's, then git decides).
        let label = if let Some(t) = name.strip_prefix("refs/tags/") {
            Some(format!("tag: {t}"))
        } else if let Some(b) = name.strip_prefix("refs/heads/") {
            (!self.remote_branches.iter().any(|r| r.name == b)).then(|| b.to_string())
        } else if let Some(b) = name.strip_prefix("refs/remotes/") {
            (!self.local.iter().any(|l| l.name == b)).then(|| b.to_string())
        } else {
            None
        };
        let sha = if full_id {
            Some(name.clone())
        } else {
            label.and_then(|lb| self.commits.iter().find(|c| c.refs.iter().any(|r| *r == lb || r.strip_prefix("HEAD -> ") == Some(lb.as_str()))).map(|c| c.sha.clone()))
        };
        if let Some(sha) = sha.filter(|s| self.commits.iter().any(|c| c.sha == *s)) {
            self.jump_to(&sha);
            return;
        }
        let dir = self.path.clone();
        self.bg_jump.start(move || git::run(&dir, &["rev-parse", "--verify", "-q", &format!("{name}^{{commit}}")]).stdout.trim().to_string());
    }

    fn jump_to(&mut self, sha: &str) {
        if let Some(i) = self.commits.iter().position(|c| c.sha == sha) {
            self.view = View::Commits;
            self.select_commit(i);
            self.scroll_to = Some(i);
        }
    }

    /// Reads the status of a tab that wasn't shown yet, for the changes dot in the tab list.
    fn peek(&mut self) {
        self.peeked = true;
        let dir = self.path.clone();
        self.bg_peek.start(move || git::load_status(&dir));
    }

    /// The diff of a file opened in the Commit tab, when it is loaded (with the current options).
    fn inline_diff(&self, sha: &str, file: &str) -> Option<Arc<Diff>> {
        if self.inline_opts != self.diff_opts || self.inline_tab != self.tab_width {
            return None;
        }
        self.inline.get(&(sha.to_string(), file.to_string())).cloned()
    }

    /// Starts loading the diff of a file opened in the Commit tab, unless it is there already.
    /// At most a few load at once ("Expand All" on a big commit would start hundreds of git
    /// processes otherwise); the others start when the frame after one finishes asks again.
    fn ensure_inline(&mut self, sha: &str, file: &str) {
        let tab = self.tab_width;
        if self.inline_opts != self.diff_opts || self.inline_tab != tab {
            self.inline.clear();
            self.bg_inline.clear();
            self.inline_opts = self.diff_opts;
            self.inline_tab = tab;
        }
        let key = (sha.to_string(), file.to_string());
        if self.inline.contains_key(&key) || self.bg_inline.iter().any(|(s, f, _)| *s == key.0 && *f == key.1) || self.bg_inline.len() >= 3 {
            return;
        }
        let Some(d) = self.detail.as_ref().filter(|d| d.sha == sha) else { return };
        let (dir, parents, orig, opts) = (self.path.clone(), d.parents.clone(), d.renamed_from.get(file).cloned(), self.diff_opts);
        let (s, f) = key.clone();
        let mut bg = Bg::default();
        bg.start(move || Arc::new(diffmodel::parse(&git::commit_file_diff(&dir, &s, &parents, &f, orig.as_deref(), &opts), tab)));
        self.bg_inline.push((key.0, key.1, bg));
    }

    /// New diff options (diff toolbar): diffs on screen are loaded again with them; a tab in
    /// the background does that when it is shown next.
    fn set_diff_opts(&mut self, o: git::DiffOpts, active: bool) {
        if o == self.diff_opts {
            return;
        }
        self.diff_opts = o;
        if active {
            self.reload_diffs();
        } else {
            self.diff_stale = true;
        }
    }

    fn reload_diffs(&mut self) {
        self.diff_stale = false;
        if self.sel_file.is_some() {
            self.reload_file_diff();
        }
        if let Some(f) = self.detail_file.clone() {
            self.load_detail_diff(f);
        }
    }

    /// The repository's tab width changed (Repository Settings): its diffs load again, their
    /// column counts depend on it. (The Commit tab's diffs check it themselves, `inline_diff`.)
    fn set_tab_width(&mut self, tab: usize) {
        let tab = tab.max(1);
        if tab == self.tab_width {
            return;
        }
        self.tab_width = tab;
        if self.started {
            self.reload_diffs();
        }
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
    /// A button of the diff toolbar.
    DiffOpt(diffview::Opt),
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
    /// The repository of the tab that was on screen last frame (see `follow_active_tab`).
    shown_tab: Option<PathBuf>,
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
        perf::mark_ui_thread();
        let _ = REPAINT.set(cc.egui_ctx.clone());
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let i = rs.adapter.get_info();
            let software = i.device_type == eframe::wgpu::DeviceType::Cpu;
            let _ = extra::GPU.set((format!("{} ({:?}, {:?})", i.name, i.backend, i.device_type), software));
            perf::note(&format!("graphics: {} {:?} {:?} driver {} {}", i.name, i.backend, i.device_type, i.driver, i.driver_info));
        }
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
            shown_tab: None,
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
        extra::set_diff_view(app.p.diff_font);
        app.ws = app.ws.min(app.workspaces.len() - 1);
        let cwd_repo = std::env::current_dir().ok().and_then(|d| git::find_toplevel(&d));
        if let Some(r) = repo.and_then(|r| git::find_toplevel(&r)) {
            app.add_tab(r);
        } else if screenshot.is_some() && cwd_repo.is_some() {
            app.add_tab(cwd_repo.unwrap());
        } else {
            // Tabs load when first shown, in the background (a tab whose folder is no
            // repository any more says so then); nothing runs before the window appears.
            for t in &saved.tabs {
                let p = PathBuf::from(t);
                if p.exists() {
                    let r = app.new_tab(p);
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
            let r = self.new_tab(path);
            self.tabs.push(r);
            self.active = self.tabs.len() - 1;
        }
        // It loads at the start of the next frame (`follow_active_tab`).
        self.save();
    }

    /// Brings the tab on screen up to date when it changed in any way (switched to, a tab
    /// closed or moved away, a workspace opened, at start): its repository's diff settings
    /// apply, and it loads (first time) or catches up with what changed while it was hidden.
    fn follow_active_tab(&mut self) {
        // (Also a tab that was never shown under the same path, e.g. after a workspace switch.)
        let same = self.tabs.get(self.active).map(|t| &t.path) == self.shown_tab.as_ref();
        if same && self.tabs.get(self.active).is_none_or(|t| t.started) {
            return;
        }
        self.shown_tab = self.tabs.get(self.active).map(|t| t.path.clone());
        self.apply_repo_view();
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.show();
        }
    }

    /// Tabs that weren't shown yet get a quick status in the background, one at a time and
    /// only while the tab on screen isn't loading, so the tab list can mark their changes.
    fn peek_hidden_tabs(&mut self) {
        // Only when the dot can be seen (the tab list, with the indicator turned on).
        if !(self.show_tablist && self.prefs_tab_indicator) {
            return;
        }
        let busy = self.tabs.iter().any(|t| t.bg_peek.busy()) || self.tabs.get(self.active).is_some_and(|t| t.bg_load.busy());
        if !busy {
            if let Some(t) = self.tabs.iter_mut().find(|t| !t.started && !t.peeked) {
                t.peek();
            }
        }
    }

    /// A tab for a repository with the current settings; it loads when first shown.
    fn new_tab(&self, path: PathBuf) -> Repo {
        let mut r = Repo::new(path.clone());
        r.name = self.tab_name(&path);
        r.log_opts.topo = self.p.topo;
        r.diff_opts = self.diff_opts();
        r.tab_width = self.repo_cfg.get(&path.display().to_string()).map(|s| s.tab_width).unwrap_or(4).max(1);
        // A command still running there (the tab was closed and opened again, or the
        // workspace switched) keeps the hunk buttons waiting in the new tab too.
        r.jobs_pending = self.jobs.iter().filter(|j| j.blocks_hunks && j.state() == JobState::Running && j.cwd.as_ref().is_some_and(|c| *c == path || c.starts_with(&path))).map(|j| j.id).collect();
        if !r.jobs_pending.is_empty() {
            r.hunk_ok_from = u64::MAX;
        }
        r
    }

    /// The diff options chosen in the diff toolbar.
    fn diff_opts(&self) -> git::DiffOpts {
        git::DiffOpts { context: self.p.diff_context, ignore_ws: self.p.diff_ignore_ws, full: self.p.diff_full }
    }

    /// How diffs are drawn (diff toolbar).
    fn diff_look(&self) -> diffview::Look {
        diffview::Look { split: self.p.diff_split, wrap: self.p.diff_wrap, show_ws: self.p.diff_show_ws }
    }

    /// The diff toolbar's state, for drawing it.
    fn diff_bar(&self) -> diffview::Bar {
        diffview::Bar { look: self.diff_look(), opts: self.diff_opts() }
    }

    /// A diff toolbar button: remember the choice; diffs that need other git options load again.
    fn diff_opt(&mut self, o: diffview::Opt) {
        let p = &mut self.p;
        match o {
            diffview::Opt::IgnoreWs => p.diff_ignore_ws = !p.diff_ignore_ws,
            diffview::Opt::ShowWs => p.diff_show_ws = !p.diff_show_ws,
            diffview::Opt::Wrap => p.diff_wrap = !p.diff_wrap,
            diffview::Opt::Less => p.diff_context = diffview::less_context(p.diff_context),
            diffview::Opt::More => p.diff_context = diffview::more_context(p.diff_context),
            diffview::Opt::Full => p.diff_full = !p.diff_full,
            diffview::Opt::Split => p.diff_split = !p.diff_split,
        }
        let opts = self.diff_opts();
        let active = self.active;
        for (i, t) in self.tabs.iter_mut().enumerate() {
            t.set_diff_opts(opts, i == active);
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
        // A command that can change the files or the index: the diff on screen won't match
        // them any more, so the hunk buttons wait until a diff loaded after it is there.
        let blocks = blocks_hunks(&spec);
        if blocks {
            if let Some(c) = &spec.cwd {
                for t in self.tabs.iter_mut().filter(|t| t.path == *c || c.starts_with(&t.path)) {
                    t.jobs_pending.push(id);
                    t.hunk_ok_from = u64::MAX;
                }
            }
        }
        let mut job = git::start_job(id, spec, ctx.clone());
        job.blocks_hunks = blocks;
        self.jobs.push(job);
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

    fn quick_stdin(&mut self, title: String, args: Vec<String>, stdin: String, ctx: &egui::Context) -> Option<u64> {
        let r = self.repo()?;
        let display = format!("git {}   {}", args.join(" "), tr("(patch via standard input)", "(Patch über Standardeingabe)"));
        let cwd = Some(r.path.clone());
        Some(self.spawn(JobSpec { title, program: "git".into(), args, display, cwd, stdin: Some(stdin), env: vec![], changes_repo: true, detached: false, automatic: false }, ctx))
    }

    fn job(&self, id: u64) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    fn poll_jobs(&mut self) {
        let mut failed: Option<u64> = None;
        let mut refresh: Vec<PathBuf> = vec![];
        let mut open: Vec<PathBuf> = vec![];
        let mut ended: Vec<u64> = vec![];
        for j in self.jobs.iter_mut() {
            if j.handled {
                continue;
            }
            match j.state() {
                JobState::Running => {}
                JobState::Done(code) => {
                    j.handled = true;
                    ended.push(j.id);
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
        // Commands that changed a repository ended: its hunk buttons wait for a diff loaded
        // from now on (the one on screen no longer matches the files).
        for t in self.tabs.iter_mut() {
            if t.jobs_pending.is_empty() {
                continue;
            }
            t.jobs_pending.retain(|id| !ended.contains(id) && self.jobs.iter().any(|j| j.id == *id));
            if t.jobs_pending.is_empty() {
                t.hunk_ok_from = t.file_req + 1;
                // The diff again right away, not only after the full reload below.
                if t.started {
                    t.reload_file_diff();
                }
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
        let t0 = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        self.frames += 1;
        if perf::enabled() {
            // The whole previous frame: building it, tessellating and painting.
            let prev = frame.info().cpu_usage.map(|s| s as f64 * 1000.0).unwrap_or(0.0);
            if prev > perf::SLOW_FRAME_MS {
                perf::note(&format!("slow frame (whole, painting included): {prev:.1} ms"));
            }
        }
        // The taskbar icon: set again over the first frames (the window may appear late).
        if matches!(self.frames, 2 | 30 | 120) {
            crate::winicon::apply(frame);
        }
        self.poll_jobs();
        self.handle_screenshot(&ctx);

        // Take over what background loading has finished (git never runs in this thread for
        // the views).
        for t in self.tabs.iter_mut() {
            t.poll_bg();
        }
        self.follow_active_tab();
        self.peek_hidden_tabs();
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && !self.was_focused {
            // Back in the window: files may have changed outside (editor, terminal). Changes in
            // .git itself are caught by the watch below.
            if let Some(t) = self.tabs.get_mut(self.active).filter(|t| t.started) {
                t.reload(git::RELOAD_INDEX);
                // An editor may have changed just the line ends: Discard Hunk asks again.
                t.discard_checked = None;
            }
        }
        self.was_focused = focused;
        // Watch the .git folder and reload only what changed.
        let now = ctx.input(|i| i.time);
        if now - self.last_poll >= POLL_SECONDS {
            self.last_poll = now;
            if let Some(t) = self.tabs.get_mut(self.active).filter(|t| t.started) {
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
        if perf::enabled() {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            if ms > perf::SLOW_FRAME_MS {
                perf::note(&format!("slow frame (building the window): {ms:.1} ms"));
            }
        }
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
                            t.reload_file_diff();
                            t.reload(git::RELOAD_INDEX);
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
                        // A tab to the left of the active one: the active one stays active.
                        if i < self.active {
                            self.active -= 1;
                        }
                        if self.active >= self.tabs.len() {
                            self.active = self.tabs.len().saturating_sub(1);
                        }
                        self.save();
                    }
                }
                // The tab that becomes active is brought up to date by `follow_active_tab`
                // at the start of the next frame (also after closing or moving tabs).
                Act::SwitchTab(i) => {
                    self.manager_active = false;
                    self.commands_view = false;
                    self.active = i;
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
                            t.load_tree();
                        }
                        if dt == DetailTab::Changes && t.detail_file.is_none() {
                            if let Some(f) = t.detail.as_ref().and_then(|d| d.files.first()).map(|f| f.1.clone()) {
                                t.load_detail_diff(f);
                            }
                        }
                    }
                }
                Act::DetailFile(f) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        if t.detail.is_some() {
                            t.load_detail_diff(f);
                            t.detail_tab = DetailTab::Changes;
                        }
                    }
                }
                Act::TreeFile(f) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.load_tree_file(f);
                    }
                }
                Act::DiffOpt(o) => self.diff_opt(o),
                Act::Terminal => {
                    if let Some(r) = self.repo() {
                        auth::open_terminal(self.shell, &r.path);
                    }
                }
                Act::Explorer(p) => auth::open_explorer(&p),
                Act::QuickLaunch => self.quick = Some(String::new()),
                Act::JumpToRef(name) => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.jump(name);
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
                            t.clear_detail();
                            t.start_detail(sha, false);
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
                        // A tab to the left of the active one: the active one stays active.
                        if i < self.active {
                            self.active -= 1;
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
#[cfg(test)]
mod tests {
    use super::*;

    fn spec(args: &[&str], detached: bool, automatic: bool) -> JobSpec {
        JobSpec {
            title: String::new(),
            program: "git".into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            display: String::new(),
            cwd: None,
            stdin: None,
            env: vec![],
            changes_repo: true,
            detached,
            automatic,
        }
    }

    #[test]
    fn which_commands_make_hunk_buttons_wait() {
        assert!(blocks_hunks(&spec(&["add", "--", "a.txt"], false, false)));
        assert!(blocks_hunks(&spec(&["apply", "--cached", "-"], false, false)));
        assert!(blocks_hunks(&spec(&["-c", "alias.yagui-seq=!git commit && git push", "yagui-seq"], false, false)));
        assert!(!blocks_hunks(&spec(&["push", "origin"], false, false)));
        assert!(!blocks_hunks(&spec(&["-c", "credential.helper=", "fetch", "--all"], false, false)));
        // A merge tool runs on its own (its end isn't known), the automatic fetch neither.
        assert!(!blocks_hunks(&spec(&["mergetool"], true, false)));
        assert!(!blocks_hunks(&spec(&["fetch"], false, true)));
        // Subcommands of tools: only those that update the work tree.
        assert!(!blocks_hunks(&spec(&["lfs", "push", "origin"], false, false)));
        assert!(blocks_hunks(&spec(&["lfs", "pull"], false, false)));
        assert!(!blocks_hunks(&spec(&["svn", "fetch"], false, false)));
        assert!(blocks_hunks(&spec(&["svn", "rebase"], false, false)));
    }
}
