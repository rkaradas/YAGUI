//! Updates from the GitHub releases of YAGUI.
//!
//! - The newest release is read from `api.github.com/repos/<REPO>/releases/latest` with `curl`
//!   (on every supported system). If Git has a GitHub sign-in stored (Accounts), it is sent too,
//!   so a private repository works for its owner; the token is passed on standard input, never
//!   on the command line.
//! - The file for this computer is chosen by its name: `yagui-<version>-<platform>.exe` on
//!   Windows, `yagui-<version>-<platform>.tar.gz` (holding the `yagui` program) elsewhere.
//! - Installing replaces the running program: it is renamed to `*.old` (allowed while it runs),
//!   the new one takes its place, YAGUI restarts and removes the old file on the next start.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::version;

#[derive(Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub api_url: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub notes: String,
    pub url: String,
    pub published: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// The file of this release that updates this computer's YAGUI.
    pub fn asset_for_this_computer(&self) -> Option<&Asset> {
        let plat = version::platform();
        self.assets.iter().find(|a| {
            let n = a.name.to_lowercase();
            n.contains(plat) && !n.contains("setup") && (if cfg!(windows) { n.ends_with(".exe") } else { n.ends_with(".tar.gz") })
        })
    }
}

fn no_window(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let _ = c;
}

/// The GitHub token Git keeps for github.com, if any (asks nothing, shows nothing).
fn github_token() -> Option<String> {
    let mut c = Command::new("git");
    c.args(["credential", "fill"]).env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    no_window(&mut c);
    let mut child = c.spawn().ok()?;
    child.stdin.take()?.write_all(b"protocol=https\nhost=github.com\n\n").ok()?;
    let out = child.wait_with_output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| l.strip_prefix("password=").map(String::from)).filter(|t| !t.is_empty())
}

/// Runs curl with GitHub headers (and the token on standard input). Returns (status, body).
fn curl(url: &str, accept: &str, out_file: Option<&Path>) -> Result<(u32, Vec<u8>), String> {
    let token = github_token();
    let mut c = Command::new("curl");
    c.args(["-sS", "-L", "--max-time", "600", "-H", &format!("Accept: {accept}"), "-H", "User-Agent: YAGUI-updater", "-H", "X-GitHub-Api-Version: 2022-11-28", "-w", "\n%{http_code}"]);
    if token.is_some() {
        c.args(["-H", "@-"]);
    }
    if let Some(f) = out_file {
        c.arg("-o").arg(f);
    }
    c.arg(url).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    no_window(&mut c);
    let mut child = c.spawn().map_err(|e| format!("curl: {e}"))?;
    if let Some(mut si) = child.stdin.take() {
        if let Some(t) = &token {
            let _ = si.write_all(format!("Authorization: Bearer {t}\n").as_bytes());
        }
    }
    let o = child.wait_with_output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).trim().to_string());
    }
    let mut body = o.stdout;
    // the status code is the last line (-w)
    let pos = body.iter().rposition(|b| *b == b'\n').unwrap_or(0);
    let code: u32 = String::from_utf8_lossy(&body[pos..]).trim().parse().unwrap_or(0);
    body.truncate(pos);
    Ok((code, body))
}

/// The newest published release, or None when there is none (or none can be seen).
pub fn latest() -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", version::REPO);
    let (code, body) = curl(&url, "application/vnd.github+json", None)?;
    if code == 404 {
        return Ok(None);
    }
    if code != 200 {
        return Err(format!("GitHub answered {code}"));
    }
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .map(|x| Asset {
                    name: x.get("name").and_then(|n| n.as_str()).unwrap_or("").into(),
                    api_url: x.get("url").and_then(|n| n.as_str()).unwrap_or("").into(),
                    size: x.get("size").and_then(|n| n.as_u64()).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(Release { tag: s("tag_name"), name: s("name"), notes: s("body"), url: s("html_url"), published: s("published_at").chars().take(10).collect(), assets }))
}

/// Downloads a release file into the temp folder.
pub fn download(a: &Asset) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("yagui-update");
    let _ = std::fs::create_dir_all(&dir);
    let dest = dir.join(&a.name);
    let (code, _) = curl(&a.api_url, "application/octet-stream", Some(&dest))?;
    if code != 200 {
        return Err(format!("GitHub answered {code}"));
    }
    let len = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    if a.size > 0 && len != a.size {
        return Err(format!("download incomplete ({len} of {} bytes)", a.size));
    }
    Ok(dest)
}

/// Puts the downloaded program in place of the running one.
pub fn install(downloaded: &Path) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let new_bin = if downloaded.extension().map(|e| e == "gz").unwrap_or(false) {
        let dir = downloaded.with_extension("").with_extension("");
        let _ = std::fs::create_dir_all(&dir);
        let st = Command::new("tar").arg("-xzf").arg(downloaded).arg("-C").arg(&dir).status().map_err(|e| e.to_string())?;
        if !st.success() {
            return Err("could not unpack the update".into());
        }
        find_file(&dir, "yagui").ok_or("the update holds no yagui program")?
    } else {
        downloaded.to_path_buf()
    };
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("could not move the running program aside: {e}"))?;
    if let Err(e) = std::fs::copy(&new_bin, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("could not write the new program: {e}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
    Ok(exe)
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if p.file_name().map(|n| n == name).unwrap_or(false) {
            return Some(p);
        }
    }
    None
}

fn old_path(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

/// Removes what the previous update left behind (call at start).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
    }
}

/// Starts the (new) program again; the caller then closes this window.
pub fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let _ = Command::new(exe).args(args).spawn();
    }
}
