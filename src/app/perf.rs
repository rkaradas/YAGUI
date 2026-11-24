//! Optional timing log for tracking down stutter on slow computers.
//!
//! Start YAGUI with `YAGUI_PERF_LOG=<file>` and it appends one line per git call made for the
//! views and per slow frame: time since start, the thread (`ui` = the window waited for it,
//! `bg` = background), duration and what ran. Without the variable nothing is measured or
//! written.

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Frames slower than this are logged.
pub const SLOW_FRAME_MS: f64 = 30.0;

fn sink() -> Option<&'static Mutex<std::fs::File>> {
    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var_os("YAGUI_PERF_LOG")?;
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok().map(Mutex::new)
    })
    .as_ref()
}

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

static UI_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();

/// Remembers the window's thread (call once from it).
pub fn mark_ui_thread() {
    let _ = UI_THREAD.set(std::thread::current().id());
    start();
}

pub fn enabled() -> bool {
    sink().is_some()
}

/// Appends a line to the log (if it is on).
pub fn note(msg: &str) {
    let Some(f) = sink() else { return };
    let ui = UI_THREAD.get().map(|t| *t == std::thread::current().id()).unwrap_or(false);
    let t = start().elapsed().as_secs_f64() * 1000.0;
    if let Ok(mut f) = f.lock() {
        let _ = writeln!(f, "{t:>10.1} ms  [{}]  {msg}", if ui { "ui" } else { "bg" });
    }
}

/// Logs how long something took, if the log is on.
pub fn took(what: &str, since: Instant) {
    if enabled() {
        note(&format!("{:>8.1} ms  {what}", since.elapsed().as_secs_f64() * 1000.0));
    }
}
