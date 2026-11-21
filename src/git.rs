//! Running git: quick synchronous reads for the views, and background jobs for user actions
//! whose output streams into the activity log.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Options added to every call: stable, readable output. Never shown to the user because they
/// don't change what the command does.
const HIDDEN: &[&str] = &["-c", "core.quotepath=false", "-c", "color.ui=false"];

pub fn base_command(program: &str, cwd: Option<&Path>) -> Command {
    let mut c = Command::new(program);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    c.env("GIT_TERMINAL_PROMPT", "0");
    c.env("GIT_EDITOR", "true");
    c.env("GIT_MERGE_AUTOEDIT", "no");
    c.env("GIT_PAGER", "cat");
    c.env("PAGER", "cat");
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

pub fn git_command(cwd: Option<&Path>, args: &[String]) -> Command {
    let mut c = base_command("git", cwd);
    c.args(HIDDEN);
    c.args(args);
    c
}

pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

/// Runs git and waits. Used for reading repository data.
pub fn run(cwd: &Path, args: &[&str]) -> Out {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    run_owned(Some(cwd), &args, None)
}

pub fn run_owned(cwd: Option<&Path>, args: &[String], stdin: Option<&str>) -> Out {
    let mut c = git_command(cwd, args);
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    match c.spawn() {
        Ok(mut child) => {
            if let (Some(s), Some(mut si)) = (stdin, child.stdin.take()) {
                let s = s.to_string();
                std::thread::spawn(move || {
                    let _ = si.write_all(s.as_bytes());
                });
            }
            match child.wait_with_output() {
                Ok(o) => Out {
                    code: o.status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
                },
                Err(e) => Out { code: -1, stdout: String::new(), stderr: e.to_string() },
            }
        }
        Err(e) => Out { code: -1, stdout: String::new(), stderr: format!("Git konnte nicht gestartet werden: {e}") },
    }
}

// ---------------------------------------------------------------------------------------------
// Background jobs

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Done(i32),
    Started,
}

pub struct Job {
    pub id: u64,
    pub title: String,
    pub display: String,
    pub cwd: Option<PathBuf>,
    pub output: Arc<Mutex<String>>,
    pub state: Arc<Mutex<JobState>>,
    pub cancel: Arc<AtomicBool>,
    pub started: Instant,
    pub finished: Arc<Mutex<Option<Instant>>>,
    pub changes_repo: bool,
    pub handled: bool,
    pub expanded: bool,
    pub automatic: bool,
}

impl Job {
    pub fn state(&self) -> JobState {
        *self.state.lock().unwrap()
    }
    pub fn output(&self) -> String {
        self.output.lock().unwrap().clone()
    }
}

pub struct JobSpec {
    pub title: String,
    pub program: String,
    pub args: Vec<String>,
    pub display: String,
    pub cwd: Option<PathBuf>,
    pub stdin: Option<String>,
    pub env: Vec<(String, String)>,
    pub changes_repo: bool,
    pub detached: bool,
    pub automatic: bool,
}

