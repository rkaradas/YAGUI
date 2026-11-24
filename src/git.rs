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
    /// stdout was not valid UTF-8 (e.g. a Windows-1252 file in a diff): `stdout` has U+FFFD in
    /// those places, so it must not be written back (as a patch, for instance).
    pub lossy: bool,
}

impl Out {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    fn failed(e: String) -> Self {
        Out { code: -1, stdout: String::new(), stderr: e, lossy: false }
    }
}

/// A started process and everything it starts, so stopping it stops all of them. On Windows
/// `git` is usually Git for Windows' launcher (Git\cmd\git.exe), which runs the real git as a
/// child: killing the launcher alone leaves that one running. So the process goes into a job
/// object, which its children join, and stopping ends the whole job.
struct ProcessTree {
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}

impl ProcessTree {
    fn new(child: &std::process::Child) -> Self {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};
            // SAFETY: a fresh unnamed job object; the process handle belongs to `child`, which
            // outlives this call. A failure leaves `job` null and only the launcher is killed.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if !job.is_null() && AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0 {
                    windows_sys::Win32::Foundation::CloseHandle(job);
                    return ProcessTree { job: std::ptr::null_mut() };
                }
                ProcessTree { job }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            ProcessTree {}
        }
    }

    fn kill(&self, child: &mut std::process::Child) {
        #[cfg(windows)]
        if !self.job.is_null() {
            // SAFETY: `job` is the job object created in `new`, still open.
            unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1);
            }
        }
        let _ = child.kill();
    }

    /// Like `kill`, for a command the user started: a web browser that the command opened (Git
    /// Credential Manager opens one to sign in) stays open, with the user's tabs in it.
    fn kill_command(&self, child: &mut std::process::Child) {
        #[cfg(windows)]
        if !self.job.is_null() {
            // SAFETY: `job` is open; the id list buffer is large enough for the size given, and
            // every process handle opened here is closed again.
            unsafe { kill_job_except_browsers(self.job) };
        }
        let _ = child.kill();
    }
}

#[cfg(windows)]
unsafe fn kill_job_except_browsers(job: windows_sys::Win32::Foundation::HANDLE) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{JobObjectBasicProcessIdList, QueryInformationJobObject};
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE};
    #[repr(C)]
    struct Ids {
        assigned: u32,
        listed: u32,
        ids: [usize; 512],
    }
    let mut list = Ids { assigned: 0, listed: 0, ids: [0; 512] };
    let ok = unsafe { QueryInformationJobObject(job, JobObjectBasicProcessIdList, &mut list as *mut Ids as *mut _, std::mem::size_of::<Ids>() as u32, std::ptr::null_mut()) };
    if ok == 0 {
        unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(job, 1) };
        return;
    }
    let _ = list.assigned;
    for &pid in &list.ids[..(list.listed as usize).min(512)] {
        let h = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32) };
        if h.is_null() {
            continue;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let named = unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) } != 0;
        let image = if named { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() };
        if !is_browser(&image) {
            unsafe { TerminateProcess(h, 1) };
        }
        unsafe { CloseHandle(h) };
    }
}

/// A web browser's program (by its file name).
#[cfg_attr(not(windows), allow(dead_code))]
fn is_browser(image: &str) -> bool {
    let file = image.rsplit(['\\', '/']).next().unwrap_or("").to_ascii_lowercase();
    matches!(file.as_str(), "chrome.exe" | "msedge.exe" | "firefox.exe" | "brave.exe" | "opera.exe" | "vivaldi.exe" | "iexplore.exe" | "arc.exe" | "librewolf.exe" | "waterfox.exe")
}

/// Waits for a pipe reader thread. A process the command started can keep its pipe open after
/// the command itself ended; when `stop` says so meanwhile, everything is ended.
fn join_reader<T: Default>(h: std::thread::JoinHandle<T>, stop: impl Fn() -> bool, end: impl Fn()) -> T {
    let mut ended = false;
    while !h.is_finished() {
        if !ended && stop() {
            ended = true;
            end();
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    h.join().unwrap_or_default()
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        #[cfg(windows)]
        if !self.job.is_null() {
            // SAFETY: closing our own handle; processes still running in the job go on.
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.job);
            }
        }
    }
}

