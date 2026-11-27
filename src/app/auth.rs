//! Authentication and integration from the File menu:
//! "Configure SSH Keys…", "Accounts…" (credentials for HTTPS hosts), "Preferences…",
//! and the toolbar's "Open in" menu (terminal, file explorer, detected editors).

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, RichText, Vec2};

use super::ui::{icon, icon_button};
use super::{App, Lang};
use crate::git::{JobSpec, JobState};
use crate::i18n::{keys, tr, trs};
use crate::icons::Icon;
use crate::theme;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

// ---------------------------------------------------------------------------------------------
// Opening things outside the app

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize, Default)]
pub enum ShellTool {
    #[default]
    Default,
    WindowsTerminal,
    CommandPrompt,
    PowerShell,
    GitBash,
}

impl ShellTool {
    pub const ALL: [ShellTool; 5] = [ShellTool::Default, ShellTool::WindowsTerminal, ShellTool::CommandPrompt, ShellTool::PowerShell, ShellTool::GitBash];
    pub fn label(self) -> &'static str {
        match self {
            ShellTool::Default => tr("Default", "Standard"),
            ShellTool::WindowsTerminal => "Windows Terminal",
            ShellTool::CommandPrompt => tr("Command Prompt", "Eingabeaufforderung"),
            ShellTool::PowerShell => "PowerShell",
            ShellTool::GitBash => "Git Bash",
        }
    }
}

#[derive(Clone)]
pub struct Editor {
    pub name: String,
    /// The program; on macOS the app bundle (`…/Visual Studio Code.app`).
    pub exe: PathBuf,
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// Editors and IDEs installed in their usual places.
pub fn detect_editors() -> Vec<Editor> {
    if cfg!(target_os = "macos") {
        return mac_editors();
    }
    if cfg!(all(unix, not(target_os = "macos"))) {
        return linux_editors();
    }
    let mut out = Vec::new();
    let local = env_path("LOCALAPPDATA");
    let pf = env_path("ProgramFiles");
    let pf86 = env_path("ProgramFiles(x86)");
    let mut cand: Vec<(&str, PathBuf)> = Vec::new();
    if let Some(l) = &local {
        cand.push(("Visual Studio Code", l.join("Programs/Microsoft VS Code/Code.exe")));
        cand.push(("Visual Studio Code Insiders", l.join("Programs/Microsoft VS Code Insiders/Code - Insiders.exe")));
        cand.push(("Cursor", l.join("Programs/cursor/Cursor.exe")));
        cand.push(("Windsurf", l.join("Programs/Windsurf/Windsurf.exe")));
        cand.push(("Zed", l.join("Programs/Zed/Zed.exe")));
    }
    for base in [&pf, &pf86].into_iter().flatten() {
        cand.push(("Visual Studio Code", base.join("Microsoft VS Code/Code.exe")));
        cand.push(("Sublime Text", base.join("Sublime Text/sublime_text.exe")));
        cand.push(("Sublime Text", base.join("Sublime Text 3/sublime_text.exe")));
        cand.push(("Notepad++", base.join("Notepad++/notepad++.exe")));
    }
    for (name, p) in cand {
        if p.exists() && !out.iter().any(|e: &Editor| e.name == name) {
            out.push(Editor { name: name.to_string(), exe: p });
        }
    }
    // JetBrains IDEs: <ProgramFiles>/JetBrains/<IDE version>/bin/<ide>64.exe
    for base in [&pf, &pf86].into_iter().flatten() {
        if let Ok(rd) = std::fs::read_dir(base.join("JetBrains")) {
            for d in rd.flatten() {
                let bin = d.path().join("bin");
                if let Ok(bins) = std::fs::read_dir(&bin) {
                    for b in bins.flatten() {
                        let n = b.file_name().to_string_lossy().to_string();
                        if n.ends_with("64.exe") && !n.starts_with("fsnotifier") && !n.starts_with("elevator") && !n.starts_with("restarter") {
                            out.push(Editor { name: d.file_name().to_string_lossy().to_string(), exe: b.path() });
                            break;
                        }
                    }
                }
            }
        }
    }
    out
}

/// Editor apps on macOS, in /Applications and ~/Applications (where JetBrains Toolbox puts its
/// IDEs, also in a "JetBrains Toolbox" subfolder). They are opened with `open -a`.
fn mac_editors() -> Vec<Editor> {
    const APPS: [(&str, &str); 13] = [
        ("Visual Studio Code", "Visual Studio Code.app"),
        ("Visual Studio Code Insiders", "Visual Studio Code - Insiders.app"),
        ("VSCodium", "VSCodium.app"),
        ("Cursor", "Cursor.app"),
        ("Windsurf", "Windsurf.app"),
        ("Devin", "Devin.app"),
        ("Zed", "Zed.app"),
        ("Zed Preview", "Zed Preview.app"),
        ("Sublime Text", "Sublime Text.app"),
        ("Nova", "Nova.app"),
        ("BBEdit", "BBEdit.app"),
        ("TextMate", "TextMate.app"),
        ("Xcode", "Xcode.app"),
    ];
    // Editions carry their own names ("IntelliJ IDEA CE.app", "PyCharm Professional Edition.app").
    const JETBRAINS: [&str; 12] = ["IntelliJ IDEA", "PyCharm", "WebStorm", "CLion", "GoLand", "RustRover", "Rider", "PhpStorm", "RubyMine", "DataGrip", "Fleet", "Android Studio"];
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = env_path("HOME") {
        roots.push(home.join("Applications"));
        roots.push(home.join("Applications/JetBrains Toolbox"));
    }
    let mut out: Vec<Editor> = Vec::new();
    let mut add = |name: String, app: PathBuf| {
        if app.is_dir() && !out.iter().any(|e| e.name == name) {
            out.push(Editor { name, exe: app });
        }
    };
    for (name, app) in APPS {
        for r in &roots {
            add(name.to_string(), r.join(app));
        }
    }
    for r in &roots {
        let Ok(rd) = std::fs::read_dir(r) else { continue };
        let mut apps: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "app")).collect();
        apps.sort();
        for app in apps {
            let name = app.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if JETBRAINS.iter().any(|j| name.starts_with(j)) {
                add(name, app);
            }
        }
    }
    out
}

/// Editors on Linux: launchers on PATH (packages, Snap) and the scripts JetBrains Toolbox writes
/// to ~/.local/share/JetBrains/Toolbox/scripts. A plain `zed` counts only in the home folder
/// (Zed's own installer puts it in ~/.local/bin): elsewhere it is usually the ZFS event daemon,
/// which is why distributions name the editor `zeditor`.
fn linux_editors() -> Vec<Editor> {
    const COMMANDS: [(&str, &str); 17] = [
        ("Visual Studio Code", "code"),
        ("Visual Studio Code Insiders", "code-insiders"),
        ("VSCodium", "codium"),
        ("Cursor", "cursor"),
        ("Windsurf", "windsurf"),
        ("Zed", "zeditor"),
        ("Zed", "zed"),
        ("Sublime Text", "subl"),
        ("Kate", "kate"),
        ("IntelliJ IDEA", "idea"),
        ("PyCharm", "pycharm"),
        ("WebStorm", "webstorm"),
        ("CLion", "clion"),
        ("GoLand", "goland"),
        ("RustRover", "rustrover"),
        ("Rider", "rider"),
        ("PhpStorm", "phpstorm"),
    ];
    let home = env_path("HOME");
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).filter(|d| !d.as_os_str().is_empty()).collect()).unwrap_or_default();
    if let Some(h) = &home {
        dirs.push(h.join(".local/share/JetBrains/Toolbox/scripts"));
    }
    let mut out: Vec<Editor> = Vec::new();
    for (name, cmd) in COMMANDS {
        if out.iter().any(|e| e.name == name) {
            continue;
        }
        let allowed = |d: &PathBuf| cmd != "zed" || home.as_ref().is_some_and(|h| d.starts_with(h));
        if let Some(exe) = dirs.iter().filter(|d| allowed(d)).map(|d| d.join(cmd)).find(|p| p.is_file()) {
            out.push(Editor { name: name.to_string(), exe });
        }
    }
    out
}

/// Windows tools want backslashes; git reports paths with forward slashes.
pub fn native(p: &Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) { s.replace('/', "\\") } else { s }
}

pub fn open_explorer(p: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(native(p)).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

pub fn open_in_editor(e: &Editor, p: &Path) {
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("open").arg("-a").arg(&e.exe).arg(p).spawn();
    } else {
        let _ = std::process::Command::new(&e.exe).arg(native(p)).spawn();
    }
}

fn windows_terminal() -> Option<PathBuf> {
    let p = env_path("LOCALAPPDATA")?.join("Microsoft/WindowsApps/wt.exe");
    p.exists().then_some(p)
}