pub fn start_job(id: u64, spec: JobSpec, ctx: eframe::egui::Context) -> Job {
    let output = Arc::new(Mutex::new(String::new()));
    let state = Arc::new(Mutex::new(JobState::Running));
    let cancel = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(Mutex::new(None));
    let job = Job {
        id,
        title: spec.title.clone(),
        display: spec.display.clone(),
        cwd: spec.cwd.clone(),
        output: output.clone(),
        state: state.clone(),
        cancel: cancel.clone(),
        started: Instant::now(),
        finished: finished.clone(),
        changes_repo: spec.changes_repo,
        handled: false,
        expanded: false,
        automatic: spec.automatic,
    };

    let mut cmd = if spec.program == "git" {
        git_command(spec.cwd.as_deref(), &spec.args)
    } else {
        let mut c = base_command(&spec.program, spec.cwd.as_deref());
        c.args(&spec.args);
        c
    };
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }

    if spec.detached {
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(0);
        match cmd.spawn() {
            Ok(_) => {
                *output.lock().unwrap() = "Das Programm wurde in einem eigenen Fenster gestartet.".into();
                *state.lock().unwrap() = JobState::Started;
            }
            Err(e) => {
                *output.lock().unwrap() = format!("Konnte nicht gestartet werden: {e}");
                *state.lock().unwrap() = JobState::Done(-1);
            }
        }
        *finished.lock().unwrap() = Some(Instant::now());
        return job;
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    std::thread::spawn(move || {
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                output.lock().unwrap().push_str(&format!("Git konnte nicht gestartet werden: {e}\n"));
                *state.lock().unwrap() = JobState::Done(-1);
                *finished.lock().unwrap() = Some(Instant::now());
                ctx.request_repaint();
                return;
            }
        };
        if let (Some(s), Some(mut si)) = (spec.stdin, child.stdin.take()) {
            std::thread::spawn(move || {
                let _ = si.write_all(s.as_bytes());
            });
        }
        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            readers.push(pipe_reader(out, output.clone(), ctx.clone()));
        }
        if let Some(err) = child.stderr.take() {
            readers.push(pipe_reader(err, output.clone(), ctx.clone()));
        }
        let code = loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                output.lock().unwrap().push_str("\n[Abgebrochen]\n");
            }
            match child.try_wait() {
                Ok(Some(st)) => break st.code().unwrap_or(-1),
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(40)),
                Err(_) => break -1,
            }
        };
        for r in readers {
            let _ = r.join();
        }
        *state.lock().unwrap() = JobState::Done(code);
        *finished.lock().unwrap() = Some(Instant::now());
        ctx.request_repaint();
    });
    job
}

fn pipe_reader<R: Read + Send + 'static>(mut r: R, out: Arc<Mutex<String>>, ctx: eframe::egui::Context) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let s = String::from_utf8_lossy(&buf[..n]).replace('\r', "\n");
                    let mut o = out.lock().unwrap();
                    o.push_str(&s);
                    if o.len() > 400_000 {
                        let cut = o.len() - 300_000;
                        let cut = (cut..o.len()).find(|i| o.is_char_boundary(*i)).unwrap_or(cut);
                        o.drain(..cut);
                    }
                    drop(o);
                    ctx.request_repaint();
                }
            }
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Repository data

#[derive(Clone, Default)]
pub struct Commit {
    pub sha: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub date: String,
    pub refs: Vec<String>,
    pub subject: String,
}

#[derive(Clone, Default)]
pub struct RefInfo {
    pub name: String,
    pub short_sha: String,
    pub upstream: String,
    pub track: String,
    pub is_head: bool,
}

#[derive(Clone)]
pub struct FileStatus {
    pub path: String,
    pub orig: Option<String>,
    pub index: char,
    pub worktree: char,
}