thread_local! {
    /// Set in a background loader's thread (`set_thread_cancel`): when it is raised, the git
    /// process that thread waits for is stopped, and later runs there don't start.
    static CANCEL: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) };
}

/// Lets the git runs of the current thread be stopped through `flag`.
pub fn set_thread_cancel(flag: Option<Arc<AtomicBool>>) {
    CANCEL.with(|c| *c.borrow_mut() = flag);
}

/// The stop flag of the current thread's git runs (for a helper thread doing part of the work).
pub fn thread_cancel() -> Option<Arc<AtomicBool>> {
    CANCEL.with(|c| c.borrow().clone())
}

fn cancel_flag() -> Option<Arc<AtomicBool>> {
    thread_cancel()
}

/// Runs git and waits. Used for reading repository data.
pub fn run(cwd: &Path, args: &[&str]) -> Out {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    run_owned(Some(cwd), &args, None)
}

pub fn run_owned(cwd: Option<&Path>, args: &[String], stdin: Option<&str>) -> Out {
    let t0 = Instant::now();
    let out = run_owned_inner(cwd, args, stdin);
    crate::app::perf::took(&format!("git {}", args.join(" ")), t0);
    out
}

fn run_owned_inner(cwd: Option<&Path>, args: &[String], stdin: Option<&str>) -> Out {
    let mut c = git_command(cwd, args);
    // Reading must not write: without this `git status` refreshes and rewrites .git/index,
    // which the folder watch reports as a change (and other Git tools react to, in turn).
    // Commands that need a lock still take it; only optional ones are skipped.
    c.env("GIT_OPTIONAL_LOCKS", "0");
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let cancel = cancel_flag();
    if cancel.as_ref().is_some_and(|f| f.load(Ordering::Relaxed)) {
        return Out::failed("cancelled".into());
    }
    let mut child = match c.spawn() {
        Ok(child) => child,
        Err(e) => return Out::failed(format!("Git konnte nicht gestartet werden: {e}")),
    };
    if let (Some(s), Some(mut si)) = (stdin, child.stdin.take()) {
        let s = s.to_string();
        std::thread::spawn(move || {
            let _ = si.write_all(s.as_bytes());
        });
    }
    let (code, out, err) = match cancel {
        None => match child.wait_with_output() {
            Ok(o) => (o.status.code().unwrap_or(-1), o.stdout, o.stderr),
            Err(e) => return Out::failed(e.to_string()),
        },
        // A background run that may be stopped: read the pipes in threads and look at the flag
        // while waiting (a replaced load's git must not keep running, e.g. a long `log -G`).
        Some(flag) => {
            let tree = ProcessTree::new(&child);
            let read = |r: Option<Box<dyn Read + Send>>| {
                std::thread::spawn(move || {
                    let mut v = Vec::new();
                    if let Some(mut r) = r {
                        let _ = r.read_to_end(&mut v);
                    }
                    v
                })
            };
            let out = read(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
            let err = read(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
            let code = loop {
                if flag.load(Ordering::Relaxed) {
                    tree.kill(&mut child);
                    let _ = child.wait();
                    break -1;
                }
                match child.try_wait() {
                    Ok(Some(st)) => break st.code().unwrap_or(-1),
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(15)),
                    Err(_) => break -1,
                }
            };
            let stop = || flag.load(Ordering::Relaxed);
            // (`child` has ended; what may still run is in the job.)
            let end = || {
                #[cfg(windows)]
                if !tree.job.is_null() {
                    // SAFETY: the job object is still open (dropped after this).
                    unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(tree.job, 1) };
                }
            };
            let out = join_reader(out, stop, end);
            let err = join_reader(err, stop, end);
            let code = if flag.load(Ordering::Relaxed) { -1 } else { code };
            (code, out, err)
        }
    };
    let (stdout, lossy) = match String::from_utf8(out) {
        Ok(s) => (s, false),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), true),
    };
    Out { code, stdout, stderr: String::from_utf8_lossy(&err).into_owned(), lossy }
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
    /// The hunk buttons of its repository wait for it (set by the app, see `blocks_hunks`).
    pub blocks_hunks: bool,
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
        blocks_hunks: false,
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
    let display = spec.display.clone();
    std::thread::spawn(move || {
        let t0 = Instant::now();
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
        // Cancelling stops everything the command started (see `ProcessTree`).
        let tree = ProcessTree::new(&child);
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
        let mut stopped = false;
        let code = loop {
            if cancel.load(Ordering::Relaxed) && !stopped {
                stopped = true;
                tree.kill_command(&mut child);
                output.lock().unwrap().push_str("\n[Abgebrochen]\n");
            }
            match child.try_wait() {
                Ok(Some(st)) => break st.code().unwrap_or(-1),
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(40)),
                Err(_) => break -1,
            }
        };
        // A process the command started may still hold its output open; Cancel still works then.
        for r in readers {
            join_reader(r, || cancel.load(Ordering::Relaxed) && !stopped, || {
                #[cfg(windows)]
                if !tree.job.is_null() {
                    // SAFETY: the job object is still open (dropped after this).
                    unsafe { kill_job_except_browsers(tree.job) };
                }
                output.lock().unwrap().push_str("\n[Abgebrochen]\n");
            });
        }
        crate::app::perf::took(&format!("job (exit {code}): {}", display.lines().next().unwrap_or("")), t0);
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

/// Which tips the history starts from: the reference in `only` if set, otherwise every
/// reference when `all` is set (else HEAD); `reflog` adds the commits the reflogs mention.
#[derive(Clone, Default)]
pub struct LogOptions {
    pub all: bool,
    pub hide_stashes: bool,
    pub reflog: bool,
    /// View > Collapse All Merges (Show First Parent)
    pub first_parent: bool,
    /// Only the history of this reference (View > Filter by Active Branch uses "HEAD";
    /// the funnel button on a branch in the sidebar uses the branch).
    pub only: Option<String>,
    /// Branches hidden from the history (eye button in the sidebar).
    pub hidden: Vec<String>,
    /// Labels only: hide tags in the commit list.
    pub hide_tags: bool,
    /// Preferences > Sort commits: topologically instead of by date.
    pub topo: bool,
    /// Every stash (not only the newest, which refs/stash points at) as an extra tip.
    pub stash_tips: Vec<String>,
}

pub fn log_tip_args(o: &LogOptions) -> Vec<String> {
    let mut v = Vec::new();
    if o.first_parent {
        v.push("--first-parent".into());
    }
    if let Some(r) = &o.only {
        v.push(r.clone());
    } else if o.all {
        for p in HIDDEN_REF_PREFIXES {
            v.push(format!("--exclude={p}*"));
        }
        if o.hide_stashes {
            v.push("--exclude=refs/stash".into());
        }
        for h in &o.hidden {
            v.push(format!("--exclude=refs/heads/{h}"));
            v.push(format!("--exclude=refs/remotes/{h}"));
        }
        v.push("--all".into());
        if !o.hide_stashes {
            v.extend(o.stash_tips.iter().cloned());
        }
    }
    if o.reflog {
        v.push("--reflog".into());
    }
    v
}

pub fn load_commits(dir: &Path, limit: usize, opts: &LogOptions) -> Vec<Commit> {
    let fmt = "--format=%H%x1f%h%x1f%P%x1f%an%x1f%ae%x1f%ad%x1f%D%x1f%s%x1e";
    let n = format!("-n{limit}");
    let tips = log_tip_args(opts);
    let order = if opts.topo { "--topo-order" } else { "--date-order" };
    let mut args = vec!["log", order, fmt, "--date=format:%d.%m.%Y %H:%M", n.as_str()];
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

/// Each stash: (stash@{n}, commit, its parents). The 2nd and 3rd parents are Git's helper
/// commits for the index and untracked files.
pub fn stash_commits(dir: &Path) -> Vec<(String, String, Vec<String>)> {
    run(dir, &["stash", "list", "--format=%gd%x1f%H%x1f%P"])
        .stdout
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\x1f').collect();
            (f.len() >= 3).then(|| (f[0].to_string(), f[1].to_string(), f[2].split_whitespace().map(String::from).collect()))
        })
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
    parse_status(&run(dir, &["status", "--porcelain=v1", "-z", "-uall"]).stdout)
}

