//! Installing and uninstalling YAGUI for the current user, without administrator rights and
//! without extra tools. The same program is the setup: started as `YAGUI-Setup-….exe` (or with
//! `--install`) it offers to install itself.
//!
//! Windows: `%LOCALAPPDATA%\Programs\YAGUI\yagui.exe`, Start menu and (optional) desktop
//! shortcut, "Open in YAGUI" on folders in Explorer, and an entry under Settings > Apps whose
//! "Uninstall" runs `yagui.exe --uninstall`.
//! Linux: `~/.local/share/yagui/yagui`, `~/.local/bin/yagui`, a desktop entry and icon.
//! macOS: the app bundle is dragged to Applications (from the .dmg); uninstalling means moving
//! it to the bin, so only settings are removed here.

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use crate::version;

#[derive(Clone)]
pub struct Options {
    pub dir: PathBuf,
    pub start_menu: bool,
    pub desktop: bool,
    pub context_menu: bool,
    /// Register with Windows (Settings > Apps) – off only for tests.
    pub register: bool,
}

fn env_dir(k: &str) -> Option<PathBuf> {
    std::env::var_os(k).map(PathBuf::from)
}

pub fn default_dir() -> PathBuf {
    if cfg!(windows) {
        env_dir("LOCALAPPDATA").unwrap_or_default().join("Programs").join("YAGUI")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Applications")
    } else {
        env_dir("HOME").unwrap_or_default().join(".local/share/yagui")
    }
}

impl Default for Options {
    fn default() -> Self {
        Options { dir: default_dir(), start_menu: true, desktop: false, context_menu: true, register: true }
    }
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "yagui.exe" } else { "yagui" }
}

/// Where the installed program is (whether or not it exists).
pub fn installed_exe(dir: &Path) -> PathBuf {
    dir.join(exe_name())
}

/// True when this program runs from the install folder.
pub fn running_installed() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let exe = exe.canonicalize().unwrap_or(exe);
    let inst = installed_exe(&default_dir());
    let inst = inst.canonicalize().unwrap_or(inst);
    exe == inst
}

pub fn is_installed() -> bool {
    installed_exe(&default_dir()).exists()
}

/// Started as a setup program (its file name says so)?
pub fn is_setup_name() -> bool {
    std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase())).map(|n| n.contains("setup")).unwrap_or(false)
}

fn run_hidden(program: &str, args: &[&str]) -> Result<(), String> {
    let mut c = Command::new(program);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let st = c.status().map_err(|e| format!("{program}: {e}"))?;
    if st.success() { Ok(()) } else { Err(format!("{program} {} failed", args.first().unwrap_or(&""))) }
}

#[cfg(windows)]
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\YAGUI";

#[cfg(windows)]
fn shortcut(lnk: &Path, target: &Path) -> Result<(), String> {
    let ps = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath='{}'; $s.WorkingDirectory='{}'; $s.IconLocation='{},0'; $s.Description='YAGUI - Yet Another Git User Interface'; $s.Save()",
        lnk.display().to_string().replace('\'', "''"),
        target.display().to_string().replace('\'', "''"),
        target.parent().map(|p| p.display().to_string()).unwrap_or_default().replace('\'', "''"),
        target.display().to_string().replace('\'', "''"),
    );
    run_hidden("powershell", &["-NoProfile", "-NonInteractive", "-Command", &ps])
}

#[cfg(windows)]
fn start_menu_lnk() -> PathBuf {
    env_dir("APPDATA").unwrap_or_default().join(r"Microsoft\Windows\Start Menu\Programs\YAGUI.lnk")
}

#[cfg(windows)]
fn desktop_lnk() -> PathBuf {
    env_dir("USERPROFILE").unwrap_or_default().join("Desktop").join("YAGUI.lnk")
}

