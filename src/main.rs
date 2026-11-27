#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod appicon;
mod catalog;
mod catalog_en;
mod git;
mod graph;
mod i18n;
mod icons;
mod installer;
mod theme;
mod updater;
mod version;
mod winicon;

use std::path::PathBuf;

/// Lets a GUI program print to the console it was started from (`yagui --version`).
fn attach_console() {
    #[cfg(windows)]
    // SAFETY: plain Win32 call.
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
}

/// Apps started from Finder or the Dock get only `/usr/bin:/bin:/usr/sbin:/sbin`, so Git from
/// Homebrew or MacPorts (and git-lfs, git-flow) would not be found. Adds their folders that exist
/// and are missing, right before the system folders, as a login shell has them; folders the user
/// put first stay first.
#[cfg(target_os = "macos")]
fn extend_path() {
    let current: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(dirs) = extended_path(current, |d| d.is_dir()) {
        if let Ok(joined) = std::env::join_paths(dirs) {
            // SAFETY: called first thing in main, before any other thread exists.
            unsafe { std::env::set_var("PATH", joined) };
        }
    }
}

/// PATH's folders with the Homebrew and MacPorts folders that exist added; None if none is
/// missing (then PATH stays as it is, also an unset one).
#[cfg(any(target_os = "macos", test))]
fn extended_path(current: Vec<PathBuf>, is_dir: impl Fn(&std::path::Path) -> bool) -> Option<Vec<PathBuf>> {
    const SYSTEM: [&str; 4] = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];
    // Empty entries would mean the current folder; no PATH at all means launchd's.
    let mut dirs: Vec<PathBuf> = current.into_iter().filter(|d| !d.as_os_str().is_empty()).collect();
    if dirs.is_empty() {
        dirs = SYSTEM.iter().map(PathBuf::from).collect();
    }
    let extra: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]
        .iter()
        .map(PathBuf::from)
        .filter(|d| is_dir(d) && !dirs.contains(d))
        .collect();
    if extra.is_empty() {
        return None;
    }
    let at = dirs.iter().position(|d| SYSTEM.iter().any(|s| d.as_path() == std::path::Path::new(s))).unwrap_or(0);
    dirs.splice(at..at, extra);
    Some(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn homebrew_and_macports_folders_go_right_before_the_system_folders() {
        let all = |_: &std::path::Path| true;
        let brew = |d: &std::path::Path| d == std::path::Path::new("/opt/homebrew/bin");
        // Finder's PATH.
        assert_eq!(extended_path(paths(&["/usr/bin", "/bin", "/usr/sbin", "/sbin"]), brew), Some(paths(&["/opt/homebrew/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"])));
        // The user's own folders stay first; folders already there are not added twice.
        assert_eq!(
            extended_path(paths(&["/Users/me/bin", "/usr/local/bin", "/usr/bin", "/bin"]), all),
            Some(paths(&["/Users/me/bin", "/usr/local/bin", "/opt/homebrew/bin", "/opt/local/bin", "/usr/bin", "/bin"]))
        );
        assert_eq!(extended_path(paths(&["/opt/homebrew/bin", "/usr/bin"]), brew), None);
        // No PATH, or an empty one: launchd's folders, without an empty entry.
        assert_eq!(extended_path(vec![], brew), Some(paths(&["/opt/homebrew/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"])));
        assert_eq!(extended_path(paths(&[""]), |_: &std::path::Path| false), None);
    }
}

const HELP: &str = "YAGUI - Yet Another Git User Interface

Usage: yagui [folder] [options]

  [folder] / --repo <folder>   open this repository
  --version                    print the version
  --check-update               ask GitHub for the newest release
  --install [--dir <folder>] [--quiet] [--desktop] [--no-shortcuts] [--no-register]
                               install for the current user (--quiet: without a window)
  --uninstall [--dir <folder>] [--quiet] [--remove-settings] [--no-shortcuts] [--no-register]
                               remove YAGUI again (only its own files)
  --screenshot <file.png>      write a picture of the window and exit
  --write-icon <file.png> <size>
                               write the program icon (used when packaging)