/// `git status --porcelain=v1 -z`: "XY path\0", and for a rename or copy on either side
/// (X or Y is R/C, e.g. " R" for a renamed intent-to-add file) the old path as the next field.
fn parse_status(text: &str) -> Vec<FileStatus> {
    let mut v = Vec::new();
    let mut parts = text.split('\0');
    while let Some(p) = parts.next() {
        let b = p.as_bytes();
        if b.len() < 4 || b[2] != b' ' || !b[0].is_ascii() || !b[1].is_ascii() {
            continue;
        }
        let (x, y) = (b[0] as char, b[1] as char);
        let path = p[3..].to_string();
        let orig = if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') { parts.next().map(String::from) } else { None };
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

/// What special operation the repository (its .git folder `g`) is in the middle of, if any.
pub fn repo_state_at(g: &Path) -> Option<&'static str> {
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
/// `g` is the .git folder (no git call).
pub fn repo_locked_at(g: &Path) -> Option<&'static str> {
    ["index.lock", "HEAD.lock"].into_iter().find(|f| g.join(f).exists())
}

/// Git is still writing one of the watched files (its lock file exists): the folder watch waits
/// for the next look instead of reading half-done state.
pub fn repo_busy_at(g: &Path, common: &Path) -> bool {
    WATCHED.iter().any(|(f, _)| f.ends_with(".lock") && watched_dir(f, g, common).join(f).exists())
}

/// The folder shared by all worktrees of a repository (`git rev-parse --git-common-dir`); for a
/// linked worktree its refs, packed-refs and config are there, not in its own .git folder.
pub fn common_dir(dir: &Path) -> Option<PathBuf> {
    let o = run(dir, &["rev-parse", "--git-common-dir"]);
    if !o.ok() {
        return None;
    }
    let p = PathBuf::from(o.stdout.trim());
    Some(if p.is_absolute() { p } else { dir.join(p) })
}

/// Where a watched file lives: the shared files in the common folder, the others (HEAD, the
/// index, merge and rebase state) in the worktree's own .git folder.
fn watched_dir<'a>(f: &str, git_dir: &'a Path, common: &'a Path) -> &'a Path {
    let shared = matches!(f, "config" | "config.lock" | "packed-refs" | "packed-refs.lock" | "reftable" | "info/exclude" | "info/exclude.lock");
    if shared { common } else { git_dir }
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

/// `common` is the repository's common folder (`common_dir`; the same as `git_dir` except in
/// a linked worktree).
pub fn snapshot(git_dir: &Path, common: &Path) -> Snapshot {
    let mut v: Snapshot = WATCHED
        .iter()
        .map(|(f, flag)| {
            let m = std::fs::metadata(watched_dir(f, git_dir, common).join(f)).ok();
            (*flag, m.and_then(|m| m.modified().ok().map(|t| (m.len(), t))))
        })
        .collect();
    // Loose branch/tag files under refs/: newest change counts as a refs change.
    v.push((RELOAD_REFS, newest_in(&common.join("refs"), 4)));
    // A linked worktree with reftable keeps its HEAD in its own reftable folder.
    v.push((RELOAD_HEAD, newest_in(&git_dir.join("reftable"), 0)));
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

/// The snapshot after a load that read only what `flags` cover: `new` for those files, `old`
/// for the rest, so a change there is still found by the next look (a load of the status alone
/// must not swallow a commit made at the same time).
pub fn merge_snapshot(old: &Snapshot, new: Snapshot, flags: u32) -> Snapshot {
    if old.len() != new.len() {
        return new;
    }
    new.into_iter().zip(old.iter()).map(|(n, o)| if n.0 & flags != 0 { n } else { *o }).collect()
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
    /// Renamed or copied files: new path -> old path (the diff needs both to show a rename).
    pub renamed_from: std::collections::HashMap<String, String>,
}

pub fn commit_detail(dir: &Path, sha: &str) -> CommitDetail {
    let o = run(dir, &["show", "-s", "--format=%H%x1f%an%x1f%ae%x1f%ad%x1f%cn%x1f%P%x1f%D%x1f%B", "--date=format:%d.%m.%Y %H:%M:%S %z", sha]);
    let f: Vec<&str> = o.stdout.splitn(8, '\x1f').collect();
    let g = |i: usize| f.get(i).map(|s| s.to_string()).unwrap_or_default();
    let parents: Vec<String> = g(5).split_whitespace().map(String::from).collect();
    // Merges and stashes: what changed against the first parent (plain diff-tree shows nothing
    // for them). A stash's third parent holds its new, untracked files.
    let files_out = if parents.len() >= 2 {
        run(dir, &["diff-tree", "--no-commit-id", "-r", "--name-status", "-M", &parents[0], sha])
    } else {
        run(dir, &["diff-tree", "--no-commit-id", "-r", "--name-status", "-M", "--root", sha])
    };
    let mut renamed_from = std::collections::HashMap::new();
    let mut files: Vec<(String, String)> = files_out
        .stdout
        .lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split('\t').collect();
            let st = parts.first()?.to_string();
            let path = parts.last()?.to_string();
            if parts.len() == 3 {
                renamed_from.insert(path.clone(), parts[1].to_string());
            }
            Some((st.chars().next().unwrap_or('M').to_string(), path))
        })
        .collect();
    if parents.len() == 3 {
        for p in run(dir, &["ls-tree", "-r", "--name-only", &parents[2]]).stdout.lines() {
            if !files.iter().any(|f| f.1 == p) {
                files.push(("A".into(), p.to_string()));
            }
        }
    }
    CommitDetail {
        sha: g(0),
        author: g(1),
        email: g(2),
        date: g(3),
        committer: g(4),
        parents,
        refs: g(6),
        message: g(7).trim_end().to_string(),
        files,
        renamed_from,
    }
}

/// How diffs are fetched for showing (the diff toolbar).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DiffOpts {
    /// Unchanged lines around each change (`-U`).
    pub context: u32,
    /// Ignore changes in whitespace (`-w`).
    pub ignore_ws: bool,
    /// The whole file instead of hunks.
    pub full: bool,
}