fn git_bash() -> Option<PathBuf> {
    let o = std::process::Command::new("git").arg("--exec-path").output().ok()?;
    let exec = PathBuf::from(String::from_utf8_lossy(&o.stdout).trim());
    // <git>/mingw64/libexec/git-core -> <git>/git-bash.exe
    let root = exec.parent()?.parent()?.parent()?;
    let p = root.join("git-bash.exe");
    p.exists().then_some(p)
}

pub fn open_terminal(shell: ShellTool, p: &Path) {
    let dir = native(p);
    #[cfg(windows)]
    {
        const NEW_CONSOLE: u32 = 0x0000_0010;
        let tool = match shell {
            ShellTool::Default => {
                if windows_terminal().is_some() { ShellTool::WindowsTerminal } else { ShellTool::CommandPrompt }
            }
            s => s,
        };
        let _ = match tool {
            ShellTool::WindowsTerminal => match windows_terminal() {
                Some(wt) => std::process::Command::new(wt).args(["-d", &dir]).spawn(),
                None => std::process::Command::new("cmd").current_dir(&dir).creation_flags(NEW_CONSOLE).spawn(),
            },
            ShellTool::PowerShell => std::process::Command::new("powershell").arg("-NoExit").current_dir(&dir).creation_flags(NEW_CONSOLE).spawn(),
            ShellTool::GitBash => match git_bash() {
                Some(b) => std::process::Command::new(b).arg(format!("--cd={dir}")).spawn(),
                None => std::process::Command::new("cmd").current_dir(&dir).creation_flags(NEW_CONSOLE).spawn(),
            },
            _ => std::process::Command::new("cmd").current_dir(&dir).creation_flags(NEW_CONSOLE).spawn(),
        };
    }
    #[cfg(target_os = "macos")]
    {
        let _ = shell;
        let _ = std::process::Command::new("open").args(["-a", "Terminal", &dir]).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = shell;
        let _ = std::process::Command::new("x-terminal-emulator").current_dir(&dir).spawn();
    }
}

fn run_capture(program: &str, args: &[&str]) -> (i32, String) {
    let mut c = std::process::Command::new(program);
    c.args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    match c.output() {
        Ok(o) => (o.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
        Err(e) => (-1, e.to_string()),
    }
}

// ---------------------------------------------------------------------------------------------
// SSH keys

pub struct SshKey {
    name: String,
    private: PathBuf,
    public: PathBuf,
    pubtext: String,
    fingerprint: String,
}

struct NewKey {
    kind: usize,
    email: String,
    file: String,
    pass: String,
    pass2: String,
}

pub struct SshState {
    keys: Vec<SshKey>,
    sel: Option<usize>,
    config: String,
    ssh_command: String,
    agent: String,
    new_key: Option<NewKey>,
    host: String,
    job: Option<u64>,
    refreshed_for: Option<u64>,
}

fn ssh_dir() -> PathBuf {
    env_path("USERPROFILE").or_else(|| env_path("HOME")).unwrap_or_default().join(".ssh")
}

fn load_key(public: &Path) -> Option<SshKey> {
    let pubtext = std::fs::read_to_string(public).ok()?.trim().to_string();
    let private = public.with_extension("");
    let (_, fp) = run_capture("ssh-keygen", &["-lf", &public.display().to_string()]);
    let fingerprint = fp.split_whitespace().nth(1).unwrap_or("").to_string();
    Some(SshKey { name: public.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), private, public: public.to_path_buf(), pubtext, fingerprint })
}

fn load_keys(extra: &[PathBuf]) -> Vec<SshKey> {
    let mut keys = Vec::new();
    if let Ok(rd) = std::fs::read_dir(ssh_dir()) {
        let mut pubs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x == "pub").unwrap_or(false)).collect();
        pubs.sort();
        for p in pubs {
            if let Some(k) = load_key(&p) {
                keys.push(k);
            }
        }
    }
    for p in extra {
        if !keys.iter().any(|k| &k.public == p) {
            if let Some(k) = load_key(p) {
                keys.push(k);
            }
        }
    }
    keys
}

fn ssh_status() -> (String, String, String) {
    let (_, cmd) = run_capture("git", &["config", "--global", "--get", "core.sshCommand"]);
    let cmd = cmd.trim().to_string();
    let (code, _) = run_capture("ssh-add", &["-l"]);
    let agent = match code {
        0 => tr("ssh-agent is running and has keys loaded", "ssh-agent läuft und hat Schlüssel geladen"),
        1 => tr("ssh-agent is running (no keys loaded)", "ssh-agent läuft (keine Schlüssel geladen)"),
        _ => tr("ssh-agent is not running", "ssh-agent läuft nicht"),
    };
    let config = if cmd.is_empty() { tr("default system ssh-agent", "Standard (System-ssh-agent)").to_string() } else { cmd.clone() };
    (config, cmd, agent.to_string())
}

impl SshState {
    pub fn new(extra: &[PathBuf]) -> Self {
        let keys = load_keys(extra);
        let (config, ssh_command, agent) = ssh_status();
        SshState { sel: if keys.is_empty() { None } else { Some(0) }, keys, config, ssh_command, agent, new_key: None, host: "github.com".into(), job: None, refreshed_for: None }
    }
}

// ---------------------------------------------------------------------------------------------
// Credentials (HTTPS accounts)

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub host: String,
    pub user: String,
}

pub struct CredState {
    helper: String,
    /// Index into `HELPERS`; None for a helper that is not in the list.
    helper_sel: Option<usize>,
    sel: Option<usize>,
    host: String,
    user: String,
    token: String,
    job: Option<u64>,
}

/// Credential helpers offered in Accounts: value of credential.helper, name, help (en, de).
type Helper = (&'static str, &'static str, &'static str, &'static str);

const STORE: Helper = ("store", "store", "Saves passwords in a plain text file (~/.git-credentials). Not recommended.", "Speichert Passwörter als Klartext in ~/.git-credentials. Nicht empfohlen.");
const CACHE: Helper = ("cache", "cache", "Keeps passwords in memory for 15 minutes.", "Behält Passwörter 15 Minuten im Speicher.");
// YAGUI runs Git without a terminal (GIT_TERMINAL_PROMPT=0), so nothing can ask for a password.
const NO_HELPER: Helper = (
    "",
    "none",
    "Git keeps no sign-in and asks for one only in a terminal. YAGUI cannot ask, so fetch, pull and push over HTTPS fail where a sign-in is needed (use SSH or another helper).",
    "Git speichert keine Anmeldung und fragt nur im Terminal danach. YAGUI kann nicht nachfragen, daher schlagen Fetch, Pull und Push über HTTPS fehl, sobald eine Anmeldung nötig ist (SSH oder einen anderen Helper verwenden).",
);

#[cfg(windows)]
const HELPERS: &[Helper] = &[
    ("manager", "Git Credential Manager", "Signs in through the browser and keeps tokens in the Windows Credential Manager (recommended).", "Meldet sich über den Browser an und speichert Tokens in der Windows-Anmeldeinformationsverwaltung (empfohlen)."),
    STORE,
    CACHE,
    NO_HELPER,
];
#[cfg(target_os = "macos")]
const HELPERS: &[Helper] = &[
    ("osxkeychain", "macOS Keychain", "Keeps passwords and tokens in the macOS Keychain (recommended; Apple's Git uses it by default).", "Speichert Passwörter und Tokens im macOS-Schlüsselbund (empfohlen; Apples Git nutzt ihn von sich aus)."),
    ("manager", "Git Credential Manager", "Signs in through the browser and keeps tokens in the macOS Keychain. Git Credential Manager must be installed.", "Meldet sich über den Browser an und speichert Tokens im macOS-Schlüsselbund. Git Credential Manager muss installiert sein."),
    STORE,
    CACHE,
    NO_HELPER,
];
#[cfg(all(unix, not(target_os = "macos")))]
const HELPERS: &[Helper] = &[
    ("manager", "Git Credential Manager", "Signs in through the browser. Git Credential Manager must be installed and given a credential store (credential.credentialStore, for example secretservice).", "Meldet sich über den Browser an. Git Credential Manager muss installiert und mit einem Speicher eingerichtet sein (credential.credentialStore, zum Beispiel secretservice)."),
    STORE,
    CACHE,
    NO_HELPER,
];

fn helper_help(h: &Helper) -> &'static str {
    tr(h.2, h.3)
}