";

fn main() -> eframe::Result<()> {
    #[cfg(target_os = "macos")]
    extend_path();
    let mut screenshot: Option<PathBuf> = None;
    let mut repo: Option<PathBuf> = None;
    let all: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| all.iter().any(|a| a == f);
    let value = |f: &str| all.iter().position(|a| a == f).and_then(|i| all.get(i + 1)).cloned();
    if has("--version") || has("-V") {
        attach_console();
        println!("{}", version::long());
        return Ok(());
    }
    if has("--check-update") {
        attach_console();
        match updater::latest() {
            Ok(Some(r)) => println!("latest release: {} ({}){}", r.tag, r.published, if version::is_newer(&r.tag) { " - newer than this build" } else { " - this build is up to date" }),
            Ok(None) => println!("no release published yet"),
            Err(e) => {
                eprintln!("update check failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if has("--help") || has("-h") {
        attach_console();
        print!("{HELP}");
        return Ok(());
    }
    if let Some(f) = value("--write-icon") {
        let size: u32 = all.iter().position(|a| a == "--write-icon").and_then(|i| all.get(i + 2)).and_then(|s| s.parse().ok()).unwrap_or(256);
        if let Some(img) = image::RgbaImage::from_raw(size, size, appicon::icon_pixels(size)) {
            let _ = img.save(&f);
        }
        return Ok(());
    }
    let quiet = has("--quiet");
    if has("--install") {
        let mut o = installer::Options::default();
        if let Some(d) = value("--dir") {
            o.dir = PathBuf::from(d);
        }
        o.desktop = has("--desktop");
        if has("--no-shortcuts") {
            o.start_menu = false;
            o.context_menu = false;
        }
        if has("--no-register") {
            o.register = false;
        }
        if quiet {
            attach_console();
            match installer::install(&o) {
                Ok(p) => println!("installed: {}", p.display()),
                Err(e) => {
                    eprintln!("install failed: {e}");
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
        let _ = app::updates::START_MODE.set(app::updates::StartMode::Install);
    } else if has("--uninstall") {
        if quiet {
            attach_console();
            let dir = value("--dir").map(PathBuf::from).unwrap_or_else(installer::default_dir);
            let u = installer::UninstallOptions { remove_settings: has("--remove-settings"), register: !has("--no-register"), shortcuts: !has("--no-shortcuts") };
            match installer::uninstall(&dir, &u) {
                Ok(()) => println!("uninstalled"),
                Err(e) => {
                    eprintln!("uninstall failed: {e}");
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
        let _ = app::updates::START_MODE.set(app::updates::StartMode::Uninstall);
        // Settings > Apps passes the folder YAGUI was installed to.
        if let Some(d) = value("--dir") {
            let _ = app::updates::UNINSTALL_DIR.set(PathBuf::from(d));
        }
    } else if installer::is_setup_name() && !installer::running_installed() {
        // Started as "YAGUI-Setup-….exe": offer to install.
        let _ = app::updates::START_MODE.set(app::updates::StartMode::Install);
    }
    let mut args = all.iter().cloned();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--repo" => repo = args.next().map(PathBuf::from),
            "--dir" => {
                args.next();
            }
            other if !other.starts_with("--") && repo.is_none() => repo = Some(PathBuf::from(other)),
            _ => {}
        }
    }

    winicon::set_app_id();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("YAGUI")
        .with_app_id("YAGUI.GitClient")
        .with_icon(std::sync::Arc::new(eframe::egui::IconData { rgba: appicon::icon_pixels(64), width: 64, height: 64 }))
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 560.0]);
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(eframe::egui::WindowLevel::AlwaysOnBottom);
        }
    }

    let options = eframe::NativeOptions { viewport, ..Default::default() };

    eframe::run_native(
        "YAGUI",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, repo, screenshot)))),
    )
}