impl Default for DiffOpts {
    fn default() -> Self {
        DiffOpts { context: 3, ignore_ws: false, full: false }
    }
}

impl DiffOpts {
    /// Fixed format whatever the user's config says (colour, external diff tool, path prefixes
    /// and context size would otherwise break showing or staging hunks), then the options.
    pub fn args(&self) -> Vec<String> {
        let mut v: Vec<String> = vec!["--no-color".into(), "--no-ext-diff".into(), "--src-prefix=a/".into(), "--dst-prefix=b/".into()];
        v.push(format!("-U{}", if self.full { 1_000_000 } else { self.context }));
        if self.ignore_ws {
            v.push("-w".into());
        }
        v
    }
}

/// The diff of one file in a commit; merges and stashes against their first parent.
/// `orig` is the old path of a renamed file (both paths are needed to show the rename).
pub fn commit_file_diff(dir: &Path, sha: &str, parents: &[String], path: &str, orig: Option<&str>, o: &DiffOpts) -> String {
    let mut paths: Vec<String> = orig.map(|p| vec![p.to_string()]).unwrap_or_default();
    paths.push(path.to_string());
    let with = |head: &[&str], tail: &[String]| -> String {
        let mut a: Vec<String> = vec!["--literal-pathspecs".into()];
        a.extend(head.iter().map(|s| s.to_string()));
        a.extend(o.args());
        a.extend(tail.iter().cloned());
        run_owned(Some(dir), &a, None).stdout
    };
    if parents.len() >= 2 {
        let mut tail = vec!["-M".to_string(), parents[0].clone(), sha.to_string(), "--".into()];
        tail.extend(paths.iter().cloned());
        let d = with(&["diff"], &tail);
        if d.trim().is_empty() && parents.len() == 3 {
            return with(&["show", "--format="], &[parents[2].clone(), "--".into(), path.to_string()]);
        }
        return d;
    }
    let mut tail = vec!["-M".to_string(), sha.to_string(), "--".into()];
    tail.extend(paths);
    with(&["show", "--format="], &tail)
}