/// The helpers Git uses outside a repository, in the order Git reads them (an empty value clears
/// the ones before it). Git for Windows sets manager system-wide, Homebrew's and MacPorts' Git
/// set osxkeychain there, and Apple's Git reads its own gitconfig first (scope "unknown"), so the
/// global setting alone does not tell. A repository's own setting does not count here.
fn effective_helpers(config: &str) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for line in config.lines() {
        let Some((scope, value)) = line.split_once('\t') else { continue };
        if !matches!(scope, "unknown" | "system" | "global") {
            continue;
        }
        let value = value.trim();
        if value.is_empty() {
            list.clear();
        } else {
            list.push(value.to_string());
        }
    }
    list
}

/// Which entry of `HELPERS` a configured helper is (by its name inside the value, so
/// "/usr/local/share/gcm-core/git-credential-manager" is Git Credential Manager).
fn helper_index(helper: &str) -> Option<usize> {
    if helper.is_empty() {
        return HELPERS.iter().position(|h| h.0.is_empty());
    }
    HELPERS.iter().position(|h| !h.0.is_empty() && helper.contains(h.0))
}

impl CredState {
    pub fn new() -> Self {
        let (code, out) = run_capture("git", &["config", "--show-scope", "--get-all", "credential.helper"]);
        let helpers = match code {
            // 1: no helper set anywhere.
            0 | 1 => Some(effective_helpers(&out)),
            // Git before 2.26 has no --show-scope: read the values without it, outside any
            // repository so a repository's own setting does not count.
            129 => {
                let mut c = std::process::Command::new("git");
                c.args(["config", "--get-all", "credential.helper"]).current_dir(std::env::temp_dir()).stdin(std::process::Stdio::null());
                #[cfg(windows)]
                c.creation_flags(0x0800_0000);
                let o = c.output().ok().filter(|o| matches!(o.status.code(), Some(0 | 1)));
                o.map(|o| effective_helpers(&String::from_utf8_lossy(&o.stdout).lines().map(|l| format!("unknown\t{l}\n")).collect::<String>()))
            }
            _ => None,
        };
        let (helper, helper_sel) = match helpers {
            Some(h) => (h.join(", "), helper_index(h.last().map(String::as_str).unwrap_or(""))),
            None => (tr("unknown", "unbekannt").to_string(), None),
        };
        CredState { helper, helper_sel, sel: None, host: "github.com".into(), user: String::new(), token: String::new(), job: None }
    }
}

pub struct PrefsState {
    tab: usize,
    name: String,
    email: String,
    git_version: String,
    job: Option<u64>,
    cc_sel: usize,
    merge_tool: String,
    diff_tool: String,
    submodule_recurse: bool,
}

/// Merge/diff tools Git knows, with the command line for the ones it doesn't.
const TOOLS: [(&str, &str, &str, &str); 8] = [
    ("vscode", "Visual Studio Code", "code --wait --merge \"$REMOTE\" \"$LOCAL\" \"$BASE\" \"$MERGED\"", "code --wait --diff \"$LOCAL\" \"$REMOTE\""),
    ("bc", "Beyond Compare", "", ""),
    ("kdiff3", "KDiff3", "", ""),
    ("meld", "Meld", "", ""),
    ("winmerge", "WinMerge", "", ""),
    ("p4merge", "P4Merge", "", ""),
    ("araxis", "Araxis Merge", "", ""),
    ("tortoisemerge", "TortoiseMerge", "", ""),
];

impl PrefsState {
    pub fn new() -> Self {
        let get = |k: &str| run_capture("git", &["config", "--global", "--get", k]).1.trim().to_string();
        PrefsState {
            tab: 0,
            name: get("user.name"),
            email: get("user.email"),
            git_version: run_capture("git", &["--version"]).1.trim().to_string(),
            job: None,
            cc_sel: 0,
            merge_tool: get("merge.tool"),
            diff_tool: get("diff.tool"),
            submodule_recurse: get("submodule.recurse") == "true",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(18))
}

fn heading(ui: &mut egui::Ui, title: &str, lines: &[&str]) {
    ui.label(RichText::new(title).font(FontId::new(15.0, theme::semibold())).color(theme::TEXT_STRONG()));
    for l in lines {
        ui.label(RichText::new(*l).size(12.5).color(theme::TEXT_DIM()));
    }
    ui.add_space(8.0);
    rule(ui);
    ui.add_space(8.0);
}

fn rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0, theme::LINE());
}

fn kv(ui: &mut egui::Ui, k: &str, v: &str, mono: bool) {
    ui.label(RichText::new(k).color(theme::TEXT_DIM()));
    let t = RichText::new(v).size(12.5);
    ui.add(egui::Label::new(if mono { t.monospace() } else { t }).wrap());
    ui.end_row();
}

fn primary(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(if enabled { theme::ACCENT_DARK() } else { theme::dk(0x36, 0x36, 0x39) }).min_size(Vec2::new(80.0, 26.0))).clicked()
}

fn job_line(ui: &mut egui::Ui, app_jobs: &[crate::git::Job], id: Option<u64>) {
    let Some(j) = id.and_then(|id| app_jobs.iter().find(|j| j.id == id)) else { return };
    ui.add_space(6.0);
    let (txt, col) = match j.state() {
        JobState::Running => (tr("Running…", "Läuft…").to_string(), theme::ACCENT()),
        JobState::Done(0) | JobState::Started => (tr("Done", "Fertig").to_string(), theme::GREEN()),
        JobState::Done(c) => (trs(format!("Failed (exit code {c})"), format!("Fehlgeschlagen (Exit-Code {c})")), theme::RED()),
    };
    ui.horizontal(|ui| {
        if j.state() == JobState::Running {
            ui.spinner();
        }
        ui.label(RichText::new(txt).size(12.0).color(col));
        ui.label(RichText::new(j.display.lines().next().unwrap_or("")).monospace().size(11.0).color(theme::TEXT_FAINT()));
    });
    let out = j.output();
    if !out.trim().is_empty() {
        egui::Frame::new().fill(theme::dk(0x1c, 0x1c, 0x1c)).corner_radius(4).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt(("authout", j.id)).max_height(90.0).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(out.trim()).monospace().size(11.5)).wrap());
            });
        });
    }
}

impl App {
    fn spawn_tool(&mut self, title: String, program: &str, args: Vec<String>, display: String, stdin: Option<String>, ctx: &egui::Context) -> u64 {
        let cwd = self.tabs.get(self.active).map(|r| r.path.clone());
        self.spawn(JobSpec { title, program: program.into(), args, display, cwd, stdin, env: vec![], changes_repo: false, detached: false, automatic: false }, ctx)
    }