/// Installs this program. Returns the installed program's path.
pub fn install(o: &Options) -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        return Err("On macOS, drag YAGUI.app from the disk image into Applications.".into());
    }
    std::fs::create_dir_all(&o.dir).map_err(|e| format!("{}: {e}", o.dir.display()))?;
    let target = installed_exe(&o.dir);
    let same = me.canonicalize().ok() == target.canonicalize().ok();
    if !same {
        if target.exists() {
            // replace a running/older copy the same way updates do
            let mut old = target.as_os_str().to_owned();
            old.push(".old");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&target, &old);
        }
        std::fs::copy(&me, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    #[cfg(windows)]
    {
        let t = target.display().to_string();
        if o.start_menu {
            shortcut(&start_menu_lnk(), &target)?;
        }
        if o.desktop {
            shortcut(&desktop_lnk(), &target)?;
        }
        if o.context_menu && o.register {
            let cmd_dir = format!("\"{t}\" \"%1\"");
            let cmd_bg = format!("\"{t}\" \"%V\"");
            for (key, cmd) in [(r"HKCU\Software\Classes\Directory\shell\YAGUI", cmd_dir), (r"HKCU\Software\Classes\Directory\Background\shell\YAGUI", cmd_bg)] {
                run_hidden("reg", &["add", key, "/ve", "/d", "Open in YAGUI", "/f"])?;
                run_hidden("reg", &["add", key, "/v", "Icon", "/d", &t, "/f"])?;
                run_hidden("reg", &["add", &format!(r"{key}\command"), "/ve", "/d", &cmd, "/f"])?;
            }
        }
        if o.register {
            let size_kb = std::fs::metadata(&target).map(|m| m.len() / 1024).unwrap_or(0).to_string();
            let uninst = format!("\"{t}\" --uninstall");
            let dir = o.dir.display().to_string();
            for (name, kind, val) in [
                ("DisplayName", "REG_SZ", "YAGUI"),
                ("DisplayVersion", "REG_SZ", version::VERSION),
                ("Publisher", "REG_SZ", "YAGUI"),
                ("DisplayIcon", "REG_SZ", t.as_str()),
                ("InstallLocation", "REG_SZ", dir.as_str()),
                ("UninstallString", "REG_SZ", uninst.as_str()),
                ("URLInfoAbout", "REG_SZ", "https://github.com/rkaradas/YAGUI"),
                ("NoModify", "REG_DWORD", "1"),
                ("NoRepair", "REG_DWORD", "1"),
                ("EstimatedSize", "REG_DWORD", size_kb.as_str()),
            ] {
                run_hidden("reg", &["add", UNINSTALL_KEY, "/v", name, "/t", kind, "/d", val, "/f"])?;
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
        let home = env_dir("HOME").unwrap_or_default();
        if o.start_menu {
            let bin = home.join(".local/bin");
            let _ = std::fs::create_dir_all(&bin);
            let link = bin.join("yagui");
            let _ = std::fs::remove_file(&link);
            let _ = std::os::unix::fs::symlink(&target, &link);
            let icon_dir = home.join(".local/share/icons/hicolor/256x256/apps");
            let _ = std::fs::create_dir_all(&icon_dir);
            if let Some(img) = image::RgbaImage::from_raw(256, 256, crate::appicon::icon_pixels(256)) {
                let _ = img.save(icon_dir.join("yagui.png"));
            }
            let apps = home.join(".local/share/applications");
            let _ = std::fs::create_dir_all(&apps);
            let desktop = format!(
                "[Desktop Entry]\nType=Application\nName=YAGUI\nGenericName=Git client\nComment=Yet Another Git User Interface\nExec=\"{}\" %f\nIcon=yagui\nTerminal=false\nCategories=Development;RevisionControl;\nStartupWMClass=YAGUI.GitClient\n",
                target.display()
            );
            std::fs::write(apps.join("yagui.desktop"), desktop).map_err(|e| e.to_string())?;
        }
    }
    Ok(target)
}

/// Removes shortcuts, Explorer entries, the Apps entry and (optionally) the settings, then the
/// program folder (after this program has exited).
pub fn uninstall(dir: &Path, remove_settings: bool, register: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = std::fs::remove_file(start_menu_lnk());
        let _ = std::fs::remove_file(desktop_lnk());
        if register {
            let _ = run_hidden("reg", &["delete", r"HKCU\Software\Classes\Directory\shell\YAGUI", "/f"]);
            let _ = run_hidden("reg", &["delete", r"HKCU\Software\Classes\Directory\Background\shell\YAGUI", "/f"]);
            let _ = run_hidden("reg", &["delete", UNINSTALL_KEY, "/f"]);
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let home = env_dir("HOME").unwrap_or_default();
        let _ = std::fs::remove_file(home.join(".local/bin/yagui"));
        let _ = std::fs::remove_file(home.join(".local/share/applications/yagui.desktop"));
        let _ = std::fs::remove_file(home.join(".local/share/icons/hicolor/256x256/apps/yagui.png"));
    }
    let _ = register;
    if remove_settings {
        if let Some(base) = env_dir("APPDATA").or_else(|| env_dir("HOME").map(|h| h.join(".config"))) {
            let _ = std::fs::remove_dir_all(base.join("YAGUI"));
        }
    }
    if cfg!(target_os = "macos") {
        return Ok(());
    }
    // The running program can't delete its own folder: remove it shortly after exit.
    let me = std::env::current_exe().ok();
    let inside = me.as_ref().map(|m| m.starts_with(dir)).unwrap_or(false);
    if inside {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let script = format!("ping 127.0.0.1 -n 3 > nul & rmdir /s /q \"{}\"", dir.display());
            let _ = Command::new("cmd").args(["/c", &script]).creation_flags(0x0800_0000).spawn();
        }
        #[cfg(unix)]
        {
            let script = format!("sleep 2; rm -rf '{}'", dir.display().to_string().replace('\'', "'\\''"));
            let _ = Command::new("sh").args(["-c", &script]).spawn();
        }
    } else {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_uninstall_into_a_folder() {
        let dir = std::env::temp_dir().join(format!("yagui-install-test-{}", std::process::id()));
        let o = Options { dir: dir.clone(), start_menu: false, desktop: false, context_menu: false, register: false };
        let exe = install(&o).unwrap();
        assert!(exe.exists());
        uninstall(&dir, false, false).unwrap();
        assert!(!dir.exists());
    }
}