pub fn commit_tree(dir: &Path, sha: &str) -> Vec<String> {
    run(dir, &["ls-tree", "-r", "--name-only", sha]).stdout.lines().map(String::from).collect()
}

/// core.autocrlf and core.eol as git sees them (the last setting wins), "" when unset;
/// booleans as "true"/"false".
pub fn eol_config(dir: &Path) -> (String, String) {
    let o = run(dir, &["config", "--get-regexp", r"^core\.(autocrlf|eol)$"]);
    let (mut autocrlf, mut eol) = (String::new(), String::new());
    for l in o.stdout.lines() {
        // "key value", "key " (an empty value) or "key" (no value at all).
        let (k, v) = match l.split_once(' ') {
            Some((k, v)) => (k, Some(v.trim().to_ascii_lowercase())),
            None => (l, None),
        };
        match k.to_ascii_lowercase().as_str() {
            // A boolean the git way: a key without a value is true, an empty value false,
            // numbers true unless 0.
            "core.autocrlf" => {
                autocrlf = match v.as_deref() {
                    None | Some("yes" | "on" | "true") => "true".to_string(),
                    Some("" | "no" | "off" | "false") => "false".to_string(),
                    Some(n) if n.parse::<i64>().is_ok() => if n.parse::<i64>() == Ok(0) { "false".to_string() } else { "true".to_string() },
                    Some(other) => other.to_string(),
                }
            }
            "core.eol" => eol = v.unwrap_or_default(),
            _ => {}
        }
    }
    (autocrlf, eol)
}