    /// The toolbar's "Open in" menu.
    pub(super) fn open_in_menu(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.tabs.get(self.active).map(|r| r.path.clone()) else { return };
        let row = |ui: &mut egui::Ui, ic: Icon, label: &str, shortcut: &str| -> bool {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(280.0, 26.0), egui::Sense::click());
            let p = ui.painter_at(rect);
            if resp.hovered() {
                p.rect_filled(rect, 4, theme::ROW_HOVER());
            }
            crate::icons::draw(&p, egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(14.0)), ic, theme::TEXT());
            p.text(egui::pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.0), theme::TEXT());
            if !shortcut.is_empty() {
                p.text(egui::pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, keys(shortcut), FontId::proportional(11.5), theme::TEXT_FAINT());
            }
            resp.clicked()
        };
        if row(ui, Icon::Terminal, tr("Open in Console", "In Konsole öffnen"), "Ctrl+Alt+T") {
            open_terminal(self.shell, &path);
            ui.close();
        }
        if row(ui, Icon::Folder, tr("Open in File Explorer", "Im Datei-Explorer öffnen"), "Ctrl+Alt+O") {
            open_explorer(&path);
            ui.close();
        }
        if !self.editors.is_empty() {
            ui.separator();
            for e in self.editors.clone() {
                if row(ui, Icon::Changes, &trs(format!("Open in {}", e.name), format!("In {} öffnen", e.name)), "") {
                    open_in_editor(&e, &path);
                    ui.close();
                }
            }
        }
        ui.separator();
        ui.label(RichText::new(trs(format!("Terminal: {}", self.shell.label()), format!("Terminal: {}", self.shell.label()))).size(11.0).color(theme::TEXT_FAINT()));
    }

    pub(super) fn auth_dialogs(&mut self, ctx: &egui::Context) {
        self.ssh_dialog(ctx);
        self.creds_dialog(ctx);
        self.prefs_dialog(ctx);
    }

    // ----------------------------------------------------------------------------------- SSH

    fn ssh_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.ssh.take() else { return };
        // reload the list once a key job has finished
        if let Some(id) = st.job {
            if st.refreshed_for != Some(id) && self.jobs.iter().any(|j| j.id == id && j.state() != JobState::Running) {
                let extra = self.extra_keys.clone();
                let sel_name = st.sel.and_then(|i| st.keys.get(i)).map(|k| k.public.clone());
                st.keys = load_keys(&extra);
                let (c, sc, a) = ssh_status();
                st.config = c;
                st.ssh_command = sc;
                st.agent = a;
                st.sel = sel_name.and_then(|p| st.keys.iter().position(|k| k.public == p)).or(if st.keys.is_empty() { None } else { Some(st.keys.len() - 1) });
                st.refreshed_for = Some(id);
            }
        }
        let mut open = true;
        let mut close = false;
        let mut act: Option<(String, &'static str, Vec<String>, String)> = None;
        egui::Window::new(RichText::new(tr("SSH Keys", "SSH-Schlüssel")).font(FontId::new(14.0, theme::semibold())))
            .id(Id::unique("ssh_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(dialog_frame(ctx))
            .show(ctx, |ui| {
                ui.set_width(660.0);
                heading(
                    ui,
                    tr("Configure SSH Keys", "SSH-Schlüssel einrichten"),
                    &[
                        tr("1. Select or generate an SSH key that identifies this computer.", "1. Wähle oder erzeuge einen SSH-Schlüssel, der diesen Computer ausweist."),
                        tr("2. Copy the public key into the SSH settings of your Git provider (GitHub, GitLab, …).", "2. Kopiere den öffentlichen Schlüssel in die SSH-Einstellungen deines Git-Anbieters (GitHub, GitLab, …)."),
                    ],
                );
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("SSH configuration:", "SSH-Konfiguration:")).font(FontId::new(12.5, theme::semibold())));
                    ui.label(RichText::new(&st.config).italics().size(12.5));
                    ui.label(RichText::new(format!("· {}", st.agent)).size(12.0).color(theme::TEXT_FAINT()));
                });
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    // key list
                    ui.vertical(|ui| {
                        egui::Frame::new().fill(theme::PANEL_DARK()).stroke(egui::Stroke::new(1.0, theme::LINE())).corner_radius(3).show(ui, |ui| {
                            ui.set_width(200.0);
                            ui.set_min_height(170.0);
                            egui::ScrollArea::vertical().id_salt("sshkeys").max_height(170.0).show(ui, |ui| {
                                if st.keys.is_empty() {
                                    ui.add_space(60.0);
                                    ui.vertical_centered(|ui| ui.label(RichText::new(tr("No SSH keys", "Keine SSH-Schlüssel")).size(14.0).color(theme::TEXT_DIM())));
                                }
                                for (i, k) in st.keys.iter().enumerate() {
                                    let sel = st.sel == Some(i);
                                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(200.0, 34.0), egui::Sense::click());
                                    let p = ui.painter_at(rect);
                                    if sel {
                                        p.rect_filled(rect, 0, theme::ACCENT_DARK());
                                    } else if resp.hovered() {
                                        p.rect_filled(rect, 0, theme::ROW_HOVER());
                                    }
                                    crate::icons::draw(&p, egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(14.0)), Icon::Lock, theme::TEXT());
                                    p.text(egui::pos2(rect.left() + 32.0, rect.top() + 11.0), Align2::LEFT_CENTER, &k.name, FontId::proportional(13.0), theme::TEXT_STRONG());
                                    let kind = k.pubtext.split_whitespace().next().unwrap_or("").trim_start_matches("ssh-");
                                    p.text(egui::pos2(rect.left() + 32.0, rect.bottom() - 10.0), Align2::LEFT_CENTER, kind, FontId::proportional(11.0), theme::TEXT_DIM());
                                    if resp.clicked() {
                                        st.sel = Some(i);
                                        st.new_key = None;
                                    }
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let add = icon_button(ui, Icon::Plus, tr("Add key", "Schlüssel hinzufügen"));
                            egui::Popup::menu(&add).show(|ui| {
                                if ui.button(tr("New SSH Key…", "Neuer SSH-Schlüssel…")).clicked() {
                                    let email = self.tabs.get(self.active).map(|r| git_global(&r.path, "user.email")).unwrap_or_default();
                                    let n = if st.keys.iter().any(|k| k.name == "id_ed25519") { "id_ed25519_yagui" } else { "id_ed25519" };
                                    st.new_key = Some(NewKey { kind: 0, email, file: native(&ssh_dir().join(n)), pass: String::new(), pass2: String::new() });
                                    ui.close();
                                }
                                if ui.button(tr("Open Existing SSH Key…", "Vorhandenen SSH-Schlüssel öffnen…")).clicked() {
                                    if let Some(p) = rfd::FileDialog::new().set_directory(ssh_dir()).pick_file() {
                                        let public = if p.extension().map(|e| e == "pub").unwrap_or(false) { p } else { PathBuf::from(format!("{}.pub", p.display())) };
                                        if public.exists() {
                                            self.extra_keys.push(public.clone());
                                            st.keys = load_keys(&self.extra_keys);
                                            st.sel = st.keys.iter().position(|k| k.public == public);
                                        }
                                    }
                                    ui.close();
                                }
                            });
                            if st.sel.is_some() && icon_button(ui, Icon::Refresh, tr("Reload", "Neu laden")).clicked() {
                                st.keys = load_keys(&self.extra_keys);
                            }
                        });
                    });
                    ui.add_space(10.0);
                    // details / new key form
                    ui.vertical(|ui| {
                        ui.set_width(430.0);
                        let mut back = false;
                        if let Some(nk) = st.new_key.as_mut() {
                            ui.label(RichText::new(tr("New SSH key", "Neuer SSH-Schlüssel")).font(FontId::new(13.0, theme::semibold())));
                            ui.add_space(4.0);
                            egui::Grid::new("newkey").num_columns(2).spacing([10.0, 7.0]).show(ui, |ui| {
                                ui.label(RichText::new(tr("Type", "Typ")).color(theme::TEXT_DIM()));
                                egui::ComboBox::from_id_salt("kt").width(300.0).selected_text(if nk.kind == 0 { "Ed25519" } else { "RSA 4096" }).show_ui(ui, |ui| {
                                    ui.selectable_value(&mut nk.kind, 0, tr("Ed25519 (recommended)", "Ed25519 (empfohlen)"));
                                    ui.selectable_value(&mut nk.kind, 1, tr("RSA 4096 (for older servers)", "RSA 4096 (für ältere Server)"));
                                });
                                ui.end_row();
                                ui.label(RichText::new(tr("E-mail / label", "E-Mail / Bezeichnung")).color(theme::TEXT_DIM()));
                                ui.add(egui::TextEdit::singleline(&mut nk.email).desired_width(300.0));
                                ui.end_row();
                                ui.label(RichText::new(tr("File", "Datei")).color(theme::TEXT_DIM()));
                                ui.add(egui::TextEdit::singleline(&mut nk.file).desired_width(300.0));
                                ui.end_row();
                                ui.label(RichText::new(tr("Passphrase", "Passphrase")).color(theme::TEXT_DIM())).on_hover_text(tr("Optional. Protects the key if someone copies the file.", "Optional. Schützt den Schlüssel, falls jemand die Datei kopiert."));
                                ui.add(egui::TextEdit::singleline(&mut nk.pass).password(true).desired_width(300.0).hint_text(tr("optional", "optional")));
                                ui.end_row();
                                ui.label(RichText::new(tr("Repeat", "Wiederholen")).color(theme::TEXT_DIM()));
                                ui.add(egui::TextEdit::singleline(&mut nk.pass2).password(true).desired_width(300.0));
                                ui.end_row();
                            });
                            let exists = Path::new(&nk.file).exists();
                            let ok = !nk.file.trim().is_empty() && nk.pass == nk.pass2 && !exists;
                            if exists {
                                ui.label(RichText::new(tr("A key with this file name already exists.", "Es gibt schon einen Schlüssel mit diesem Dateinamen.")).size(12.0).color(theme::ORANGE()));
                            } else if nk.pass != nk.pass2 {
                                ui.label(RichText::new(tr("The passphrases don't match.", "Die Passphrasen stimmen nicht überein.")).size(12.0).color(theme::ORANGE()));
                            }
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let kind = if nk.kind == 0 { "ed25519" } else { "rsa" };
                                let mut shown = format!("ssh-keygen -t {kind}");
                                if nk.kind == 1 {
                                    shown.push_str(" -b 4096");
                                }
                                shown.push_str(&format!(" -C \"{}\" -f \"{}\" -N {}", nk.email, nk.file, if nk.pass.is_empty() { "\"\"" } else { "••••" }));
                                ui.label(RichText::new(&shown).monospace().size(10.5).color(theme::TEXT_FAINT())).on_hover_text(tr("This command creates the key pair. The passphrase is never shown.", "Dieser Befehl erzeugt das Schlüsselpaar. Die Passphrase wird nie angezeigt."));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if primary(ui, tr("Generate", "Erzeugen"), ok) {
                                        let mut args = vec!["-t".to_string(), kind.into()];
                                        if nk.kind == 1 {
                                            args.extend(["-b".into(), "4096".into()]);
                                        }
                                        args.extend(["-C".into(), nk.email.clone(), "-f".into(), nk.file.clone(), "-N".into(), nk.pass.clone()]);
                                        act = Some((tr("Generate SSH key", "SSH-Schlüssel erzeugen").into(), "ssh-keygen", args, shown.clone()));
                                    }
                                    if ui.button(tr("Back", "Zurück")).clicked() {
                                        back = true;
                                    }
                                });
                            });
                        } else if let Some(k) = st.sel.and_then(|i| st.keys.get(i)) {
                            egui::Grid::new("keydetails").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                                kv(ui, tr("Location:", "Ort:"), &native(&k.private), false);
                                kv(ui, "SHA256:", k.fingerprint.trim_start_matches("SHA256:"), true);
                            });
                            ui.add_space(4.0);
                            ui.label(RichText::new(tr("Public key:", "Öffentlicher Schlüssel:")).color(theme::TEXT_DIM()));
                            let mut t = k.pubtext.clone();
                            ui.add(egui::TextEdit::multiline(&mut t).font(egui::TextStyle::Monospace).desired_rows(3).desired_width(f32::INFINITY).interactive(false));
                            ui.horizontal(|ui| {
                                if ui.link(tr("Copy to clipboard", "In die Zwischenablage kopieren")).clicked() {
                                    ctx.copy_text(k.pubtext.clone());
                                }
                                ui.label(RichText::new("·").color(theme::TEXT_FAINT()));
                                ui.label(RichText::new(tr("Add it at:", "Hinzufügen bei:")).size(12.0).color(theme::TEXT_DIM()));
                                for (name, url) in [("GitHub", "https://github.com/settings/ssh/new"), ("GitLab", "https://gitlab.com/-/user_settings/ssh_keys"), ("Bitbucket", "https://bitbucket.org/account/settings/ssh-keys/")] {
                                    if ui.link(name).clicked() {
                                        open_url(url);
                                    }
                                }
                            });
                            ui.add_space(8.0);
                            rule(ui);
                            ui.add_space(6.0);
                            let key_path = native(&k.private);
                            let using = st.ssh_command.contains(&key_path) || st.ssh_command.contains(&k.private.display().to_string());
                            ui.horizontal_wrapped(|ui| {
                                let b = ui.button(tr("Add to ssh-agent", "Zum ssh-agent hinzufügen"));
                                if super::ui::tip_with_cmd(b, tr("Load the key so you don't have to type the passphrase each time", "Lädt den Schlüssel, damit die Passphrase nicht jedes Mal nötig ist"), &format!("ssh-add \"{key_path}\"")).clicked() {
                                    act = Some((tr("Add key to ssh-agent", "Schlüssel zum ssh-agent hinzufügen").into(), "ssh-add", vec![key_path.clone()], format!("ssh-add \"{key_path}\"")));
                                }
                                if !using {
                                    let cmdv = format!("ssh -i \"{}\" -o IdentitiesOnly=yes", key_path.replace('\\', "/"));
                                    let b = ui.button(tr("Use this key for Git", "Diesen Schlüssel für Git verwenden"));
                                    if super::ui::tip_with_cmd(b, tr("Git will always use this key for SSH connections", "Git verwendet diesen Schlüssel immer für SSH-Verbindungen"), &format!("git config --global core.sshCommand '{cmdv}'")).clicked() {
                                        act = Some((tr("Use key for Git", "Schlüssel für Git verwenden").into(), "git", vec!["config".into(), "--global".into(), "core.sshCommand".into(), cmdv.clone()], format!("git config --global core.sshCommand \"{cmdv}\"")));
                                    }
                                }
                                if !st.ssh_command.is_empty() {
                                    let b = ui.button(tr("Use system default", "Systemstandard verwenden"));
                                    if super::ui::tip_with_cmd(b, tr("Remove the custom SSH command", "Eigenen SSH-Befehl entfernen"), "git config --global --unset core.sshCommand").clicked() {
                                        act = Some((tr("Reset SSH command", "SSH-Befehl zurücksetzen").into(), "git", vec!["config".into(), "--global".into(), "--unset".into(), "core.sshCommand".into()], "git config --global --unset core.sshCommand".into()));
                                    }
                                }
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(tr("Test connection to", "Verbindung testen zu")).color(theme::TEXT_DIM()));
                                egui::ComboBox::from_id_salt("sshhost").width(150.0).selected_text(&st.host).show_ui(ui, |ui| {
                                    for h in ["github.com", "gitlab.com", "bitbucket.org", "ssh.dev.azure.com"] {
                                        ui.selectable_value(&mut st.host, h.to_string(), h);
                                    }
                                });
                                let target = format!("git@{}", st.host);
                                let b = ui.button(tr("Test", "Testen"));
                                if super::ui::tip_with_cmd(b, tr("Checks whether the server accepts your key", "Prüft, ob der Server deinen Schlüssel akzeptiert"), &format!("ssh -T {target}")).clicked() {
                                    act = Some((trs(format!("Test SSH to {}", st.host), format!("SSH-Test zu {}", st.host)), "ssh", vec!["-T".into(), "-o".into(), "BatchMode=yes".into(), "-o".into(), "StrictHostKeyChecking=accept-new".into(), target.clone()], format!("ssh -T {target}")));
                                }
                            });
                        } else {
                            ui.add_space(50.0);
                            ui.vertical_centered(|ui| {
                                icon(ui, Icon::Lock, 40.0, theme::TEXT_FAINT());
                                ui.label(RichText::new(tr("Generate a key with + or pick an existing one.", "Erzeuge mit + einen Schlüssel oder wähle einen vorhandenen.")).color(theme::TEXT_DIM()));
                            });
                        }
                        if back {
                            st.new_key = None;
                        }
                        job_line(ui, &self.jobs, st.job);
                    });
                });
                ui.add_space(10.0);
                rule(ui);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, "OK", true) {
                        close = true;
                    }
                });
            });
        if let Some((title, prog, args, display)) = act {
            let id = self.spawn_tool(title, prog, args, display, None, ctx);
            st.job = Some(id);
            if prog == "ssh-keygen" {
                st.new_key = None;
            }
        }
        if open && !close && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ssh = Some(st);
        }
    }

    // ----------------------------------------------------------------------------------- Accounts

    fn creds_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.creds.take() else { return };
        let mut open = true;
        let mut close = false;
        let mut act: Option<(String, Vec<String>, String, Option<String>)> = None;
        let mut add_account: Option<Account> = None;
        let mut remove_account: Option<usize> = None;
        egui::Window::new(RichText::new(tr("Accounts", "Konten")).font(FontId::new(14.0, theme::semibold())))
            .id(Id::unique("creds_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(dialog_frame(ctx))
            .show(ctx, |ui| {
                ui.set_width(680.0);
                heading(
                    ui,
                    tr("Accounts & sign-in", "Konten & Anmeldung"),
                    &[tr(
                        "For HTTPS remotes Git needs a username and a password or access token. They are kept by the credential helper, never by this app.",
                        "Für HTTPS-Remotes braucht Git einen Benutzernamen und ein Passwort oder Zugriffstoken. Gespeichert werden sie vom Credential-Helper, nie von dieser App.",
                    )],
                );
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        egui::Frame::new().fill(theme::PANEL_DARK()).stroke(egui::Stroke::new(1.0, theme::LINE())).corner_radius(3).show(ui, |ui| {
                            ui.set_width(200.0);
                            ui.set_min_height(220.0);
                            if self.accounts.is_empty() {
                                ui.add_space(80.0);
                                ui.vertical_centered(|ui| ui.label(RichText::new(tr("No accounts", "Keine Konten")).color(theme::TEXT_DIM())));
                            }
                            for (i, a) in self.accounts.iter().enumerate() {
                                let sel = st.sel == Some(i);
                                let (rect, resp) = ui.allocate_exact_size(Vec2::new(200.0, 40.0), egui::Sense::click());
                                let p = ui.painter_at(rect);
                                if sel {
                                    p.rect_filled(rect, 0, theme::ACCENT_DARK());
                                } else if resp.hovered() {
                                    p.rect_filled(rect, 0, theme::ROW_HOVER());
                                }
                                crate::icons::draw(&p, egui::Rect::from_center_size(egui::pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(18.0)), Icon::Cloud, theme::TEXT());
                                p.text(egui::pos2(rect.left() + 38.0, rect.top() + 13.0), Align2::LEFT_CENTER, &a.host, FontId::proportional(13.0), theme::TEXT_STRONG());
                                p.text(egui::pos2(rect.left() + 38.0, rect.bottom() - 12.0), Align2::LEFT_CENTER, &a.user, FontId::proportional(11.5), theme::TEXT_DIM());
                                if resp.clicked() {
                                    st.sel = Some(i);
                                    st.host = a.host.clone();
                                    st.user = a.user.clone();
                                    st.token.clear();
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            if icon_button(ui, Icon::Plus, tr("Add account", "Konto hinzufügen")).clicked() {
                                st.sel = None;
                                st.host = "github.com".into();
                                st.user.clear();
                                st.token.clear();
                            }
                            if let Some(i) = st.sel {
                                if icon_button(ui, Icon::Close, tr("Sign out and remove", "Abmelden und entfernen")).clicked() {
                                    let a = &self.accounts[i];
                                    let input = format!("protocol=https\nhost={}\nusername={}\n\n", a.host, a.user);
                                    act = Some((trs(format!("Sign out of {}", a.host), format!("Von {} abmelden", a.host)), vec!["credential".into(), "reject".into()], format!("git credential reject   (host={})", a.host), Some(input)));
                                    remove_account = Some(i);
                                }
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.set_width(450.0);
                        ui.label(RichText::new(if st.sel.is_some() { tr("Account", "Konto") } else { tr("Add an account", "Konto hinzufügen") }).font(FontId::new(13.0, theme::semibold())));
                        ui.add_space(4.0);
                        egui::Grid::new("cred_form").num_columns(2).spacing([10.0, 7.0]).show(ui, |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(tr("Server", "Server")).color(theme::TEXT_DIM())));
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut st.host).desired_width(260.0));
                                let r = icon_button(ui, Icon::ChevronDown, tr("Known servers", "Bekannte Server"));
                                egui::Popup::menu(&r).show(|ui| {
                                    for h in ["github.com", "gitlab.com", "bitbucket.org", "dev.azure.com"] {
                                        if ui.button(h).clicked() {
                                            st.host = h.into();
                                            ui.close();
                                        }
                                    }
                                });
                            });
                            ui.end_row();
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(tr("Username", "Benutzername")).color(theme::TEXT_DIM())));
                            ui.add(egui::TextEdit::singleline(&mut st.user).desired_width(290.0));
                            ui.end_row();
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(tr("Access token", "Zugriffstoken")).color(theme::TEXT_DIM())));
                            ui.add(egui::TextEdit::singleline(&mut st.token).password(true).desired_width(290.0).hint_text(tr("token or password", "Token oder Passwort")));
                            ui.end_row();
                            ui.label("");
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(tr("Create a token:", "Token erstellen:")).size(11.5).color(theme::TEXT_FAINT()));
                                for (name, url) in [("GitHub", "https://github.com/settings/tokens/new?scopes=repo&description=YAGUI"), ("GitLab", "https://gitlab.com/-/user_settings/personal_access_tokens"), ("Bitbucket", "https://bitbucket.org/account/settings/app-passwords/")] {
                                    if ui.link(RichText::new(name).size(11.5)).clicked() {
                                        open_url(url);
                                    }
                                }
                            });
                            ui.end_row();
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            // Without a credential helper, Git would not keep the sign-in.
                            let can = !st.host.trim().is_empty() && !st.user.trim().is_empty() && !st.token.is_empty() && !st.helper.is_empty();
                            if primary(ui, tr("Save sign-in", "Anmeldung speichern"), can) {
                                let input = format!("protocol=https\nhost={}\nusername={}\npassword={}\n\n", st.host.trim(), st.user.trim(), st.token);
                                act = Some((trs(format!("Save sign-in for {}", st.host.trim()), format!("Anmeldung für {} speichern", st.host.trim())), vec!["credential".into(), "approve".into()], format!("git credential approve   (host={}, username={}, password=••••)", st.host.trim(), st.user.trim()), Some(input)));
                                add_account = Some(Account { host: st.host.trim().into(), user: st.user.trim().into() });
                                st.token.clear();
                            }
                            let url = format!("https://{}", st.host.trim());
                            let b = ui.add_enabled(!st.host.trim().is_empty(), egui::Button::new(tr("Test", "Testen")));
                            if super::ui::tip_with_cmd(b, tr("Ask the server for its branches using the saved sign-in", "Fragt den Server mit der gespeicherten Anmeldung nach seinen Branches"), &format!("git ls-remote {url}/<repo>")).clicked() {
                                // Test against the current repository's remote on that host, if any.
                                let target = self.tabs.get(self.active).and_then(|r| r.remotes.iter().find(|(_, u)| u.contains(st.host.trim())).map(|(_, u)| u.clone()));
                                if let Some(t) = target {
                                    act = Some((trs(format!("Test access to {}", st.host.trim()), format!("Zugriff auf {} testen", st.host.trim())), vec!["ls-remote".into(), "--heads".into(), t.clone()], format!("git ls-remote --heads {t}"), None));
                                }
                            }
                        });
                        ui.add_space(4.0);
                        if st.helper.is_empty() {
                            ui.label(RichText::new(tr("No credential helper is set (below), so Git would not keep a sign-in.", "Es ist kein Credential-Helper eingestellt (unten), daher würde Git keine Anmeldung behalten.")).size(11.5).color(theme::ORANGE()));
                        }
                        ui.label(RichText::new(tr("The token is handed to Git once and not kept here. With Git Credential Manager a browser sign-in also opens by itself on the first push or pull.", "Das Token wird einmal an Git übergeben und hier nicht gespeichert. Mit dem Git Credential Manager öffnet sich beim ersten Push oder Pull auch von selbst eine Anmeldung im Browser.")).size(11.5).color(theme::TEXT_FAINT()));
                        job_line(ui, &self.jobs, st.job);
                    });
                });
                ui.add_space(10.0);
                rule(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Credential helper", "Credential-Helper")).color(theme::TEXT_DIM()));
                    let before = st.helper_sel;
                    let shown = match st.helper_sel {
                        Some(i) => HELPERS[i].1.to_string(),
                        None => st.helper.clone(),
                    };
                    egui::ComboBox::from_id_salt("helper").width(200.0).selected_text(shown).show_ui(ui, |ui| {
                        for (i, h) in HELPERS.iter().enumerate() {
                            ui.selectable_value(&mut st.helper_sel, Some(i), h.1).on_hover_text(helper_help(h));
                        }
                    });
                    if let Some(i) = st.helper_sel.filter(|_| before != st.helper_sel) {
                        // An empty value first: it clears the helpers set system-wide (Git for
                        // Windows, Apple's, Homebrew's and MacPorts' Git set one there), which Git
                        // would otherwise ask first. Then the chosen one, as Git Credential Manager
                        // and the GitHub CLI do it themselves.
                        let h = HELPERS[i].0;
                        let reset = "git config --global --replace-all credential.helper \"\"";
                        let reset_args: Vec<String> = vec!["config".into(), "--global".into(), "--replace-all".into(), "credential.helper".into(), String::new()];
                        let turn_off = (tr("Turn off credential helper", "Credential-Helper ausschalten").to_string(), reset_args, reset.to_string(), None);
                        if h.is_empty() {
                            act = Some(turn_off);
                            st.helper.clear();
                        } else if run_capture("git", &["config", "--global", "--replace-all", "credential.helper", ""]).0 == 0 {
                            act = Some((tr("Set credential helper", "Credential-Helper festlegen").into(), vec!["config".into(), "--global".into(), "--add".into(), "credential.helper".into(), h.into()], format!("{reset} && git config --global --add credential.helper {h}"), None));
                            st.helper = h.to_string();
                        } else {
                            // Run the reset as the job instead, so its error shows in Activity.
                            act = Some(turn_off);
                        }
                    }
                    if let Some(i) = st.helper_sel {
                        ui.label(RichText::new(helper_help(&HELPERS[i])).size(11.5).color(theme::TEXT_FAINT()));
                    }
                });
                if !st.helper.is_empty() {
                    ui.label(RichText::new(trs(format!("Currently: {}", st.helper), format!("Aktuell: {}", st.helper))).size(11.0).color(theme::TEXT_FAINT()));
                }
                ui.add_space(6.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, tr("Close", "Schließen"), true) {
                        close = true;
                    }
                });
            });
        if let Some((title, args, display, stdin)) = act {
            let id = self.spawn_tool(title, "git", args, display, stdin, ctx);
            st.job = Some(id);
        }
        if let Some(a) = add_account {
            if !self.accounts.iter().any(|x| x.host == a.host && x.user == a.user) {
                self.accounts.push(a);
                st.sel = Some(self.accounts.len() - 1);
            }
            self.save();
        }
        if let Some(i) = remove_account {
            self.accounts.remove(i);
            st.sel = None;
            self.save();
        }
        if open && !close && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.creds = Some(st);
        }
    }

    // ----------------------------------------------------------------------------------- Preferences

    fn prefs_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.prefs.take() else { return };
        let mut open = true;
        let mut close = false;
        let mut act: Option<(String, Vec<String>, String)> = None;
        let mut lang: Option<Lang> = None;
        let mut open_ssh = false;
        let mut open_creds = false;
        let mut theme_change: Option<bool> = None;
        let mut rescan = false;
        let mut cfg_cmds: Vec<(String, Vec<String>, String)> = Vec::new();
        let topo_before = self.p.topo;
        egui::Window::new(RichText::new(tr("Preferences", "Einstellungen")).font(FontId::new(14.0, theme::semibold())))
            .id(Id::unique("prefs_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(dialog_frame(ctx))
            .show(ctx, |ui| {
                ui.set_width(640.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    for (i, t) in [tr("General", "Allgemein"), tr("Commit", "Commit"), "Git", tr("Integration", "Integration"), tr("Custom Commands", "Eigene Befehle"), tr("Sign-in", "Anmeldung")].iter().enumerate() {
                        let sel = st.tab == i;
                        let r = ui.add(egui::Button::new(RichText::new(*t).font(FontId::new(13.0, theme::semibold())).color(if sel { theme::ACCENT() } else { theme::TEXT_DIM() })).frame(false));
                        if sel {
                            let rr = r.rect;
                            ui.painter().line_segment([rr.left_bottom() + Vec2::new(0.0, 5.0), rr.right_bottom() + Vec2::new(0.0, 5.0)], egui::Stroke::new(2.0, theme::ACCENT()));
                        }
                        if r.clicked() {
                            st.tab = i;
                        }
                    }
                });
                ui.add_space(6.0);
                rule(ui);
                ui.add_space(12.0);
                if st.tab == 4 {
                    super::extra::custom_cmds_editor(ui, &mut self.custom_cmds, &mut st.cc_sel, "prefs_cc");
                }
                egui::ScrollArea::vertical().id_salt("prefs_scroll").max_height(470.0).show(ui, |ui| {
                egui::Grid::new("prefs").num_columns(2).spacing([14.0, 9.0]).min_col_width(170.0).show(ui, |ui| match st.tab {
                    0 => {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(tr("Language", "Sprache")).color(theme::TEXT_DIM())));
                        ui.horizontal(|ui| {
                            for (l, name) in [(Lang::En, "English"), (Lang::De, "Deutsch")] {
                                if ui.radio(self.lang == l, name).clicked() {
                                    lang = Some(l);
                                }
                            }
                        });
                        ui.end_row();
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(tr("Terminal", "Terminal")).color(theme::TEXT_DIM())));
                        egui::ComboBox::from_id_salt("shell").width(240.0).selected_text(self.shell.label()).show_ui(ui, |ui| {
                            for s in ShellTool::ALL {
                                ui.selectable_value(&mut self.shell, s, s.label());
                            }
                        });
                        ui.end_row();
                        let lbl = |ui: &mut egui::Ui, t: &str| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(t).color(theme::TEXT_DIM())));
                        };
                        lbl(ui, tr("Theme", "Farbschema"));
                        ui.horizontal(|ui| {
                            if ui.radio(!crate::theme::is_light(), tr("Dark", "Dunkel")).clicked() {
                                theme_change = Some(false);
                            }
                            if ui.radio(crate::theme::is_light(), tr("Light", "Hell")).clicked() {
                                theme_change = Some(true);
                            }
                        });
                        ui.end_row();
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| ui.label(RichText::new(tr("Source code folders", "Quellcode-Ordner")).color(theme::TEXT_DIM())));
                        ui.vertical(|ui| {
                            let mut remove = None;
                            for (i, d) in self.src_dirs.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(native(d)).size(12.5));
                                    if ui.small_button("×").on_hover_text(tr("Remove", "Entfernen")).clicked() {
                                        remove = Some(i);
                                    }
                                });
                            }
                            if let Some(i) = remove {
                                self.src_dirs.remove(i);
                                rescan = true;
                            }
                            if ui.button(tr("Add Folder…", "Ordner hinzufügen…")).on_hover_text(tr("The Repository Manager finds the repositories in these folders", "Der Repository Manager findet die Repositories in diesen Ordnern")).clicked() {
                                if let Some(f) = rfd::FileDialog::new().pick_folder() {
                                    self.src_dirs.push(f);
                                    rescan = true;
                                }
                            }
                        });
                        ui.end_row();
                        lbl(ui, tr("Sort commits", "Commits sortieren"));
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.p.topo, true, tr("Topologically", "Nach Abstammung"));
                            ui.radio_value(&mut self.p.topo, false, tr("By date", "Nach Datum"));
                        });
                        ui.end_row();
                        lbl(ui, tr("Fetch", "Fetch"));
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.p.auto_fetch, tr("Fetch remotes automatically every", "Remotes automatisch holen alle"));
                                ui.add_enabled(self.p.auto_fetch, egui::DragValue::new(&mut self.p.fetch_minutes).range(1..=240).suffix(" min"));
                            });
                            ui.checkbox(&mut self.p.fetch_tags, tr("Always include tags into fetch", "Tags immer mitholen"));
                        });
                        ui.end_row();
                        lbl(ui, tr("Tabs", "Tabs"));
                        ui.checkbox(&mut self.prefs_tab_indicator, tr("Tab indicator for uncommitted changes", "Punkt im Tab bei nicht committeten Änderungen"));
                        ui.end_row();
                        lbl(ui, tr("Submodules", "Submodule"));
                        let before = st.submodule_recurse;
                        ui.checkbox(&mut st.submodule_recurse, tr("Update submodules automatically on checkout", "Submodule beim Auschecken mit aktualisieren"));
                        if before != st.submodule_recurse {
                            let v = if st.submodule_recurse { "true" } else { "false" };
                            cfg_cmds.push((tr("Submodules on checkout", "Submodule beim Auschecken").into(), vec!["config".into(), "--global".into(), "submodule.recurse".into(), v.into()], format!("git config --global submodule.recurse {v}")));
                        }
                        ui.end_row();
                        lbl(ui, tr("Space in branch names", "Leerzeichen in Branch-Namen"));
                        egui::ComboBox::from_id_salt("space_char").selected_text(match self.p.space_char.as_str() { "-" => "-", "_" => "_", _ => tr("keep", "beibehalten") }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.p.space_char, "-".into(), tr("replace with -", "durch - ersetzen"));
                            ui.selectable_value(&mut self.p.space_char, "_".into(), tr("replace with _", "durch _ ersetzen"));
                            ui.selectable_value(&mut self.p.space_char, String::new(), tr("keep", "beibehalten"));
                        });
                        ui.end_row();
                        lbl(ui, tr("Commit list", "Commit-Liste"));
                        ui.checkbox(&mut self.p.compact_labels, tr("Compact branch labels", "Kompakte Branch-Beschriftungen")).on_hover_text(tr("One label when a branch and its server copy point at the same commit", "Ein Label, wenn Branch und Server-Kopie auf denselben Commit zeigen"));
                        ui.end_row();
                        lbl(ui, tr("Diff font size", "Schriftgröße der Unterschiede"));
                        ui.add(egui::DragValue::new(&mut self.p.diff_font).range(8.0..=24.0).speed(0.25));
                        ui.end_row();
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| ui.label(RichText::new(tr("Editors found", "Gefundene Editoren")).color(theme::TEXT_DIM())));
                        ui.vertical(|ui| {
                            if self.editors.is_empty() {
                                ui.label(RichText::new(tr("none", "keine")).color(theme::TEXT_FAINT()));
                            }
                            for e in &self.editors {
                                ui.label(RichText::new(&e.name).size(12.5)).on_hover_text(native(&e.exe));
                            }
                        });
                        ui.end_row();
                    }
                    4 => {}
                    1 => {
                        let lbl = |ui: &mut egui::Ui, t: &str| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(t).color(theme::TEXT_DIM())));
                        };
                        lbl(ui, tr("Subject length", "Länge des Titels"));
                        ui.horizontal(|ui| {
                            ui.label(tr("fine up to", "gut bis"));
                            ui.add(egui::DragValue::new(&mut self.p.subject_low).range(20..=200));
                            ui.label(tr("warn after", "Warnung ab"));
                            ui.add(egui::DragValue::new(&mut self.p.subject_high).range(20..=200));
                        });
                        ui.end_row();
                        lbl(ui, tr("Page guide at column", "Hilfslinie bei Spalte"));
                        ui.add(egui::DragValue::new(&mut self.p.page_guide).range(40..=200));
                        ui.end_row();
                        lbl(ui, tr("After committing", "Nach dem Commit"));
                        ui.checkbox(&mut self.p.push_on_commit, tr("Push automatically on commit", "Automatisch pushen"));
                        ui.end_row();
                    }
                    3 => {
                        let lbl = |ui: &mut egui::Ui, t: &str| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(t).color(theme::TEXT_DIM())));
                        };
                        lbl(ui, tr("Issue tracker", "Issue-Tracker"));
                        ui.checkbox(&mut self.p.issue_links, tr("Highlight issue tracker links in commit messages", "Issue-Links in Commit-Beschreibungen hervorheben"));
                        ui.end_row();
                        for (which, key) in [(0, "merge"), (1, "diff")] {
                            lbl(ui, if which == 0 { tr("External merge tool", "Externes Merge-Programm") } else { tr("External diff tool", "Externes Vergleichsprogramm") });
                            let cur = if which == 0 { st.merge_tool.clone() } else { st.diff_tool.clone() };
                            let shown = TOOLS.iter().find(|t| t.0 == cur).map(|t| t.1.to_string()).unwrap_or_else(|| if cur.is_empty() { tr("none", "keins").to_string() } else { cur.clone() });
                            egui::ComboBox::from_id_salt(("tool", key)).width(240.0).selected_text(shown).show_ui(ui, |ui| {
                                for (id, name, mcmd, dcmd) in TOOLS {
                                    if ui.selectable_label(cur == id, name).clicked() && cur != id {
                                        let tool_key = if which == 0 { "merge.tool" } else { "diff.tool" };
                                        cfg_cmds.push((format!("{key}: {name}"), vec!["config".into(), "--global".into(), tool_key.into(), id.into()], format!("git config --global {tool_key} {id}")));
                                        let cmd = if which == 0 { mcmd } else { dcmd };
                                        if !cmd.is_empty() {
                                            let k = if which == 0 { format!("mergetool.{id}.cmd") } else { format!("difftool.{id}.cmd") };
                                            cfg_cmds.push((format!("{key}: {name}"), vec!["config".into(), "--global".into(), k.clone(), cmd.into()], format!("git config --global {k} '{cmd}'")));
                                        }
                                        if which == 0 {
                                            st.merge_tool = id.into();
                                        } else {
                                            st.diff_tool = id.into();
                                        }
                                    }
                                }
                            });
                            ui.end_row();
                        }
                        lbl(ui, tr("Terminal", "Terminal"));
                        egui::ComboBox::from_id_salt("shell2").width(240.0).selected_text(self.shell.label()).show_ui(ui, |ui| {
                            for s in ShellTool::ALL {
                                ui.selectable_value(&mut self.shell, s, s.label());
                            }
                        });
                        ui.end_row();
                    }
                    2 => {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(tr("Output", "Ausgabe")).color(theme::TEXT_DIM())));
                        ui.checkbox(&mut self.p.verbose, tr("Enable verbose Git output", "Ausführliche Git-Ausgabe")).on_hover_text("GIT_TRACE=1");
                        ui.end_row();
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(tr("Git", "Git")).color(theme::TEXT_DIM())));
                        ui.label(RichText::new(&st.git_version).size(12.5));
                        ui.end_row();
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(tr("Your name", "Dein Name")).color(theme::TEXT_DIM())));
                        ui.add(egui::TextEdit::singleline(&mut st.name).desired_width(300.0));
                        ui.end_row();
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(tr("Your e-mail", "Deine E-Mail")).color(theme::TEXT_DIM())));
                        ui.add(egui::TextEdit::singleline(&mut st.email).desired_width(300.0));
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            if primary(ui, tr("Save", "Speichern"), !st.name.trim().is_empty() && !st.email.trim().is_empty()) {
                                act = Some((tr("Set global user", "Globalen Benutzer setzen").into(), vec!["config".into(), "--global".into(), "user.name".into(), st.name.trim().into()], format!("git config --global user.name \"{}\"", st.name.trim())));
                            }
                            ui.label(RichText::new(tr("Shown in every commit you make.", "Steht in jedem Commit, den du erstellst.")).size(11.5).color(theme::TEXT_FAINT()));
                        });
                        ui.end_row();
                    }
                    _ => {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new("SSH").color(theme::TEXT_DIM())));
                        ui.horizontal(|ui| {
                            if ui.button(tr("Configure SSH Keys…", "SSH-Schlüssel einrichten…")).clicked() {
                                open_ssh = true;
                            }
                            ui.label(RichText::new(tr("for git@… remotes", "für git@…-Remotes")).size(11.5).color(theme::TEXT_FAINT()));
                        });
                        ui.end_row();
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new("HTTPS").color(theme::TEXT_DIM())));
                        ui.horizontal(|ui| {
                            if ui.button(tr("Accounts…", "Konten…")).clicked() {
                                open_creds = true;
                            }
                            ui.label(RichText::new(tr("for https://… remotes", "für https://…-Remotes")).size(11.5).color(theme::TEXT_FAINT()));
                        });
                        ui.end_row();
                    }
                });
                });
                job_line(ui, &self.jobs, st.job);
                ui.add_space(10.0);
                rule(ui);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, tr("Close", "Schließen"), true) {
                        close = true;
                    }
                });
            });
        if let Some(l) = theme_change {
            crate::theme::set_light(l);
            crate::theme::apply_visuals(ctx);
        }
        if rescan {
            self.start_scan();
        }
        for (title, args, display) in cfg_cmds {
            st.job = Some(self.spawn_tool(title, "git", args, display, None, ctx));
        }
        if topo_before != self.p.topo {
            let topo = self.p.topo;
            for t in self.tabs.iter_mut() {
                t.log_opts.topo = topo;
                t.reload(crate::git::RELOAD_REFS);
            }
        }
        self.apply_repo_view();
        if let Some(l) = lang {
            self.lang = l;
            crate::i18n::set(l);
        }
        if let Some((title, args, display)) = act {
            st.job = Some(self.spawn_tool(title, "git", args, display, None, ctx));
            let e = st.email.trim().to_string();
            self.spawn_tool(tr("Set global e-mail", "Globale E-Mail setzen").into(), "git", vec!["config".into(), "--global".into(), "user.email".into(), e.clone()], format!("git config --global user.email \"{e}\""), None, ctx);
        }
        if open_ssh {
            self.ssh = Some(SshState::new(&self.extra_keys));
        }
        if open_creds {
            self.creds = Some(CredState::new());
        }
        if open && !close && !ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.prefs = Some(st);
        } else {
            self.save();
        }
    }
}

