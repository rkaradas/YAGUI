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
//!   That needs a program folder this user may write to; a package (`/usr/bin/yagui` from the
//!   .deb) or a read-only folder gets the release page instead (`self_update_blocker`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::i18n::trs;
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
    let mut child = c.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            trs("curl is not installed (Debian/Ubuntu: sudo apt install curl)".into(), "curl ist nicht installiert (Debian/Ubuntu: sudo apt install curl)".into())
        } else {
            format!("curl: {e}")
        }
    })?;
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

/// Why an update check failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckErr {
    /// GitHub does not show the repository: it is private and there is no (matching) sign-in.
    NoAccess(u32),
    /// GitHub refused the sign-in stored in Git (expired or revoked).
    BadSignIn,
    /// Too many requests or not allowed (403/429).
    Refused(u32),
    /// Anything else: network, curl missing, an unexpected answer.
    Other(String),
}

impl CheckErr {
    pub fn text(&self) -> String {
        match self {
            CheckErr::NoAccess(c) => trs(
                format!("no access to github.com/{} (GitHub answered {c})", version::REPO),
                format!("kein Zugriff auf github.com/{} (GitHub antwortete {c})", version::REPO),
            ),
            CheckErr::BadSignIn => trs("GitHub refused the sign-in stored in Git (expired or revoked)".into(), "GitHub lehnt die in Git gespeicherte Anmeldung ab (abgelaufen oder widerrufen)".into()),
            CheckErr::Refused(c) => trs(
                format!("GitHub refused the request ({c}): too many requests, or not allowed – try again later"),
                format!("GitHub lehnte die Anfrage ab ({c}): zu viele Anfragen oder nicht erlaubt – später erneut versuchen"),
            ),
            CheckErr::Other(s) => s.clone(),
        }
    }

    /// Signing in to GitHub (File > Accounts) would help.
    pub fn sign_in_helps(&self) -> bool {
        matches!(self, CheckErr::NoAccess(_) | CheckErr::BadSignIn)
    }
}

impl std::fmt::Display for CheckErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

/// The newest published release, or None when there is none.
pub fn latest() -> Result<Option<Release>, CheckErr> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", version::REPO);
    let (code, body) = curl(&url, "application/vnd.github+json", None).map_err(CheckErr::Other)?;
    match code {
        200 => {}
        404 => {
            // No published release, or no access: for a private repository without a
            // (matching) GitHub sign-in GitHub answers 404 too. The repository tells them apart.
            let (repo_code, _) = curl(&format!("https://api.github.com/repos/{}", version::REPO), "application/vnd.github+json", None).map_err(CheckErr::Other)?;
            return no_release_or_no_access(repo_code);
        }
        401 => return Err(CheckErr::BadSignIn),
        403 | 429 => return Err(CheckErr::Refused(code)),
        c => return Err(CheckErr::Other(trs(format!("GitHub answered {c}"), format!("GitHub antwortete {c}")))),
    }
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|e| CheckErr::Other(e.to_string()))?;
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

/// releases/latest answered 404: with the repository visible there is no release yet,
/// otherwise this user cannot see the (private) repository.
fn no_release_or_no_access(repo_code: u32) -> Result<Option<Release>, CheckErr> {
    match repo_code {
        200 => Ok(None),
        401 => Err(CheckErr::BadSignIn),
        403 | 429 => Err(CheckErr::Refused(repo_code)),
        c => Err(CheckErr::NoAccess(c)),
    }
}

/// Why the running program cannot replace itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blocker {
    /// Installed by the package manager (the .deb, under /usr): update the package instead.
    Package(String),
    /// This user may not replace the program file: the reason (path and error).
    NoWrite(String),
}

/// Whether the running program cannot replace itself (worked out once).
pub fn self_update_blocker() -> Option<Blocker> {
    static BLOCKER: OnceLock<Option<Blocker>> = OnceLock::new();
    BLOCKER.get_or_init(|| std::env::current_exe().ok().and_then(|exe| blocker_for(&exe))).clone()
}

/// A program under /usr (not /usr/local) on Linux belongs to the package manager. Otherwise
/// its folder must take a new file (tried with a short-lived probe file), and on Windows the
/// program itself must be renamable: exe -> exe.old needs DELETE on it or "delete subfolders
/// and files" on the folder, which e.g. a C:\ProgramData folder made by an administrator does
/// not give normal users.
fn blocker_for(exe: &Path) -> Option<Blocker> {
    if cfg!(target_os = "linux") && exe.starts_with("/usr") && !exe.starts_with("/usr/local") {
        return Some(Blocker::Package(exe.display().to_string()));
    }
    let dir = exe.parent()?;
    let probe = dir.join(format!(".yagui-update-probe-{}", std::process::id()));
    if let Err(e) = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        return Some(Blocker::NoWrite(format!("{}: {e}", dir.display())));
    }
    let _ = std::fs::remove_file(&probe);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const DELETE: u32 = 0x0001_0000;
        if let Err(e) = std::fs::OpenOptions::new().access_mode(DELETE).share_mode(7).open(exe) {
            // Only a refusal counts: a passing lock by another program (os error 32) would
            // otherwise stay cached for the whole session.
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                return Some(Blocker::NoWrite(format!("{}: {e}", exe.display())));
            }
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_404_means_no_release_only_when_the_repository_is_visible() {
        assert!(matches!(no_release_or_no_access(200), Ok(None)));
        let e = no_release_or_no_access(404).unwrap_err();
        assert_eq!(e, CheckErr::NoAccess(404));
        assert!(e.sign_in_helps());
        assert!(!CheckErr::Refused(429).sign_in_helps());
    }

    #[test]
    fn self_update_needs_a_program_folder_that_takes_new_files() {
        let dir = std::env::temp_dir().join(format!("yagui-blocker-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("yagui");
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(blocker_for(&exe), None);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "the probe file is removed again");
        assert!(matches!(blocker_for(&dir.join("missing").join("yagui")), Some(Blocker::NoWrite(_))));
        let _ = std::fs::remove_dir_all(&dir);
        #[cfg(target_os = "linux")]
        {
            assert!(matches!(blocker_for(Path::new("/usr/bin/yagui")), Some(Blocker::Package(_))));
            assert!(!matches!(blocker_for(Path::new("/usr/local/bin/yagui")), Some(Blocker::Package(_))));
        }
    }

    /// A program the user may not rename (only DELETE is denied, creating files is allowed).
    #[cfg(windows)]
    #[test]
    fn a_program_that_may_not_be_renamed_is_blocked() {
        let dir = std::env::temp_dir().join(format!("yagui-blocker-acl-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("yagui.exe");
        std::fs::write(&exe, b"").unwrap();
        let icacls = |p: &Path, a: &[&str]| std::process::Command::new("icacls").arg(p).args(a).output().map(|o| o.status.success()).unwrap_or(false);
        // BUILTIN\Users: no "delete child" on the folder, no "delete" on the file.
        let denied = icacls(&dir, &["/deny", "*S-1-5-32-545:(DC)"]) && icacls(&exe, &["/deny", "*S-1-5-32-545:(DE)"]);
        let b = blocker_for(&exe);
        icacls(&exe, &["/remove:d", "*S-1-5-32-545"]);
        icacls(&dir, &["/remove:d", "*S-1-5-32-545"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(denied && matches!(b, Some(Blocker::NoWrite(_))), "{b:?}");
    }
}