/// Would Discard Hunk (`git apply --reverse` on the work tree) change the file outside the
/// hunk? Besides line ends (`discard_changes_line_ends`, with the line-end settings read now:
/// they can change in any config file), git apply re-encodes a file with a
/// working-tree-encoding, rewrites `$Id$` with ident and runs a filter's smudge; such files are
/// left to Discard whole file. So are files whose attributes are in doubt: a .gitattributes
/// deleted from the work tree is still read from the index by `check-attr`, but not by git
/// apply.
pub fn discard_changes_file(dir: &Path, path: &str) -> bool {
    let deleted = run(dir, &["ls-files", "--deleted", "--", ":(glob)**/.gitattributes"]).stdout;
    if !deleted.trim().is_empty() {
        return true;
    }
    let attrs = run(dir, &["--literal-pathspecs", "check-attr", "working-tree-encoding", "ident", "filter", "--", path]).stdout;
    let converted = attrs.lines().any(|l| {
        let v = l.rsplit(": ").next().unwrap_or("").trim();
        !matches!(v, "unspecified" | "unset" | "")
    });
    if converted {
        return true;
    }
    let eol = run(dir, &["--literal-pathspecs", "ls-files", "--eol", "--", path]).stdout;
    discard_changes_line_ends(&eol, &eol_config(dir))
}