impl FileStatus {
    pub fn staged(&self) -> bool {
        self.index != ' ' && self.index != '?' && self.index != '!'
    }
    pub fn unstaged(&self) -> bool {
        self.worktree != ' '
    }
    pub fn conflicted(&self) -> bool {
        matches!((self.index, self.worktree), ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D'))
    }
}

pub fn find_toplevel(dir: &Path) -> Option<PathBuf> {
    let o = run(dir, &["rev-parse", "--show-toplevel"]);
    if o.ok() {
        let p = o.stdout.trim();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

/// Reference namespaces that "all commits" leaves out.
pub const HIDDEN_REF_PREFIXES: [&str; 3] = ["refs/bisect/", "refs/worktree/", "refs/rewritten/"];

/// Which tips the history starts from: every reference when `all` is set (else HEAD);
/// `reflog` adds the commits the reflogs mention.
#[derive(Clone, Copy)]
pub struct LogOptions {
    pub all: bool,
    pub hide_stashes: bool,
    pub reflog: bool,
}

pub fn log_tip_args(o: LogOptions) -> Vec<String> {
    let mut v = Vec::new();
    if o.all {
        for p in HIDDEN_REF_PREFIXES {
            v.push(format!("--exclude={p}*"));
        }
        if o.hide_stashes {
            v.push("--exclude=refs/stash".into());
        }
        v.push("--all".into());
    }
    if o.reflog {
        v.push("--reflog".into());
    }
    v
}

pub fn load_commits(dir: &Path, limit: usize, opts: LogOptions) -> Vec<Commit> {
    let fmt = "--format=%H%x1f%h%x1f%P%x1f%an%x1f%ae%x1f%ad%x1f%D%x1f%s%x1e";
    let n = format!("-n{limit}");
    let tips = log_tip_args(opts);
    let mut args = vec!["log", "--date-order", fmt, "--date=format:%d.%m.%Y %H:%M", n.as_str()];
    args.extend(tips.iter().map(|s| s.as_str()));
    let o = run(dir, &args);
    let mut out = Vec::new();
    for rec in o.stdout.split('\x1e') {
        let rec = rec.trim_start_matches(['\n', '\r']);
        if rec.is_empty() {
            continue;
        }
        let f: Vec<&str> = rec.split('\x1f').collect();
        if f.len() < 8 {
            continue;
        }
        out.push(Commit {
            sha: f[0].into(),
            short: f[1].into(),
            parents: f[2].split_whitespace().map(String::from).collect(),
            author: f[3].into(),
            email: f[4].into(),
            date: f[5].into(),
            refs: f[6].split(", ").filter(|s| !s.is_empty()).map(String::from).collect(),
            subject: f[7].trim_end().into(),
        });
    }
    out
}

pub fn load_refs(dir: &Path) -> (Vec<RefInfo>, Vec<RefInfo>, Vec<RefInfo>) {
    let o = run(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname)%1f%(objectname:short)%1f%(upstream:short)%1f%(upstream:track)%1f%(HEAD)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    );
    let (mut local, mut remote, mut tags) = (Vec::new(), Vec::new(), Vec::new());
    for line in o.stdout.lines() {
        let f: Vec<&str> = line.split('\x1f').collect();
        if f.len() < 5 {
            continue;
        }
        let full = f[0];
        let info = |name: &str| RefInfo {
            name: name.to_string(),
            short_sha: f[1].into(),
            upstream: f[2].into(),
            track: f[3].into(),
            is_head: f[4].trim() == "*",
        };
        if let Some(n) = full.strip_prefix("refs/heads/") {
            local.push(info(n));
        } else if let Some(n) = full.strip_prefix("refs/remotes/") {
            if !n.ends_with("/HEAD") {
                remote.push(info(n));
            }
        } else if let Some(n) = full.strip_prefix("refs/tags/") {
            tags.push(info(n));
        }
    }
    (local, remote, tags)
}

pub fn load_remotes(dir: &Path) -> Vec<(String, String)> {
    let o = run(dir, &["remote", "-v"]);
    let mut v: Vec<(String, String)> = Vec::new();
    for line in o.stdout.lines() {
        let mut it = line.split_whitespace();
        if let (Some(n), Some(u)) = (it.next(), it.next()) {
            if !v.iter().any(|(x, _)| x == n) {
                v.push((n.into(), u.into()));
            }
        }
    }
    v
}

pub fn load_stashes(dir: &Path) -> Vec<(String, String)> {
    let o = run(dir, &["stash", "list", "--format=%gd%x1f%s"]);
    o.stdout
        .lines()
        .filter_map(|l| l.split_once('\x1f').map(|(a, b)| (a.to_string(), b.to_string())))
        .collect()
}

pub fn load_submodules(dir: &Path) -> Vec<String> {
    if !dir.join(".gitmodules").exists() {
        return Vec::new();
    }
    let o = run(dir, &["submodule", "status"]);
    o.stdout
        .lines()
        .filter_map(|l| l.trim_start_matches([' ', '+', '-', 'U']).split_whitespace().nth(1).map(String::from))
        .collect()
}

pub fn load_worktrees(dir: &Path) -> Vec<(String, String)> {
    let o = run(dir, &["worktree", "list", "--porcelain"]);
    let mut v = Vec::new();
    let mut path = String::new();
    for line in o.stdout.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = p.to_string();
        } else if let Some(b) = line.strip_prefix("branch ") {
            v.push((path.clone(), b.trim_start_matches("refs/heads/").to_string()));
        } else if line == "detached" {
            v.push((path.clone(), "(detached)".into()));
        }
    }
    v
}