fn git_global(dir: &Path, key: &str) -> String {
    crate::git::run(dir, &["config", "--get", key]).stdout.trim().to_string()
}

/// Turns an HTTPS remote URL into its SSH form and back (github.com/user/repo).
pub fn switch_remote_url(url: &str) -> Option<String> {
    if let Some(rest) = url.strip_prefix("https://") {
        let rest = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
        let (host, path) = rest.split_once('/')?;
        Some(format!("git@{host}:{path}"))
    } else if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        Some(format!("https://{host}/{path}"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helpers_in_effect_come_from_system_and_global_settings() {
        // Apple's Git: osxkeychain in its own gitconfig (scope "unknown"), nothing global.
        assert_eq!(effective_helpers("unknown\tosxkeychain\n"), vec!["osxkeychain"]);
        // Homebrew's Git: osxkeychain system-wide.
        assert_eq!(effective_helpers("system\tosxkeychain\n"), vec!["osxkeychain"]);
        // A helper chosen in Accounts: an empty value clears the others, then the chosen one.
        assert_eq!(effective_helpers("unknown\tosxkeychain\nglobal\t\nglobal\tmanager\n"), vec!["manager"]);
        // Git for Windows' manager, switched off globally with an empty value.
        assert!(effective_helpers("system\tmanager\nglobal\t\n").is_empty());
        // A repository's own setting does not count for the Accounts dialog.
        assert_eq!(effective_helpers("system\tosxkeychain\nlocal\tstore\n"), vec!["osxkeychain"]);
        assert_eq!(effective_helpers("global\t\nglobal\tcache --timeout=600\n"), vec!["cache --timeout=600"]);
        // An old Git that does not know --show-scope prints an error: no helper read.
        assert!(effective_helpers("error: unknown option `show-scope'\n").is_empty());
    }

    #[test]
    fn a_configured_helper_is_found_by_its_name() {
        let pos = |v: &str| HELPERS.iter().position(|h| h.0 == v);
        assert_eq!(helper_index("/usr/local/share/gcm-core/git-credential-manager"), pos("manager"));
        assert_eq!(helper_index("store --file ~/.my-credentials"), pos("store"));
        assert_eq!(helper_index(""), pos(""));
        assert_eq!(helper_index("libsecret"), None);
        #[cfg(target_os = "macos")]
        assert_eq!(helper_index("osxkeychain"), pos("osxkeychain"));
    }
}