/// Would `git apply --reverse` (Discard Hunk) rewrite line ends outside the hunk? git apply
/// reads the work-tree file with line ends normalised and writes it back through the
/// checkout conversion, so the file only stays as it was when it already is what a checkout
/// writes: `eol` is `git ls-files --eol` of the file (index, work tree, attributes), `cfg`
/// from `eol_config`.
pub fn discard_changes_line_ends(eol: &str, cfg: &(String, String)) -> bool {
    let (head, _) = eol.split_once('\t').unwrap_or((eol, ""));
    let (left, attr) = head.split_once("attr/").unwrap_or((head, ""));
    let mut index = "";
    let mut work = "";
    for t in left.split_whitespace() {
        if let Some(v) = t.strip_prefix("i/") {
            index = v;
        } else if let Some(v) = t.strip_prefix("w/") {
            work = v;
        }
    }
    let attrs: Vec<&str> = attr.split_whitespace().collect();
    if attrs.iter().any(|a| *a == "-text" || *a == "binary") {
        return false; // no conversion
    }
    #[derive(PartialEq)]
    enum Conv {
        None,
        Crlf,
        Lf,
    }
    let eol_attr = if attrs.contains(&"eol=crlf") {
        Some(Conv::Crlf)
    } else if attrs.contains(&"eol=lf") {
        Some(Conv::Lf)
    } else {
        None
    };
    let text_attr = attrs.iter().any(|a| *a == "text" || *a == "text=auto") || eol_attr.is_some();
    let (autocrlf, core_eol) = (cfg.0.as_str(), cfg.1.as_str());
    let conv = if text_attr {
        eol_attr.unwrap_or(match (autocrlf, core_eol) {
            ("true", _) => Conv::Crlf,
            ("input", _) => Conv::Lf,
            (_, "crlf") => Conv::Crlf,
            (_, "lf") => Conv::Lf,
            _ if cfg!(windows) => Conv::Crlf,
            _ => Conv::Lf,
        })
    } else {
        match autocrlf {
            "true" => Conv::Crlf,
            // Converted only when committing, but git apply reads the file normalised too.
            "input" => Conv::Lf,
            _ => Conv::None,
        }
    };
    let none = |s: &str| s.is_empty() || s == "none";
    // A text index with a work-tree file git doesn't see as text: an encoding conversion.
    if work == "-text" && index != "-text" && !index.is_empty() {
        return true;
    }
    // CR LF in the index and in the file: the hunk's lines carry the CR, and git apply keeps
    // the file's CR LF then.
    if index == "crlf" && work == "crlf" {
        return false;
    }
    match conv {
        Conv::None => false,
        Conv::Crlf => !((index == "lf" || none(index)) && (work == "crlf" || none(work))),
        Conv::Lf => !((index == "lf" || none(index)) && (work == "lf" || none(work))),
    }
}

/// The diff of a local change (the Changes view), whose hunks can be staged, unstaged or
/// discarded: `lossy` in the result says the text had bytes that aren't UTF-8.
pub fn working_diff(dir: &Path, f: &FileStatus, staged: bool, o: &DiffOpts) -> Out {
    // Showing a diff must not refresh (rewrite) the index either; GIT_OPTIONAL_LOCKS does not
    // cover `git diff`, this setting does. No text conversion (diff.<driver>.textconv, e.g.
    // Git for Windows' astextplain for PDF and Office files): its text is not what is in the
    // file, so a hunk of it would write converted text into the index or the file.
    // A submodule shows as "Subproject commit …" whatever diff.submodule says (its inner diff
    // is not a hunk of this repository).
    // (Paths are taken literally: "notes[1].md" is not a pattern that also matches notes1.md.)
    let mut a: Vec<String> = vec!["--literal-pathspecs".into(), "-c".into(), "diff.autoRefreshIndex=false".into(), "diff".into(), "--no-textconv".into(), "--submodule=short".into()];
    a.extend(o.args());
    if f.index == '?' && !staged {
        a.extend(["--no-index".to_string(), "--".into(), "/dev/null".into(), f.path.clone()]);
        return run_owned(Some(dir), &a, None);
    }
    if staged {
        a.extend(["--cached".to_string(), "-M".into()]);
    }
    a.push("--".into());
    // A staged rename needs both paths to show as one. (Not a copy: its source still exists
    // and would bring its own changes into this diff.)
    if staged && f.index == 'R' {
        if let Some(orig) = &f.orig {
            a.push(orig.clone());
        }
    }
    a.push(f.path.clone());
    run_owned(Some(dir), &a, None)
}