pub fn load_status(dir: &Path) -> Vec<FileStatus> {
    let o = run(dir, &["status", "--porcelain=v1", "-z", "-uall"]);
    let mut v = Vec::new();
    let mut parts = o.stdout.split('\0').peekable();
    while let Some(p) = parts.next() {
        if p.len() < 4 {
            continue;
        }
        let mut ch = p.chars();
        let x = ch.next().unwrap_or(' ');
        let y = ch.next().unwrap_or(' ');
        let path = p[3..].to_string();
        let orig = if x == 'R' || x == 'C' { parts.next().map(String::from) } else { None };
        v.push(FileStatus { path, orig, index: x, worktree: y });
    }
    v
}

pub fn head_name(dir: &Path) -> String {
    let o = run(dir, &["symbolic-ref", "--short", "-q", "HEAD"]);
    if o.ok() && !o.stdout.trim().is_empty() {
        return o.stdout.trim().to_string();
    }
    let o = run(dir, &["rev-parse", "--short", "HEAD"]);
    if o.ok() {
        format!("(detached {})", o.stdout.trim())
    } else {
        "(noch kein Commit)".into()
    }
}

pub fn git_dir(dir: &Path) -> Option<PathBuf> {
    let o = run(dir, &["rev-parse", "--absolute-git-dir"]);
    if o.ok() { Some(PathBuf::from(o.stdout.trim())) } else { None }
}

/// What special operation the repository is in the middle of, if any.
pub fn repo_state(dir: &Path) -> Option<&'static str> {
    let g = git_dir(dir)?;
    if g.join("rebase-merge").exists() || g.join("rebase-apply").join("rebasing").exists() {
        Some("rebase")
    } else if g.join("rebase-apply").exists() {
        Some("am")
    } else if g.join("MERGE_HEAD").exists() {
        Some("merge")
    } else if g.join("CHERRY_PICK_HEAD").exists() {
        Some("cherry-pick")
    } else if g.join("REVERT_HEAD").exists() {
        Some("revert")
    } else if g.join("BISECT_START").exists() {
        // BISECT_START (not BISECT_LOG) marks the bisect state: it is the bisect file in WATCHED.
        Some("bisect")
    } else if g.join("SQUASH_MSG").exists() {
        // SQUASH_MSG belongs to the same "repository state" group (RELOAD_STATE):
        // a `merge --squash` prepared changes and a message that still need a commit.
        Some("squash")
    } else {
        None
    }
}

/// The lock file while another git process holds a lock (HEAD.lock / index.lock).
pub fn repo_locked(dir: &Path) -> Option<&'static str> {
    let g = git_dir(dir)?;
    ["index.lock", "HEAD.lock"].into_iter().find(|f| g.join(f).exists())
}

// ---------------------------------------------------------------------------------------------
// Watching the .git folder

pub const RELOAD_CONFIG: u32 = 0x01; // config, config.lock
pub const RELOAD_HEAD: u32 = 0x02; // HEAD, HEAD.lock
pub const RELOAD_REFS: u32 = 0x04; // packed-refs, packed-refs.lock (and refs/)
pub const RELOAD_REFTABLE: u32 = 0x08; // reftable/
pub const RELOAD_STATE: u32 = 0x10; // MERGE_HEAD, CHERRY_PICK_HEAD(.lock), REVERT_HEAD(.lock), SQUASH_MSG, BISECT_START, rebase-*
pub const RELOAD_INDEX: u32 = 0x20; // index, index.lock
pub const RELOAD_IGNORE: u32 = 0x40; // info/exclude(.lock): ignore rules changed -> status

/// Every watched file with its reload flag.
pub const WATCHED: &[(&str, u32)] = &[
    ("config", RELOAD_CONFIG),
    ("config.lock", RELOAD_CONFIG),
    ("HEAD", RELOAD_HEAD),
    ("HEAD.lock", RELOAD_HEAD),
    ("packed-refs", RELOAD_REFS),
    ("packed-refs.lock", RELOAD_REFS),
    ("reftable", RELOAD_REFTABLE),
    ("MERGE_HEAD", RELOAD_STATE),
    ("CHERRY_PICK_HEAD", RELOAD_STATE),
    ("CHERRY_PICK_HEAD.lock", RELOAD_STATE),
    ("REVERT_HEAD", RELOAD_STATE),
    ("REVERT_HEAD.lock", RELOAD_STATE),
    ("SQUASH_MSG", RELOAD_STATE),
    ("BISECT_START", RELOAD_STATE),
    ("rebase-apply", RELOAD_STATE),
    ("rebase-merge", RELOAD_STATE),
    ("index", RELOAD_INDEX),
    ("index.lock", RELOAD_INDEX),
    ("info/exclude", RELOAD_IGNORE),
    ("info/exclude.lock", RELOAD_IGNORE),
];