pub fn file_log_entries(dir: &Path, path: &str) -> String {
    run(dir, &["log", "--follow", "--format=%h  %ad  %an  %s", "--date=format:%d.%m.%Y", "--", path]).stdout
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_entries_with_renames_on_either_side() {
        // A staged rename, a renamed intent-to-add file (" R"), a non-ASCII old name and an
        // ordinary change: the old path of a rename is a field of its own.
        let s = parse_status("R  neu.txt\0alt.txt\0 R b.txt\0Größe.txt\0 M c.txt\0?? d e.txt\0");
        let got: Vec<(char, char, &str, Option<&str>)> = s.iter().map(|f| (f.index, f.worktree, f.path.as_str(), f.orig.as_deref())).collect();
        assert_eq!(got, vec![('R', ' ', "neu.txt", Some("alt.txt")), (' ', 'R', "b.txt", Some("Größe.txt")), (' ', 'M', "c.txt", None), ('?', '?', "d e.txt", None)]);
        assert!(parse_status("").is_empty());
        assert!(parse_status("xx\0").is_empty());
    }

    #[test]
    fn a_partial_load_keeps_the_other_parts_of_the_snapshot() {
        let t = |s: u64| Some((s, std::time::SystemTime::UNIX_EPOCH));
        let old: Snapshot = vec![(RELOAD_REFS, t(1)), (RELOAD_INDEX, t(1))];
        let new: Snapshot = vec![(RELOAD_REFS, t(2)), (RELOAD_INDEX, t(2))];
        let merged = merge_snapshot(&old, new.clone(), RELOAD_INDEX);
        assert_eq!(merged, vec![(RELOAD_REFS, t(1)), (RELOAD_INDEX, t(2))]);
        assert_eq!(changed_flags(&merged, &new), RELOAD_REFS);
        assert_eq!(merge_snapshot(&old, new.clone(), u32::MAX), new);
        assert_eq!(merge_snapshot(&vec![], new.clone(), RELOAD_INDEX), new);
    }

    #[test]
    fn line_end_rules_for_discarding() {
        let cfg = |a: &str, e: &str| (a.to_string(), e.to_string());
        let risky = |eol: &str, c: &(String, String)| discard_changes_line_ends(eol, c);
        assert!(!risky("i/lf    w/lf    attr/                 \tf", &cfg("", "")));
        assert!(!risky("i/lf    w/crlf  attr/                 \tf", &cfg("true", "")));
        assert!(risky("i/lf    w/lf    attr/                 \tf", &cfg("true", "")));
        assert!(risky("i/mixed w/mixed attr/                 \tf", &cfg("true", "")));
        assert!(risky("i/lf    w/crlf  attr/                 \tf", &cfg("input", "")));
        assert!(!risky("i/lf    w/lf    attr/                 \tf", &cfg("input", "")));
        assert!(!risky("i/lf    w/lf    attr/text eol=lf      \tf", &cfg("true", "")));
        assert!(!risky("i/lf    w/mixed attr/-text            \tf", &cfg("true", "")));
        assert!(!risky("", &cfg("true", "")));
    }

    /// Stopping a run ends everything it started: on Windows `git` is usually a launcher that
    /// runs the real git, which here runs a shell that sleeps. If only the launcher were killed,
    /// the others would hold the pipes open and the run would wait for the sleep to end.
    #[cfg(windows)]
    #[test]
    fn a_stopped_run_ends_its_whole_process_tree() {
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        let t0 = Instant::now();
        let h = std::thread::spawn(move || {
            set_thread_cancel(Some(f));
            run(Path::new("."), &["-c", "alias.nap=!sleep 20", "nap"])
        });
        std::thread::sleep(std::time::Duration::from_millis(800));
        flag.store(true, Ordering::Relaxed);
        let out = h.join().unwrap();
        assert!(!out.ok());
        assert!(t0.elapsed() < std::time::Duration::from_secs(8), "took {:?}", t0.elapsed());
    }

    #[test]
    fn a_stopped_background_run_ends_early() {
        let flag = Arc::new(AtomicBool::new(true));
        let f = flag.clone();
        let out = std::thread::spawn(move || {
            set_thread_cancel(Some(f));
            run(Path::new("."), &["--version"])
        })
        .join()
        .unwrap();
        assert!(!out.ok() && out.stdout.is_empty());
        // Not stopped: it runs as usual and says whether the output was UTF-8.
        let out = std::thread::spawn(|| {
            set_thread_cancel(Some(Arc::new(AtomicBool::new(false))));
            run(Path::new("."), &["--version"])
        })
        .join()
        .unwrap();
        assert!(out.ok() && out.stdout.starts_with("git version") && !out.lossy);
    }
}