/// A fingerprint (exists, size, modified time) of each watched file.
pub type Snapshot = Vec<(u32, Option<(u64, std::time::SystemTime)>)>;

pub fn snapshot(git_dir: &Path) -> Snapshot {
    let mut v: Snapshot = WATCHED
        .iter()
        .map(|(f, flag)| {
            let m = std::fs::metadata(git_dir.join(f)).ok();
            (*flag, m.and_then(|m| m.modified().ok().map(|t| (m.len(), t))))
        })
        .collect();
    // Loose branch/tag files under refs/: newest change counts as a refs change.
    v.push((RELOAD_REFS, newest_in(&git_dir.join("refs"), 4)));
    v
}

fn newest_in(dir: &Path, depth: u32) -> Option<(u64, std::time::SystemTime)> {
    let mut best: Option<(u64, std::time::SystemTime)> = None;
    let mut count = 0u64;
    let Ok(rd) = std::fs::read_dir(dir) else { return None };
    for e in rd.flatten() {
        let Ok(m) = e.metadata() else { continue };
        count += 1;
        let cand = if m.is_dir() {
            if depth == 0 { None } else { newest_in(&e.path(), depth - 1) }
        } else {
            m.modified().ok().map(|t| (m.len(), t))
        };
        if let Some(c) = cand {
            count += c.0;
            if best.map(|b| c.1 > b.1).unwrap_or(true) {
                best = Some(c);
            }
        }
    }
    best.map(|(_, t)| (count, t))
}

/// Which parts changed between two snapshots, as reload flags.
pub fn changed_flags(old: &Snapshot, new: &Snapshot) -> u32 {
    old.iter().zip(new.iter()).filter(|(a, b)| a.1 != b.1).fold(0, |acc, (a, _)| acc | a.0)
}

pub struct CommitDetail {
    pub sha: String,
    pub author: String,
    pub email: String,
    pub date: String,
    pub committer: String,
    pub parents: Vec<String>,
    pub refs: String,
    pub message: String,
    pub files: Vec<(String, String)>,
}

pub fn commit_detail(dir: &Path, sha: &str) -> CommitDetail {
    let o = run(dir, &["show", "-s", "--format=%H%x1f%an%x1f%ae%x1f%ad%x1f%cn%x1f%P%x1f%D%x1f%B", "--date=format:%d.%m.%Y %H:%M:%S %z", sha]);
    let f: Vec<&str> = o.stdout.splitn(8, '\x1f').collect();
    let g = |i: usize| f.get(i).map(|s| s.to_string()).unwrap_or_default();
    let files_out = run(dir, &["diff-tree", "--no-commit-id", "-r", "--name-status", "-M", "--root", sha]);
    let files = files_out
        .stdout
        .lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let st = it.next()?.to_string();
            let path = it.last()?.to_string();
            Some((st.chars().next().unwrap_or('M').to_string(), path))
        })
        .collect();
    CommitDetail {
        sha: g(0),
        author: g(1),
        email: g(2),
        date: g(3),
        committer: g(4),
        parents: g(5).split_whitespace().map(String::from).collect(),
        refs: g(6),
        message: g(7).trim_end().to_string(),
        files,
    }
}

pub fn commit_file_diff(dir: &Path, sha: &str, path: &str) -> String {
    run(dir, &["show", "--format=", "-M", sha, "--", path]).stdout
}

pub fn commit_tree(dir: &Path, sha: &str) -> Vec<String> {
    run(dir, &["ls-tree", "-r", "--name-only", sha]).stdout.lines().map(String::from).collect()
}

pub fn working_diff(dir: &Path, f: &FileStatus, staged: bool) -> String {
    if f.index == '?' && !staged {
        let o = run(dir, &["diff", "--no-index", "--", "/dev/null", &f.path]);
        return o.stdout;
    }
    if staged {
        run(dir, &["diff", "--cached", "-M", "--", &f.path]).stdout
    } else {
        run(dir, &["diff", "--", &f.path]).stdout
    }
}

/// A diff split into its file header and hunks, so single hunks can be staged.
pub struct ParsedDiff {
    pub header: String,
    pub hunks: Vec<String>,
    /// Plain-language facts from the header lines, in the order git writes them.
    pub facts: Vec<String>,
    /// For each hunk: (old start, new start, context text after the second @@).
    pub hunk_info: Vec<(u32, u32, String)>,
    pub binary: bool,
}

/// Splits a diff into its parts: header lines (diff type, old/new mode, new/deleted file
/// mode, similarity index, rename-from/-to, copy-from/-to, index sha, binary files differ),
/// hunk headers (old start, new start, context) and context/added/deleted lines.
pub fn parse_diff(text: &str) -> ParsedDiff {
    let mut header = String::new();
    let mut hunks: Vec<String> = Vec::new();
    let mut facts = Vec::new();
    let mut hunk_info = Vec::new();
    let mut binary = false;
    for line in text.split_inclusive('\n') {
        if line.starts_with("@@") {
            hunk_info.push(parse_hunk_header(line));
            hunks.push(line.to_string());
        } else if let Some(h) = hunks.last_mut() {
            h.push_str(line);
        } else {
            header.push_str(line);
            let l = line.trim_end();
            let fact = if let Some(m) = l.strip_prefix("new file mode ") {
                Some(format!("Neue Datei (Modus {m})"))
            } else if let Some(m) = l.strip_prefix("deleted file mode ") {
                Some(format!("Datei gelöscht (Modus {m})"))
            } else if let Some(m) = l.strip_prefix("old mode ") {
                Some(format!("Dateimodus vorher {m}"))
            } else if let Some(m) = l.strip_prefix("new mode ") {
                Some(format!("Dateimodus jetzt {m}"))
            } else if let Some(s) = l.strip_prefix("similarity index ") {
                Some(format!("Ähnlichkeit {s}"))
            } else if let Some(p) = l.strip_prefix("rename from ") {
                Some(format!("Umbenannt von {p}"))
            } else if let Some(p) = l.strip_prefix("rename to ") {
                Some(format!("Umbenannt nach {p}"))
            } else if let Some(p) = l.strip_prefix("copy from ") {
                Some(format!("Kopiert von {p}"))
            } else if let Some(p) = l.strip_prefix("copy to ") {
                Some(format!("Kopiert nach {p}"))
            } else if l.starts_with("Binary files ") || l == "GIT binary patch" {
                binary = true;
                Some("Binärdatei – Inhalt kann nicht zeilenweise gezeigt werden".to_string())
            } else {
                None
            };
            if let Some(f) = fact {
                facts.push(f);
            }
        }
    }
    ParsedDiff { header, hunks, facts, hunk_info, binary }
}

fn parse_hunk_header(line: &str) -> (u32, u32, String) {
    // "@@ -12,5 +12,7 @@ fn context"
    let mut old = 0;
    let mut new = 0;
    let mut parts = line.trim_end().splitn(4, ' ');
    let _ = parts.next();
    if let Some(o) = parts.next() {
        old = o.trim_start_matches('-').split(',').next().and_then(|n| n.parse().ok()).unwrap_or(0);
    }
    if let Some(n) = parts.next() {
        new = n.trim_start_matches('+').split(',').next().and_then(|n| n.parse().ok()).unwrap_or(0);
    }
    let ctx = line.splitn(3, "@@").nth(2).map(|s| s.trim().to_string()).unwrap_or_default();
    (old, new, ctx)
}

pub fn file_log_entries(dir: &Path, path: &str) -> String {
    run(dir, &["log", "--follow", "--format=%h  %ad  %an  %s", "--date=format:%d.%m.%Y", "--", path]).stdout
}
