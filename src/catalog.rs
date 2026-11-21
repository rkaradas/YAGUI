//! Every git command the app offers, with a plain-language explanation of what it does
//! and of each option. A command is turned into an argument list by `build_args`.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Danger {
    /// Only reads, never changes anything.
    Read,
    /// Changes the repository, but nothing is lost.
    Change,
    /// Can throw away work or rewrite history.
    Risky,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Normal git command, run and wait.
    Normal,
    /// Opens its own window or keeps running (gitk, git gui, daemon...).
    External,
    /// Opens the app's own interactive rebase editor.
    InteractiveRebase,
    /// Free command line.
    Free,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cat {
    Setup,
    Changes,
    Branches,
    Integrate,
    Remote,
    Stash,
    Tags,
    History,
    Modules,
    Patches,
    Maintain,
    Lfs,
    Flow,
    Settings,
    Expert,
    Tools,
}

pub const CATS: &[(Cat, &str, &str)] = &[
    (Cat::Setup, "Start", "Repository anlegen oder herunterladen"),
    (Cat::Changes, "Änderungen & Commit", "Dateien vormerken, speichern, verwerfen"),
    (Cat::Branches, "Branches", "Entwicklungszweige anlegen, wechseln, löschen"),
    (Cat::Integrate, "Zusammenführen", "Merge, Rebase, Cherry-Pick, Revert"),
    (Cat::Remote, "Server & Sync", "Fetch, Pull, Push und Remotes"),
    (Cat::Stash, "Stash", "Änderungen zwischenlagern"),
    (Cat::Tags, "Tags", "Versionen markieren"),
    (Cat::History, "Verlauf & Suche", "Log, Blame, Suche, Bisect"),
    (Cat::Modules, "Submodule & Worktrees", "Eingebettete Repositories, mehrere Arbeitsordner"),
    (Cat::Patches, "Patches & Austausch", "Patches, Archive, Bundles, E-Mail"),
    (Cat::Maintain, "Wartung", "Aufräumen, prüfen, optimieren"),
    (Cat::Lfs, "Git LFS", "Große Dateien"),
    (Cat::Flow, "Git Flow", "Feature/Release/Hotfix-Arbeitsablauf"),
    (Cat::Settings, "Einstellungen & Hilfe", "Konfiguration, Hilfe, Version"),
    (Cat::Expert, "Experten (Plumbing)", "Interne Bausteine von Git"),
    (Cat::Tools, "Externe Werkzeuge", "Programme mit eigenem Fenster, freie Eingabe"),
];

#[derive(Clone, Debug)]
pub enum P {
    /// A text value. `prefix` is emitted before the value ("-m"); a prefix ending in `=` or `.`
    /// is joined to the value. `multi` splits the value at spaces into several arguments.
    Text {
        key: &'static str,
        label: &'static str,
        prefix: &'static str,
        hint: &'static str,
        help: &'static str,
        multi: bool,
        required: bool,
    },
    /// A checkbox that adds `args` when ticked.
    Flag {
        args: &'static str,
        label: &'static str,
        help: &'static str,
        default: bool,
    },
    /// One of several options: (args, label, help).
    Choice {
        key: &'static str,
        label: &'static str,
        options: &'static [(&'static str, &'static str, &'static str)],
    },
    /// A fixed argument that is always added.
    Lit(&'static str, &'static str),
    /// Text sent to the command's standard input.
    Stdin { label: &'static str, hint: &'static str },
}

#[derive(Clone, Debug)]
pub struct Cmd {
    pub id: &'static str,
    pub cat: Cat,
    pub title: &'static str,
    pub base: &'static str,
    pub danger: Danger,
    pub summary: &'static str,
    pub params: Vec<P>,
    pub kind: Kind,
    pub needs_repo: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Bool(bool),
    Choice(usize),
    None,
}

pub fn default_values(cmd: &Cmd) -> Vec<Value> {
    cmd.params
        .iter()
        .map(|p| match p {
            P::Text { .. } | P::Stdin { .. } => Value::Text(String::new()),
            P::Flag { default, .. } => Value::Bool(*default),
            P::Choice { .. } => Value::Choice(0),
            P::Lit(..) => Value::None,
        })
        .collect()
}

/// One piece of the final command line with an explanation of what it means.
#[derive(Clone, Debug)]
pub struct Arg {
    pub text: String,
    pub help: String,
}

/// Splits a string like a simple shell: spaces separate, "double" or 'single' quotes group.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for ch in s.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => cur.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                has = true;
            }
            None if ch.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            None => cur.push(ch),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn build_args(cmd: &Cmd, values: &[Value], extra: &str) -> Vec<Arg> {
    let mut out = Vec::new();
    for (i, part) in split_args(cmd.base).into_iter().enumerate() {
        let help = if i == 0 {
            format!("Git-Befehl „{}“: {}", part, cmd.title)
        } else if part.starts_with('-') {
            format!("Fest eingestellte Option dieses Befehls ({})", cmd.title)
        } else {
            format!("Unterbefehl „{part}“")
        };
        out.push(Arg { text: part, help });
    }
    for (p, v) in cmd.params.iter().zip(values.iter()) {
        match (p, v) {
            (P::Text { prefix, label, help, multi, .. }, Value::Text(t)) => {
                let t = t.trim();
                if t.is_empty() {
                    continue;
                }
                let help = if help.is_empty() { label.to_string() } else { format!("{label}: {help}") };
                let vals = if *multi { split_args(t) } else { vec![t.to_string()] };
                if prefix.ends_with('=') || prefix.ends_with('.') {
                    for v in vals {
                        out.push(Arg { text: format!("{prefix}{v}"), help: help.clone() });
                    }
                } else {
                    if !prefix.is_empty() {
                        out.push(Arg { text: prefix.to_string(), help: help.clone() });
                    }
                    for v in vals {
                        out.push(Arg { text: v, help: help.clone() });
                    }
                }
            }
            (P::Flag { args, label, help, .. }, Value::Bool(true)) => {
                let h = if help.is_empty() { label.to_string() } else { format!("{label}: {help}") };
                for a in split_args(args) {
                    out.push(Arg { text: a, help: h.clone() });
                }
            }
            (P::Choice { label, options, .. }, Value::Choice(c)) => {
                if let Some((args, olabel, ohelp)) = options.get(*c) {
                    for a in split_args(args) {
                        out.push(Arg { text: a, help: format!("{label}: {olabel} – {ohelp}") });
                    }
                }
            }
            (P::Lit(a, help), _) => {
                for x in split_args(a) {
                    out.push(Arg { text: x, help: help.to_string() });
                }
            }
            _ => {}
        }
    }
    for a in split_args(extra) {
        out.push(Arg { text: a, help: "Zusätzliche Option, die du selbst eingetragen hast".into() });
    }
    out
}

pub fn stdin_of(cmd: &Cmd, values: &[Value]) -> Option<String> {
    for (p, v) in cmd.params.iter().zip(values.iter()) {
        if let (P::Stdin { .. }, Value::Text(t)) = (p, v) {
            if !t.is_empty() {
                let mut s = t.clone();
                if !s.ends_with('\n') {
                    s.push('\n');
                }
                return Some(s);
            }
        }
    }
    None
}

/// Required fields that are still empty.
pub fn missing(cmd: &Cmd, values: &[Value]) -> Vec<&'static str> {
    cmd.params
        .iter()
        .zip(values.iter())
        .filter_map(|(p, v)| match (p, v) {
            (P::Text { required: true, label, .. }, Value::Text(t)) if t.trim().is_empty() => Some(*label),
            _ => None,
        })
        .collect()
}

pub fn quote_for_display(a: &str) -> String {
    if a.is_empty() {
        "\"\"".into()
    } else if a.chars().any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '&' || c == '|') {
        format!("\"{}\"", a.replace('"', "\\\""))
    } else {
        a.to_string()
    }
}

pub fn command_line(args: &[Arg]) -> String {
    let mut s = String::from("git");
    for a in args {
        s.push(' ');
        s.push_str(&quote_for_display(&a.text));
    }
    s
}

pub fn set_text(cmd: &Cmd, values: &mut [Value], key: &str, val: &str) {
    for (p, v) in cmd.params.iter().zip(values.iter_mut()) {
        if let P::Text { key: k, .. } = p {
            if *k == key {
                *v = Value::Text(val.to_string());
                return;
            }
        }
    }
}

pub fn set_flag(cmd: &Cmd, values: &mut [Value], args: &str, on: bool) {
    for (p, v) in cmd.params.iter().zip(values.iter_mut()) {
        if let P::Flag { args: a, .. } = p {
            if *a == args {
                *v = Value::Bool(on);
                return;
            }
        }
    }
}

pub fn set_choice(cmd: &Cmd, values: &mut [Value], args: &str) {
    for (p, v) in cmd.params.iter().zip(values.iter_mut()) {
        if let P::Choice { options, .. } = p {
            if let Some(i) = options.iter().position(|o| o.0 == args) {
                *v = Value::Choice(i);
                return;
            }
        }
    }
}

pub fn has_text_key(cmd: &Cmd, key: &str) -> bool {
    cmd.params.iter().any(|p| matches!(p, P::Text { key: k, .. } if *k == key))
}

// ---------------------------------------------------------------------------------------------
// Builders

fn t(key: &'static str, label: &'static str, hint: &'static str, help: &'static str) -> P {
    P::Text { key, label, prefix: "", hint, help, multi: false, required: true }
}
fn o(key: &'static str, label: &'static str, hint: &'static str, help: &'static str) -> P {
    P::Text { key, label, prefix: "", hint, help, multi: false, required: false }
}
fn m(key: &'static str, label: &'static str, hint: &'static str, help: &'static str) -> P {
    P::Text { key, label, prefix: "", hint, help, multi: true, required: false }
}
fn mr(key: &'static str, label: &'static str, hint: &'static str, help: &'static str) -> P {
    P::Text { key, label, prefix: "", hint, help, multi: true, required: true }
}
fn v(prefix: &'static str, key: &'static str, label: &'static str, hint: &'static str, help: &'static str) -> P {
    P::Text { key, label, prefix, hint, help, multi: false, required: false }
}
fn vr(prefix: &'static str, key: &'static str, label: &'static str, hint: &'static str, help: &'static str) -> P {
    P::Text { key, label, prefix, hint, help, multi: false, required: true }
}
fn paths() -> P {
    P::Text {
        key: "paths",
        label: "Nur diese Dateien/Ordner",
        prefix: "--",
        hint: "leer = alle",
        help: "beschränkt den Befehl auf diese Pfade (mit Leerzeichen trennen)",
        multi: true,
        required: false,
    }
}
fn f(args: &'static str, label: &'static str, help: &'static str) -> P {
    P::Flag { args, label, help, default: false }
}
fn fon(args: &'static str, label: &'static str, help: &'static str) -> P {
    P::Flag { args, label, help, default: true }
}
fn ch(key: &'static str, label: &'static str, options: &'static [(&'static str, &'static str, &'static str)]) -> P {
    P::Choice { key, label, options }
}
fn lit(a: &'static str, help: &'static str) -> P {
    P::Lit(a, help)
}
fn stdin(label: &'static str, hint: &'static str) -> P {
    P::Stdin { label, hint }
}

const SCOPE: &[(&str, &str, &str)] = &[
    ("", "Nur dieses Repository", "gilt nur für das geöffnete Repository (.git/config)"),
    ("--global", "Für mich (global)", "gilt für alle deine Repositories (~/.gitconfig)"),
    ("--system", "Für alle Benutzer (System)", "gilt für jeden auf diesem Computer"),
];

pub const RESET_MODES: &[(&str, &str, &str)] = &[
    ("--mixed", "Gemischt (Standard)", "Branch zurücksetzen, Änderungen bleiben als nicht vorgemerkte Dateien erhalten"),
    ("--soft", "Weich", "Branch zurücksetzen, Änderungen bleiben vorgemerkt (gestaged)"),
    ("--hard", "Hart – verwirft Änderungen!", "Branch und alle Dateien zurücksetzen. Ungespeicherte Arbeit geht verloren"),
    ("--keep", "Behalten", "wie hart, bricht aber ab, wenn lokale Änderungen verloren gingen"),
    ("--merge", "Merge", "setzt zurück, behält aber Änderungen, die nicht vorgemerkt sind"),
];

pub const MERGE_MODES: &[(&str, &str, &str)] = &[
    ("", "Standard", "Fast-Forward wenn möglich, sonst Merge-Commit"),
    ("--no-ff", "Immer Merge-Commit", "erstellt immer einen Merge-Commit, auch wenn Fast-Forward ginge"),
    ("--ff-only", "Nur Fast-Forward", "bricht ab, wenn ein Merge-Commit nötig wäre"),
    ("--squash", "Squash", "fasst alle Änderungen zusammen, ohne Commit; du committest danach selbst"),
];

const PULL_MODES: &[(&str, &str, &str)] = &[
    ("", "Standard (laut Einstellung)", "verwendet die Einstellung pull.rebase / pull.ff"),
    ("--rebase", "Rebase", "deine lokalen Commits werden oben auf die neuen gesetzt"),
    ("--no-rebase", "Merge", "die neuen Commits werden per Merge zusammengeführt"),
    ("--ff-only", "Nur Fast-Forward", "nur wenn du keine eigenen neuen Commits hast"),
];

const FORCE_MODES: &[(&str, &str, &str)] = &[
    ("", "Normal", "nur wenn der Server-Branch keine fremden neuen Commits hat"),
    ("--force-with-lease", "Sicher erzwingen", "überschreibt den Server-Branch, aber nur wenn dort niemand anderes etwas gepusht hat"),
    ("--force", "Erzwingen – gefährlich!", "überschreibt den Server-Branch in jedem Fall; Arbeit anderer kann verloren gehen"),
];

const DELETE_MODES: &[(&str, &str, &str)] = &[
    ("-d", "Nur wenn zusammengeführt", "löscht nur, wenn die Commits schon in einem anderen Branch sind"),
    ("-D", "Erzwingen", "löscht auch, wenn Commits dadurch verloren gehen"),
];

const ARCHIVE_FORMATS: &[(&str, &str, &str)] = &[
    ("--format=zip", "ZIP", "ZIP-Archiv"),
    ("--format=tar", "TAR", "TAR-Archiv"),
    ("--format=tar.gz", "TAR.GZ", "gepacktes TAR-Archiv"),
];

const UNTRACKED: &[(&str, &str, &str)] = &[
    ("", "Standard", "neue Dateien werden angezeigt, Ordner zusammengefasst"),
    ("-uall", "Alle einzeln", "jede neue Datei einzeln anzeigen"),
    ("-uno", "Keine", "neue (nicht verfolgte) Dateien ausblenden"),
];

const GC_PRUNE: &[(&str, &str, &str)] = &[
    ("", "Standard", "unerreichbare Objekte nach 2 Wochen löschen"),
    ("--prune=now", "Sofort löschen", "unerreichbare Objekte sofort löschen"),
    ("--no-prune", "Nichts löschen", "nur packen, nichts entfernen"),
];

const CAT_FILE: &[(&str, &str, &str)] = &[
    ("-p", "Inhalt anzeigen", "zeigt den Inhalt des Objekts lesbar an"),
    ("-t", "Typ", "zeigt den Typ (blob, tree, commit, tag)"),
    ("-s", "Größe", "zeigt die Größe in Bytes"),
    ("-e", "Existiert?", "prüft nur, ob es das Objekt gibt"),
];

const MAINT_TASKS: &[(&str, &str, &str)] = &[
    ("", "Alle fälligen", "führt die eingestellten Aufgaben aus"),
    ("--task=gc", "gc", "Garbage Collection"),
    ("--task=commit-graph", "commit-graph", "Commit-Graph-Datei aktualisieren"),
    ("--task=prefetch", "prefetch", "im Hintergrund vom Server holen"),
    ("--task=loose-objects", "loose-objects", "lose Objekte packen"),
    ("--task=incremental-repack", "incremental-repack", "Pakete schrittweise neu packen"),
    ("--task=pack-refs", "pack-refs", "Referenzen zusammenfassen"),
];

const SORT: &[(&str, &str, &str)] = &[
    ("", "Alphabetisch", "nach Namen"),
    ("--sort=-creatordate", "Neueste zuerst", "nach Erstellungsdatum"),
    ("--sort=-v:refname", "Nach Versionsnummer", "höchste Version zuerst"),
];

fn c(id: &'static str, cat: Cat, title: &'static str, base: &'static str, danger: Danger, summary: &'static str, params: Vec<P>) -> Cmd {
    Cmd { id, cat, title, base, danger, summary, params, kind: Kind::Normal, needs_repo: true }
}

trait CmdExt {
    fn ext(self) -> Self;
    fn norepo(self) -> Self;
    fn kind(self, k: Kind) -> Self;
}
impl CmdExt for Cmd {
    fn ext(mut self) -> Self {
        self.kind = Kind::External;
        self
    }
    fn norepo(mut self) -> Self {
        self.needs_repo = false;
        self
    }
    fn kind(mut self, k: Kind) -> Self {
        self.kind = k;
        self
    }
}

use Danger::{Change as C, Read as R, Risky as X};

pub fn all() -> Vec<Cmd> {
    let mut l: Vec<Cmd> = Vec::new();

    // ------------------------------------------------------------------ Start
    l.push(c("init", Cat::Setup, "Neues Repository anlegen", "init", C,
        "Macht aus einem Ordner ein Git-Repository. Danach kann Git alle Änderungen in diesem Ordner verfolgen.",
        vec![v("--initial-branch=", "initial", "Name des ersten Branches", "main", "wie der erste Branch heißen soll"),
             f("--bare", "Nur Ablage (bare)", "ohne Arbeitsordner, z.B. für einen Server"),
             t("path", "Ordner", "z.B. C:\\Projekte\\neu", "in welchem Ordner das Repository entstehen soll")]).norepo());
    l.push(c("clone", Cat::Setup, "Repository klonen (herunterladen)", "clone", C,
        "Lädt ein vorhandenes Repository von einem Server (z.B. GitHub) vollständig auf deinen Computer herunter.",
        vec![fon("--progress", "Fortschritt zeigen", "zeigt, wie weit der Download ist"),
             f("--recurse-submodules", "Submodule mitladen", "lädt eingebettete Repositories gleich mit"),
             v("--branch=", "branch", "Nur diesen Branch auschecken", "z.B. develop", "welcher Branch nach dem Klonen aktiv ist"),
             v("--depth=", "depth", "Nur die letzten N Commits", "z.B. 1", "spart Platz und Zeit (flacher Klon)"),
             f("--single-branch", "Nur einen Branch", "lädt nur einen Branch statt aller"),
             f("--bare", "Ohne Arbeitsordner (bare)", "nur die Git-Daten"),
             f("--mirror", "Spiegel", "exakte Kopie inklusive aller Referenzen"),
             t("url", "Adresse", "https://github.com/user/projekt.git", "Adresse des Repositories auf dem Server"),
             t("path", "Zielordner", "C:\\Projekte\\projekt", "wohin geklont wird")]).norepo());

    // ------------------------------------------------------------------ Changes
    l.push(c("status", Cat::Changes, "Status anzeigen", "status", R,
        "Zeigt, welche Dateien geändert, vorgemerkt (gestaged) oder neu sind und auf welchem Branch du bist.",
        vec![f("-s", "Kurzform", "eine Zeile pro Datei"), f("-b", "Branch anzeigen", "zeigt Branch und Abstand zum Server"),
             f("--ignored", "Ignorierte zeigen", "zeigt auch Dateien, die per .gitignore ausgeschlossen sind"),
             ch("untracked", "Neue Dateien", UNTRACKED)]));
    l.push(c("add", Cat::Changes, "Dateien vormerken (stage)", "add", C,
        "Merkt Änderungen für den nächsten Commit vor. Nur vorgemerkte Änderungen werden gespeichert.",
        vec![f("-A", "Alles (auch gelöschte)", "merkt alle Änderungen im ganzen Repository vor"),
             f("-u", "Nur bekannte Dateien", "neue Dateien werden nicht vorgemerkt"),
             f("-f", "Auch ignorierte", "merkt auch Dateien vor, die in .gitignore stehen"),
             f("-N", "Nur ankündigen (intent-to-add)", "Datei wird bekannt gemacht, aber Inhalt noch nicht vorgemerkt"),
             f("--dry-run", "Nur testen", "zeigt nur, was passieren würde"),
             m("paths", "Dateien", ". = alles im Ordner", "welche Dateien vorgemerkt werden")]));
    l.push(c("rm", Cat::Changes, "Datei löschen / nicht mehr verfolgen", "rm", X,
        "Löscht Dateien und merkt das Löschen für den nächsten Commit vor. Mit „Nur aus Git entfernen“ bleibt die Datei auf der Festplatte.",
        vec![f("--cached", "Nur aus Git entfernen", "Datei bleibt auf der Festplatte, wird aber nicht mehr verfolgt"),
             f("-r", "Ordner rekursiv", "ganze Ordner entfernen"), f("-f", "Erzwingen", "auch wenn die Datei Änderungen hat"),
             f("--dry-run", "Nur testen", "zeigt nur, was entfernt würde"),
             lit("--", "Trennzeichen: danach folgen Dateinamen"),
             mr("paths", "Dateien", "pfad/zur/datei.txt", "welche Dateien")]));
    l.push(c("mv", Cat::Changes, "Datei verschieben / umbenennen", "mv", C,
        "Verschiebt oder benennt eine Datei um und merkt das für den nächsten Commit vor.",
        vec![f("-f", "Überschreiben", "Ziel überschreiben, falls vorhanden"),
             t("from", "Von", "alt.txt", "bisheriger Name"), t("to", "Nach", "neu.txt", "neuer Name oder Ordner")]));
    l.push(c("restore", Cat::Changes, "Datei wiederherstellen (Änderungen verwerfen)", "restore", X,
        "Setzt Dateien auf den letzten gespeicherten Stand zurück. Ohne Optionen werden ungespeicherte Änderungen in den Dateien verworfen!",
        vec![f("--staged", "Aus Vormerkung nehmen", "nimmt Dateien aus dem Staging, der Inhalt bleibt"),
             f("--worktree", "Dateien zurücksetzen", "verwirft Änderungen in den Dateien selbst"),
             v("--source=", "commit", "Stand von Commit", "z.B. HEAD~2", "von welchem Commit der Inhalt genommen wird"),
             lit("--", "Trennzeichen: danach folgen Dateinamen"),
             mr("paths", "Dateien", "pfad/datei.txt oder .", "welche Dateien")]));
    l.push(c("unstage", Cat::Changes, "Aus Vormerkung nehmen (reset Datei)", "reset", C,
        "Nimmt vorgemerkte Dateien wieder aus dem Staging. Der Inhalt der Dateien bleibt unverändert.",
        vec![o("commit", "Stand", "HEAD", "Vormerkung auf diesen Stand setzen"), lit("--", "Trennzeichen: danach folgen Dateinamen"),
             mr("paths", "Dateien", "pfad/datei.txt", "welche Dateien")]));
    l.push(c("commit", Cat::Changes, "Commit erstellen", "commit", C,
        "Speichert alle vorgemerkten Änderungen dauerhaft als neuen Commit mit einer Beschreibung.",
        vec![vr("-m", "message", "Beschreibung", "Was wurde geändert?", "kurze Zusammenfassung der Änderung"),
             f("-a", "Alle geänderten Dateien", "merkt geänderte (bekannte) Dateien automatisch vor"),
             f("--amend", "Letzten Commit ändern", "ersetzt den letzten Commit statt einen neuen zu machen"),
             f("-s", "Signed-off-by", "fügt eine Unterschriftszeile hinzu"),
             f("-S", "GPG-signieren", "signiert den Commit kryptografisch"),
             f("--allow-empty", "Leeren Commit erlauben", "auch ohne Änderungen"),
             f("--no-verify", "Hooks überspringen", "prüfende Skripte (pre-commit) nicht ausführen"),
             v("--author=", "author", "Anderer Autor", "Name <mail@beispiel.de>", "Commit im Namen von jemand anderem"),
             v("--date=", "date", "Anderes Datum", "2026-01-31 12:00", "Datum des Commits")]));
    l.push(c("commit_amend", Cat::Changes, "Letzten Commit nachbessern (amend)", "commit --amend", X,
        "Ersetzt den letzten Commit: vorgemerkte Änderungen kommen dazu, die Beschreibung kann geändert werden. Ändert die Geschichte – nicht nach Push verwenden.",
        vec![v("-m", "message", "Neue Beschreibung", "leer = alte behalten", "neue Commit-Beschreibung"),
             fon("--no-edit", "Beschreibung behalten", "alte Beschreibung unverändert lassen (aus, wenn du oben eine neue eingibst)"),
             f("--reset-author", "Autor erneuern", "du wirst als Autor eingetragen, Datum = jetzt")]));
    l.push(c("commit_fixup", Cat::Changes, "Fixup-Commit für älteren Commit", "commit", C,
        "Erstellt einen Commit, der später beim Rebase mit „autosquash“ automatisch in einen älteren Commit eingeschmolzen wird.",
        vec![vr("--fixup=", "commit", "Ziel-Commit", "SHA", "zu welchem Commit die Korrektur gehört")]));
    l.push(c("clean", Cat::Changes, "Nicht verfolgte Dateien löschen", "clean", X,
        "Löscht Dateien, die Git nicht kennt (neu und nie vorgemerkt). Erst mit „Nur anzeigen“ testen, dann mit „Wirklich löschen“ ausführen.",
        vec![fon("-n", "Nur anzeigen (Test)", "zeigt nur, was gelöscht würde – nichts wird gelöscht"),
             f("-f", "Wirklich löschen", "nötig, damit tatsächlich gelöscht wird"),
             f("-d", "Auch Ordner", "auch nicht verfolgte Ordner"),
             f("-x", "Auch ignorierte", "auch Dateien aus .gitignore (z.B. Build-Ergebnisse)"),
             f("-X", "Nur ignorierte", "nur Dateien aus .gitignore"), paths()]));
    l.push(c("diff", Cat::Changes, "Unterschiede anzeigen", "diff", R,
        "Zeigt Zeile für Zeile, was sich geändert hat: zwischen Dateien und Vormerkung, Commits oder Branches.",
        vec![f("--staged", "Vorgemerktes", "Unterschiede der vorgemerkten Änderungen zum letzten Commit"),
             f("--stat", "Nur Statistik", "nur geänderte Dateien und Anzahl Zeilen"),
             f("--name-status", "Nur Dateinamen", "nur Namen und Art der Änderung"),
             f("--word-diff", "Wortweise", "Unterschiede innerhalb der Zeile hervorheben"),
             f("-w", "Leerzeichen ignorieren", "Unterschiede nur in Leerzeichen ausblenden"),
             o("a", "Von (Commit/Branch)", "z.B. main", "Vergleichsbasis"), o("b", "Bis (Commit/Branch)", "z.B. feature", "Vergleichsziel"), paths()]));
    l.push(c("assume_unchanged", Cat::Changes, "Änderungen an Datei vorübergehend ignorieren", "update-index", C,
        "Git tut so, als wäre die Datei unverändert (z.B. lokale Einstellungsdatei). Mit der zweiten Option wieder aufheben.",
        vec![ch("mode", "Aktion", &[("--assume-unchanged", "Ignorieren", "Änderungen nicht mehr anzeigen"),
                                    ("--no-assume-unchanged", "Wieder beachten", "Änderungen wieder anzeigen"),
                                    ("--skip-worktree", "Skip-Worktree", "Datei lokal anders halten, auch bei Pull"),
                                    ("--no-skip-worktree", "Skip-Worktree aufheben", "normales Verhalten")]),
             mr("paths", "Dateien", "config.local.json", "welche Dateien")]));
    l.push(c("check_ignore", Cat::Changes, "Warum wird eine Datei ignoriert?", "check-ignore -v", R,
        "Zeigt, welche Regel in welcher .gitignore-Datei dafür sorgt, dass eine Datei ignoriert wird.",
        vec![f("--no-index", "Auch verfolgte Dateien prüfen", ""), mr("paths", "Dateien", "build/out.exe", "welche Dateien")]));
    l.push(c("notes_add", Cat::Changes, "Notiz an Commit hängen", "notes add", C,
        "Hängt eine zusätzliche Notiz an einen Commit, ohne den Commit selbst zu ändern.",
        vec![f("-f", "Bestehende ersetzen", ""), vr("-m", "message", "Notiz", "Text der Notiz", ""), o("commit", "Commit", "HEAD", "an welchen Commit")]));
    l.push(c("notes_show", Cat::Changes, "Notiz anzeigen", "notes show", R, "Zeigt die Notiz eines Commits.",
        vec![o("commit", "Commit", "HEAD", "")]));
    l.push(c("notes_list", Cat::Changes, "Alle Notizen auflisten", "notes list", R, "Listet alle Commits mit Notizen.", vec![]));
    l.push(c("notes_remove", Cat::Changes, "Notiz entfernen", "notes remove", C, "Entfernt die Notiz von einem Commit.",
        vec![o("commit", "Commit", "HEAD", "")]));

    // ------------------------------------------------------------------ Branches
    l.push(c("branch_list", Cat::Branches, "Branches auflisten", "branch", R,
        "Listet die Branches. Der aktuelle ist mit * markiert.",
        vec![f("-a", "Auch Server-Branches", "zeigt auch Branches der Remotes"), f("-r", "Nur Server-Branches", ""),
             f("-vv", "Details", "letzter Commit und verknüpfter Server-Branch"),
             f("--merged", "Nur bereits zusammengeführte", ""), f("--no-merged", "Nur noch nicht zusammengeführte", ""),
             ch("sort", "Sortierung", SORT)]));
    l.push(c("branch_create", Cat::Branches, "Neuen Branch anlegen", "branch", C,
        "Legt einen neuen Branch an, wechselt aber noch nicht dorthin.",
        vec![f("--track", "Server-Branch verfolgen", "neuer Branch merkt sich den Startpunkt als Upstream"),
             t("name", "Name", "feature/login", "Name des neuen Branches"),
             o("commit", "Startpunkt", "leer = aktueller Stand", "von welchem Commit/Branch aus")]));
    l.push(c("switch_create", Cat::Branches, "Neuen Branch anlegen und wechseln", "switch -c", C,
        "Legt einen neuen Branch an und macht ihn sofort zum aktuellen Branch.",
        vec![t("name", "Name", "feature/login", "Name des neuen Branches"),
             o("commit", "Startpunkt", "leer = aktueller Stand", "von welchem Commit/Branch aus")]));
    l.push(c("switch", Cat::Branches, "Zu Branch wechseln (switch)", "switch", C,
        "Wechselt zu einem anderen Branch. Deine Dateien werden auf dessen Stand gebracht.",
        vec![f("--discard-changes", "Lokale Änderungen verwerfen", "Vorsicht: ungespeicherte Änderungen gehen verloren"),
             f("--detach", "Ohne Branch (detached)", "nur den Stand ansehen, ohne auf einem Branch zu sein"),
             f("--guess", "Server-Branch automatisch übernehmen", "legt lokalen Branch an, wenn es ihn nur auf dem Server gibt"),
             t("branch", "Branch", "main", "wohin gewechselt wird")]));
    l.push(c("checkout", Cat::Branches, "Auschecken (Branch oder Commit)", "checkout", C,
        "Wechselt zu einem Branch, Tag oder Commit. Bei einem Commit bist du danach „detached“ (auf keinem Branch).",
        vec![f("-f", "Erzwingen", "verwirft lokale Änderungen"), f("--detach", "Detached", "ohne Branch"),
             t("branch", "Ziel", "main, v1.0 oder SHA", "Branch, Tag oder Commit")]));
    l.push(c("checkout_track", Cat::Branches, "Server-Branch lokal auschecken", "checkout --track", C,
        "Legt einen lokalen Branch an, der einem Branch auf dem Server folgt, und wechselt dorthin.",
        vec![v("-b", "name", "Lokaler Name", "leer = wie auf dem Server", "Name des neuen lokalen Branches"), t("branch", "Server-Branch", "origin/feature", "")]));
    l.push(c("checkout_file", Cat::Branches, "Datei aus anderem Stand holen", "checkout", X,
        "Holt Dateien aus einem anderen Commit oder Branch in deinen Arbeitsordner. Überschreibt deine Änderungen an diesen Dateien!",
        vec![o("commit", "Von Commit/Branch", "z.B. main", "woher die Datei genommen wird"), lit("--", "Trennzeichen: danach folgen Dateinamen"),
             mr("paths", "Dateien", "pfad/datei.txt", "")]));
    l.push(c("branch_rename", Cat::Branches, "Branch umbenennen", "branch -m", C,
        "Gibt einem Branch einen neuen Namen.",
        vec![o("branch", "Alter Name", "leer = aktueller Branch", ""), t("name", "Neuer Name", "feature/neu", "")]));
    l.push(c("branch_copy", Cat::Branches, "Branch kopieren", "branch -c", C, "Erstellt eine Kopie eines Branches unter neuem Namen.",
        vec![o("branch", "Quelle", "leer = aktueller Branch", ""), t("name", "Neuer Name", "feature/kopie", "")]));
    l.push(c("branch_delete", Cat::Branches, "Branch löschen", "branch", X,
        "Löscht einen lokalen Branch. Die Commits bleiben erhalten, wenn sie noch in einem anderen Branch sind.",
        vec![ch("mode", "Art", DELETE_MODES), t("branch", "Branch", "feature/alt", "welcher Branch gelöscht wird")]));
    l.push(c("branch_upstream", Cat::Branches, "Server-Branch zuordnen (Upstream)", "branch", C,
        "Legt fest, mit welchem Branch auf dem Server dieser Branch bei Pull/Push verbunden ist.",
        vec![vr("--set-upstream-to=", "upstream", "Server-Branch", "origin/main", ""), o("branch", "Lokaler Branch", "leer = aktueller", "")]));
    l.push(c("branch_unset_upstream", Cat::Branches, "Verbindung zum Server-Branch lösen", "branch --unset-upstream", C,
        "Entfernt die Verbindung zum Server-Branch.", vec![o("branch", "Branch", "leer = aktueller", "")]));
    l.push(c("reset", Cat::Branches, "Branch auf Commit zurücksetzen (reset)", "reset", X,
        "Setzt den aktuellen Branch auf einen anderen Commit. Je nach Modus bleiben deine Änderungen erhalten oder werden verworfen.",
        vec![ch("mode", "Modus", RESET_MODES), t("commit", "Auf Commit", "HEAD~1 oder SHA", "wohin der Branch zeigen soll")]));
    l.push(c("show_branch", Cat::Branches, "Branches vergleichen (show-branch)", "show-branch", R,
        "Zeigt in einer Tabelle, welche Commits in welchen Branches enthalten sind.",
        vec![f("-a", "Alle Branches", ""), m("branches", "Branches", "main feature", "")]));

    // ------------------------------------------------------------------ Integrate
    l.push(c("merge", Cat::Integrate, "Branch zusammenführen (merge)", "merge", C,
        "Holt die Änderungen eines anderen Branches in deinen aktuellen Branch.",
        vec![ch("mode", "Art", MERGE_MODES), f("--no-commit", "Nicht automatisch committen", "Ergebnis erst prüfen, dann selbst committen"),
             f("--allow-unrelated-histories", "Fremde Geschichte erlauben", "für Repositories ohne gemeinsamen Ursprung"),
             v("-X", "strategy", "Konflikt-Strategie", "ours oder theirs", "bei Konflikten automatisch eine Seite bevorzugen"),
             v("-m", "message", "Beschreibung", "leer = Standardtext", "Text für den Merge-Commit"),
             t("branch", "Branch", "feature/login", "welcher Branch hereingeholt wird")]));
    l.push(c("merge_abort", Cat::Integrate, "Merge abbrechen", "merge --abort", X,
        "Bricht einen laufenden Merge mit Konflikten ab und stellt den Stand davor wieder her.", vec![]));
    l.push(c("merge_continue", Cat::Integrate, "Merge abschließen", "merge --continue", C,
        "Schließt den Merge ab, nachdem alle Konflikte gelöst und vorgemerkt sind.", vec![]));
    l.push(c("rebase", Cat::Integrate, "Branch neu aufsetzen (rebase)", "rebase", X,
        "Setzt deine Commits oben auf einen anderen Branch, als hättest du dort angefangen. Ändert die Geschichte deiner Commits.",
        vec![f("--autostash", "Änderungen automatisch zwischenlagern", "lokale Änderungen vorher stashen und danach zurückholen"),
             f("--autosquash", "Fixup-Commits einschmelzen", ""), f("--rebase-merges", "Merges erhalten", ""),
             f("--update-refs", "Abhängige Branches mitnehmen", ""),
             v("--onto", "onto", "Auf (onto)", "z.B. main", "auf welchen Commit verpflanzt wird (fortgeschritten)"),
             t("branch", "Basis", "main", "auf welchen Branch deine Commits gesetzt werden")]));
    l.push(c("rebase_i", Cat::Integrate, "Interaktiver Rebase (Commits bearbeiten)", "rebase -i", X,
        "Öffnet einen Editor, in dem du Commits umsortieren, zusammenfassen (squash), umbenennen (reword), bearbeiten oder löschen kannst.",
        vec![t("commit", "Ab Commit (ausschließlich)", "z.B. HEAD~5 oder SHA", "alle Commits danach werden bearbeitbar")]).kind(Kind::InteractiveRebase));
    l.push(c("rebase_continue", Cat::Integrate, "Rebase fortsetzen", "rebase --continue", C, "Setzt den Rebase fort, nachdem Konflikte gelöst sind.", vec![]));
    l.push(c("rebase_skip", Cat::Integrate, "Commit beim Rebase überspringen", "rebase --skip", X, "Lässt den aktuellen problematischen Commit weg.", vec![]));
    l.push(c("rebase_abort", Cat::Integrate, "Rebase abbrechen", "rebase --abort", C, "Bricht den Rebase ab und stellt den Stand davor her.", vec![]));
    l.push(c("cherry_pick", Cat::Integrate, "Commit übernehmen (cherry-pick)", "cherry-pick", C,
        "Kopiert einzelne Commits aus einem anderen Branch in deinen aktuellen Branch.",
        vec![f("-x", "Herkunft vermerken", "schreibt „cherry picked from …“ in die Beschreibung"),
             f("-n", "Nicht committen", "nur die Änderungen übernehmen"),
             v("-m", "parent", "Eltern-Nummer (bei Merge-Commit)", "1", "welche Seite eines Merge-Commits gilt"),
             mr("commits", "Commits", "SHA [SHA ...]", "welche Commits kopiert werden")]));
    l.push(c("cherry_pick_continue", Cat::Integrate, "Cherry-Pick fortsetzen", "cherry-pick --continue", C, "Setzt nach gelösten Konflikten fort.", vec![]));
    l.push(c("cherry_pick_skip", Cat::Integrate, "Cherry-Pick: Commit überspringen", "cherry-pick --skip", C, "Überspringt den aktuellen Commit.", vec![]));
    l.push(c("cherry_pick_abort", Cat::Integrate, "Cherry-Pick abbrechen", "cherry-pick --abort", C, "Bricht ab und stellt den Stand davor her.", vec![]));
    l.push(c("revert", Cat::Integrate, "Commit rückgängig machen (revert)", "revert", C,
        "Erstellt einen neuen Commit, der die Änderungen eines alten Commits umkehrt. Die Geschichte bleibt erhalten – sicher auch nach Push.",
        vec![fon("--no-edit", "Standardbeschreibung", "Beschreibung „Revert …“ automatisch verwenden"),
             f("-n", "Nicht committen", "nur die umgekehrten Änderungen vormerken"),
             v("-m", "parent", "Eltern-Nummer (bei Merge-Commit)", "1", ""),
             mr("commits", "Commits", "SHA", "welche Commits rückgängig gemacht werden")]));
    l.push(c("revert_continue", Cat::Integrate, "Revert fortsetzen", "revert --continue", C, "Setzt nach gelösten Konflikten fort.", vec![]));
    l.push(c("revert_abort", Cat::Integrate, "Revert abbrechen", "revert --abort", C, "Bricht ab.", vec![]));
    l.push(c("range_diff", Cat::Integrate, "Zwei Commit-Reihen vergleichen (range-diff)", "range-diff", R,
        "Vergleicht zwei Versionen einer Commit-Reihe, z.B. vor und nach einem Rebase.",
        vec![t("a", "Erste Reihe", "main..feature@{1}", ""), t("b", "Zweite Reihe", "main..feature", "")]));
    l.push(c("rerere_status", Cat::Integrate, "Gemerkte Konfliktlösungen: Status", "rerere status", R,
        "rerere merkt sich, wie du Konflikte gelöst hast, und löst gleiche Konflikte künftig automatisch.", vec![]));
    l.push(c("rerere_diff", Cat::Integrate, "Gemerkte Konfliktlösungen: Unterschiede", "rerere diff", R, "Zeigt die gemerkte Lösung des aktuellen Konflikts.", vec![]));
    l.push(c("rerere_forget", Cat::Integrate, "Gemerkte Lösung vergessen", "rerere forget", C, "Vergisst die gemerkte Lösung für eine Datei.", vec![mr("paths", "Dateien", "datei.txt", "")]));
    l.push(c("rerere_clear", Cat::Integrate, "Alle gemerkten Lösungen löschen", "rerere clear", C, "Löscht die Aufzeichnungen des laufenden Konflikts.", vec![]));
    l.push(c("merge_file", Cat::Integrate, "Drei Dateien zusammenführen (merge-file)", "merge-file", C,
        "Führt zwei geänderte Versionen einer Datei mit ihrer gemeinsamen Basis zusammen. Das Ergebnis landet in der ersten Datei.",
        vec![f("-p", "Nur anzeigen", "Ergebnis ausgeben statt Datei zu überschreiben"),
             t("current", "Meine Datei", "meine.txt", ""), t("base", "Gemeinsame Basis", "basis.txt", ""), t("other", "Andere Datei", "andere.txt", "")]));

    // ------------------------------------------------------------------ Remote
    l.push(c("fetch", Cat::Remote, "Neues vom Server holen (fetch)", "fetch", R,
        "Lädt neue Commits und Branches vom Server herunter, ändert aber deine Dateien und Branches nicht.",
        vec![f("--all", "Alle Remotes", "von allen Servern holen"), fon("--prune", "Gelöschte entfernen", "entfernt lokale Kopien von Branches, die auf dem Server gelöscht wurden"),
             f("--tags", "Alle Tags", "auch alle Tags holen"), f("--dry-run", "Nur testen", ""),
             v("--depth=", "depth", "Tiefe", "", "nur N Commits holen"), f("--unshallow", "Vollständige Geschichte nachladen", ""),
             o("remote", "Remote", "origin", "von welchem Server"), o("branch", "Branch", "leer = alle", "nur diesen Branch")]));
    l.push(c("pull", Cat::Remote, "Neues holen und einarbeiten (pull)", "pull", C,
        "Holt neue Commits vom Server und führt sie mit deinem aktuellen Branch zusammen (fetch + merge/rebase).",
        vec![ch("mode", "Art", PULL_MODES), f("--autostash", "Lokale Änderungen zwischenlagern", ""),
             f("--prune", "Gelöschte entfernen", ""), f("--recurse-submodules", "Submodule mitnehmen", ""),
             o("remote", "Remote", "origin", ""), o("branch", "Branch", "leer = verbundener Branch", "")]));
    l.push(c("push", Cat::Remote, "Commits hochladen (push)", "push", C,
        "Lädt deine neuen Commits auf den Server hoch, damit andere sie sehen.",
        vec![f("-u", "Upstream setzen", "verbindet den Branch dauerhaft mit dem Server-Branch"),
             ch("force", "Überschreiben", FORCE_MODES), f("--tags", "Alle Tags mit hochladen", ""),
             f("--follow-tags", "Passende Tags mit hochladen", "nur annotierte Tags der gepushten Commits"),
             f("--all", "Alle Branches", ""), f("--no-verify", "Hooks überspringen", ""), f("--dry-run", "Nur testen", ""),
             o("remote", "Remote", "origin", "zu welchem Server"), o("branch", "Branch", "leer = aktueller", "welcher Branch (oder lokal:server)")]));
    l.push(c("push_delete", Cat::Remote, "Branch auf dem Server löschen", "push", X,
        "Löscht einen Branch auf dem Server. Dein lokaler Branch bleibt erhalten.",
        vec![t("remote", "Remote", "origin", ""), lit("--delete", "Option: auf dem Server löschen"), t("branch", "Branch", "feature/alt", "")]));
    l.push(c("remote_list", Cat::Remote, "Remotes anzeigen", "remote -v", R, "Zeigt alle verbundenen Server mit ihren Adressen.", vec![]));
    l.push(c("remote_add", Cat::Remote, "Remote hinzufügen", "remote add", C,
        "Verbindet das Repository mit einem (weiteren) Server.",
        vec![f("-f", "Gleich holen", "danach sofort fetch"), t("remote", "Name", "origin", "Kurzname des Servers"), t("url", "Adresse", "https://…/projekt.git", "")]));
    l.push(c("remote_remove", Cat::Remote, "Remote entfernen", "remote remove", C,
        "Trennt die Verbindung zu einem Server. Auf dem Server wird nichts gelöscht.", vec![t("remote", "Name", "upstream", "")]));
    l.push(c("remote_rename", Cat::Remote, "Remote umbenennen", "remote rename", C, "Gibt einem Remote einen neuen Kurznamen.",
        vec![t("remote", "Alter Name", "origin", ""), t("name", "Neuer Name", "github", "")]));
    l.push(c("remote_set_url", Cat::Remote, "Remote-Adresse ändern", "remote set-url", C, "Ändert die Server-Adresse eines Remotes.",
        vec![f("--push", "Nur für Push", ""), t("remote", "Name", "origin", ""), t("url", "Neue Adresse", "git@github.com:user/projekt.git", "")]));
    l.push(c("remote_show", Cat::Remote, "Remote-Details", "remote show", R, "Zeigt Details zu einem Server: Branches und Verbindungen.", vec![t("remote", "Name", "origin", "")]));
    l.push(c("remote_prune", Cat::Remote, "Veraltete Server-Branches aufräumen", "remote prune", C,
        "Entfernt lokale Kopien von Branches, die es auf dem Server nicht mehr gibt.", vec![f("--dry-run", "Nur testen", ""), t("remote", "Name", "origin", "")]));
    l.push(c("remote_update", Cat::Remote, "Alle Remotes aktualisieren", "remote update", R, "Holt von allen Servern.", vec![f("--prune", "Gelöschte entfernen", "")]));
    l.push(c("ls_remote", Cat::Remote, "Branches/Tags auf dem Server ansehen", "ls-remote", R,
        "Fragt den Server, welche Branches und Tags es dort gibt, ohne etwas herunterzuladen.",
        vec![f("--heads", "Nur Branches", ""), f("--tags", "Nur Tags", ""), o("remote", "Remote oder Adresse", "origin", "")]));
    l.push(c("request_pull", Cat::Remote, "Pull-Request-Text erzeugen", "request-pull", R,
        "Erzeugt eine Zusammenfassung deiner Änderungen, die du jemandem schicken kannst, damit er sie übernimmt.",
        vec![t("start", "Basis-Commit", "origin/main", ""), t("url", "Deine Adresse", "https://…/projekt.git", ""), o("end", "Ende", "HEAD", "")]));

    // ------------------------------------------------------------------ Stash
    l.push(c("stash_push", Cat::Stash, "Änderungen zwischenlagern (stash)", "stash push", C,
        "Legt deine noch nicht gespeicherten Änderungen beiseite und räumt den Arbeitsordner auf. Später kannst du sie zurückholen.",
        vec![v("-m", "message", "Beschreibung", "z.B. halbfertiges Login", "damit du den Stash wiedererkennst"),
             f("-u", "Neue Dateien mitnehmen", "auch nicht verfolgte Dateien"), f("-a", "Auch ignorierte", ""),
             f("--keep-index", "Vorgemerktes behalten", "vorgemerkte Änderungen bleiben im Ordner"),
             f("--staged", "Nur Vorgemerktes", "nur vorgemerkte Änderungen stashen"), paths()]));
    l.push(c("stash_list", Cat::Stash, "Stashes auflisten", "stash list", R, "Zeigt alle zwischengelagerten Änderungen.", vec![]));
    l.push(c("stash_show", Cat::Stash, "Stash-Inhalt ansehen", "stash show -p", R, "Zeigt die Änderungen in einem Stash.",
        vec![o("stash", "Stash", "stash@{0}", "leer = neuester")]));
    l.push(c("stash_apply", Cat::Stash, "Stash anwenden (behalten)", "stash apply", C,
        "Holt die Änderungen aus einem Stash zurück; der Stash selbst bleibt erhalten.",
        vec![f("--index", "Vormerkung wiederherstellen", ""), o("stash", "Stash", "stash@{0}", "leer = neuester")]));
    l.push(c("stash_pop", Cat::Stash, "Stash zurückholen und entfernen (pop)", "stash pop", C,
        "Holt die Änderungen aus einem Stash zurück und löscht den Stash danach.",
        vec![f("--index", "Vormerkung wiederherstellen", ""), o("stash", "Stash", "stash@{0}", "leer = neuester")]));
    l.push(c("stash_drop", Cat::Stash, "Stash löschen", "stash drop", X, "Löscht einen Stash endgültig.", vec![o("stash", "Stash", "stash@{0}", "leer = neuester")]));
    l.push(c("stash_clear", Cat::Stash, "Alle Stashes löschen", "stash clear", X, "Löscht alle Stashes endgültig.", vec![]));
    l.push(c("stash_branch", Cat::Stash, "Branch aus Stash erstellen", "stash branch", C,
        "Erstellt einen neuen Branch am Ausgangspunkt des Stashes und wendet den Stash dort an.",
        vec![t("name", "Neuer Branch", "feature/aus-stash", ""), o("stash", "Stash", "stash@{0}", "")]));
    l.push(c("stash_create", Cat::Stash, "Stash-Commit erzeugen (ohne Ablage)", "stash create", R,
        "Erzeugt einen Stash-Commit und gibt seine SHA aus, ohne ihn in die Stash-Liste zu legen (für Skripte).", vec![]));

    // ------------------------------------------------------------------ Tags
    l.push(c("tag_list", Cat::Tags, "Tags auflisten", "tag -n", R, "Zeigt alle Tags (Versionsmarken) mit ihrer Beschreibung.",
        vec![ch("sort", "Sortierung", SORT), v("-l", "pattern", "Muster", "v1.*", "nur passende Tags")]));
    l.push(c("tag_create", Cat::Tags, "Tag erstellen", "tag", C,
        "Markiert einen Commit dauerhaft mit einem Namen, z.B. eine Version „v1.0“. Mit Beschreibung entsteht ein annotierter Tag.",
        vec![v("-m", "message", "Beschreibung", "leer = einfacher Tag", "macht daraus einen annotierten Tag"),
             f("-s", "Signieren", "GPG-Signatur"), f("-f", "Ersetzen", "vorhandenen Tag gleichen Namens überschreiben"),
             t("tag", "Name", "v1.0.0", ""), o("commit", "Commit", "leer = aktueller", "welcher Commit markiert wird")]));
    l.push(c("tag_delete", Cat::Tags, "Tag löschen (lokal)", "tag -d", C, "Löscht einen Tag auf deinem Computer.", vec![t("tag", "Name", "v1.0.0", "")]));
    l.push(c("push_tag", Cat::Tags, "Tag hochladen", "push", C, "Lädt einen Tag auf den Server hoch.",
        vec![t("remote", "Remote", "origin", ""), t("tag", "Tag", "v1.0.0", "")]));
    l.push(c("push_tags", Cat::Tags, "Alle Tags hochladen", "push", C, "Lädt alle Tags auf den Server hoch.",
        vec![t("remote", "Remote", "origin", ""), lit("--tags", "Option: alle Tags")]));
    l.push(c("push_tag_delete", Cat::Tags, "Tag auf dem Server löschen", "push", X, "Löscht einen Tag auf dem Server.",
        vec![t("remote", "Remote", "origin", ""), lit("--delete", "Option: auf dem Server löschen"), t("tag", "Tag", "v1.0.0", "")]));
    l.push(c("describe", Cat::Tags, "Commit über Tag beschreiben", "describe", R,
        "Beschreibt einen Commit relativ zum nächsten Tag, z.B. „v1.2-5-gabc123“ (5 Commits nach v1.2).",
        vec![fon("--tags", "Alle Tags", "auch einfache Tags verwenden"), f("--always", "Immer etwas ausgeben", ""), f("--long", "Lange Form", ""),
             o("commit", "Commit", "HEAD", "")]));
    l.push(c("verify_tag", Cat::Tags, "Tag-Signatur prüfen", "verify-tag", R, "Prüft die GPG-Signatur eines Tags.", vec![t("tag", "Tag", "v1.0.0", "")]));

    // ------------------------------------------------------------------ History
    l.push(c("log", Cat::History, "Verlauf anzeigen (log)", "log", R,
        "Zeigt die Liste der Commits mit Autor, Datum und Beschreibung.",
        vec![fon("--oneline", "Eine Zeile pro Commit", ""), f("--graph", "Als Graph", "Verzweigungen zeichnen"), f("--all", "Alle Branches", ""),
             f("--stat", "Geänderte Dateien", ""), f("-p", "Mit Änderungen (Patch)", ""), f("--first-parent", "Nur Hauptlinie", ""),
             f("--no-merges", "Ohne Merge-Commits", ""), f("--follow", "Umbenennungen folgen (eine Datei)", ""),
             v("-n", "count", "Anzahl", "z.B. 20", "höchstens so viele Commits"), v("--author=", "author", "Autor", "Name", ""),
             v("--grep=", "grep", "Beschreibung enthält", "Suchtext", ""), v("--since=", "since", "Seit", "2 weeks ago", ""),
             v("--until=", "until", "Bis", "2026-01-01", ""), v("-S", "pickaxe", "Code enthält (Pickaxe)", "funktionsName", "Commits, die diesen Text hinzufügen/entfernen"),
             o("range", "Bereich", "main..feature", "welche Commits"), paths()]));
    l.push(c("show", Cat::History, "Commit ansehen (show)", "show", R, "Zeigt einen Commit mit Beschreibung und Änderungen.",
        vec![f("--stat", "Nur Statistik", ""), f("--name-status", "Nur Dateinamen", ""), o("commit", "Commit/Tag/Datei", "HEAD oder HEAD:pfad/datei", "")]));
    l.push(c("shortlog", Cat::History, "Commits pro Autor zählen", "shortlog -sne", R, "Zählt, wie viele Commits jeder Autor gemacht hat.",
        vec![f("--all", "Alle Branches", ""), lit("HEAD", "ab dem aktuellen Stand")]));
    l.push(c("blame", Cat::History, "Wer hat welche Zeile geschrieben? (blame)", "blame", R,
        "Zeigt für jede Zeile einer Datei, in welchem Commit und von wem sie zuletzt geändert wurde.",
        vec![v("-L", "lines", "Zeilen", "10,20", "nur diese Zeilen"), f("-w", "Leerzeichen ignorieren", ""),
             f("-M", "Verschobene Zeilen erkennen", ""), f("-C", "Kopierte Zeilen erkennen", ""),
             o("commit", "Stand", "leer = aktuell", ""), lit("--", "Trennzeichen: danach der Dateiname"), t("file", "Datei", "src/main.rs", "")]));
    l.push(c("annotate", Cat::History, "Datei annotieren (annotate)", "annotate", R, "Ältere Form von blame: Autor und Commit pro Zeile.",
        vec![t("file", "Datei", "src/main.rs", "")]));
    l.push(c("grep", Cat::History, "Im Code suchen (grep)", "grep -n", R, "Durchsucht alle verfolgten Dateien nach einem Text.",
        vec![f("-i", "Groß/klein egal", ""), f("-w", "Ganzes Wort", ""), f("-E", "Regulärer Ausdruck", ""), f("--count", "Nur Anzahl pro Datei", ""),
             t("pattern", "Suchtext", "TODO", ""), o("commit", "In Commit", "leer = aktueller Stand", ""), paths()]));
    l.push(c("reflog", Cat::History, "Reflog – alles, was HEAD je war", "reflog", R,
        "Zeigt jede Bewegung von Branches und HEAD. Damit findest du „verlorene“ Commits nach Reset oder Rebase wieder.",
        vec![v("-n", "count", "Anzahl", "50", ""), o("ref", "Referenz", "HEAD", "")]));
    l.push(c("whatchanged", Cat::History, "Was hat sich geändert (whatchanged)", "whatchanged", R, "Alte Form von log mit Dateiliste.",
        vec![v("-n", "count", "Anzahl", "10", "")]));
    l.push(c("cherry", Cat::History, "Noch nicht übernommene Commits (cherry)", "cherry -v", R,
        "Zeigt Commits, die noch nicht im Ziel-Branch sind (+) bzw. schon gleichwertig enthalten sind (-).",
        vec![o("upstream", "Ziel", "origin/main", ""), o("head", "Quelle", "HEAD", "")]));
    l.push(c("bisect_start", Cat::History, "Fehlersuche starten (bisect)", "bisect start", C,
        "Findet durch Halbieren den Commit, der einen Fehler eingebaut hat. Du sagst jeweils „gut“ oder „schlecht“.",
        vec![o("bad", "Schlechter Commit", "HEAD", "hier ist der Fehler da"), o("good", "Guter Commit", "v1.0", "hier war noch alles gut")]));
    l.push(c("bisect_good", Cat::History, "Bisect: dieser Stand ist gut", "bisect good", C, "Markiert den aktuellen (oder angegebenen) Commit als fehlerfrei.", vec![o("commit", "Commit", "leer = aktueller", "")]));
    l.push(c("bisect_bad", Cat::History, "Bisect: dieser Stand ist schlecht", "bisect bad", C, "Markiert den aktuellen (oder angegebenen) Commit als fehlerhaft.", vec![o("commit", "Commit", "leer = aktueller", "")]));
    l.push(c("bisect_skip", Cat::History, "Bisect: Stand überspringen", "bisect skip", C, "Dieser Commit lässt sich nicht testen.", vec![o("commit", "Commit", "leer = aktueller", "")]));
    l.push(c("bisect_run", Cat::History, "Bisect automatisch mit Skript", "bisect run", C, "Lässt ein Test-Skript entscheiden, ob ein Stand gut ist (Exit-Code 0 = gut).", vec![mr("script", "Befehl", "npm test", "")]));
    l.push(c("bisect_log", Cat::History, "Bisect-Protokoll", "bisect log", R, "Zeigt die bisherigen Bisect-Schritte.", vec![]));
    l.push(c("bisect_reset", Cat::History, "Bisect beenden", "bisect reset", C, "Beendet die Fehlersuche und kehrt zum Ausgangs-Branch zurück.", vec![]));

    // ------------------------------------------------------------------ Modules
    l.push(c("submodule_status", Cat::Modules, "Submodule anzeigen", "submodule status", R, "Zeigt eingebettete Repositories und ihren Stand.", vec![f("--recursive", "Verschachtelte auch", "")]));
    l.push(c("submodule_add", Cat::Modules, "Submodul hinzufügen", "submodule add", C, "Bettet ein anderes Repository als Unterordner ein.",
        vec![v("-b", "branch", "Branch", "main", ""), f("--force", "Erzwingen", ""), t("url", "Adresse", "https://…/lib.git", ""), o("path", "Ordner", "libs/lib", "")]));
    l.push(c("submodule_update", Cat::Modules, "Submodule aktualisieren", "submodule update", C, "Bringt alle Submodule auf den Stand, den das Repository erwartet.",
        vec![fon("--init", "Auch neue einrichten", ""), fon("--recursive", "Verschachtelte auch", ""), f("--remote", "Neuesten Server-Stand holen", ""), f("--force", "Erzwingen", ""), paths()]));
    l.push(c("submodule_init", Cat::Modules, "Submodule einrichten", "submodule init", C, "Trägt Submodule in die lokale Konfiguration ein.", vec![paths()]));
    l.push(c("submodule_deinit", Cat::Modules, "Submodul abmelden", "submodule deinit", X, "Entfernt den Inhalt eines Submoduls aus dem Arbeitsordner.",
        vec![f("-f", "Erzwingen", ""), f("--all", "Alle", ""), m("paths", "Submodule", "libs/lib", "")]));
    l.push(c("submodule_sync", Cat::Modules, "Submodul-Adressen abgleichen", "submodule sync", C, "Übernimmt geänderte Adressen aus .gitmodules.", vec![fon("--recursive", "Verschachtelte auch", "")]));
    l.push(c("submodule_foreach", Cat::Modules, "Befehl in jedem Submodul ausführen", "submodule foreach", C, "Führt einen Befehl in jedem Submodul aus.",
        vec![f("--recursive", "Verschachtelte auch", ""), t("command", "Befehl", "git pull", "")]));
    l.push(c("submodule_set_url", Cat::Modules, "Submodul-Adresse ändern", "submodule set-url", C, "Ändert die Adresse eines Submoduls.",
        vec![t("path", "Submodul", "libs/lib", ""), t("url", "Neue Adresse", "", "")]));
    l.push(c("submodule_set_branch", Cat::Modules, "Submodul-Branch festlegen", "submodule set-branch", C, "Legt fest, welchem Branch das Submodul folgt.",
        vec![vr("-b", "branch", "Branch", "main", ""), t("path", "Submodul", "libs/lib", "")]));
    l.push(c("submodule_summary", Cat::Modules, "Submodul-Änderungen zusammenfassen", "submodule summary", R, "Zeigt neue Commits in Submodulen.", vec![]));
    l.push(c("submodule_absorb", Cat::Modules, "Submodul-Daten einsammeln", "submodule absorbgitdirs", C, "Verschiebt Git-Daten der Submodule ins Haupt-Repository.", vec![]));
    l.push(c("worktree_list", Cat::Modules, "Arbeitsordner (Worktrees) anzeigen", "worktree list", R, "Ein Repository kann mehrere Arbeitsordner gleichzeitig haben, jeder auf einem anderen Branch.", vec![]));
    l.push(c("worktree_add", Cat::Modules, "Weiteren Arbeitsordner anlegen", "worktree add", C, "Legt einen zusätzlichen Ordner an, in dem ein anderer Branch ausgecheckt ist.",
        vec![v("-b", "name", "Neuer Branch", "leer = vorhandenen nutzen", ""), t("path", "Ordner", "..\\projekt-feature", ""), o("branch", "Branch/Commit", "feature", "")]));
    l.push(c("worktree_remove", Cat::Modules, "Arbeitsordner entfernen", "worktree remove", X, "Entfernt einen zusätzlichen Arbeitsordner.",
        vec![f("--force", "Erzwingen (auch mit Änderungen)", ""), t("path", "Ordner", "", "")]));
    l.push(c("worktree_move", Cat::Modules, "Arbeitsordner verschieben", "worktree move", C, "Verschiebt einen zusätzlichen Arbeitsordner an einen neuen Ort.", vec![t("path", "Ordner", "", ""), t("to", "Neuer Ort", "", "")]));
    l.push(c("worktree_lock", Cat::Modules, "Arbeitsordner sperren", "worktree lock", C, "Schützt einen Arbeitsordner vor automatischem Aufräumen.", vec![v("--reason", "reason", "Grund", "", ""), t("path", "Ordner", "", "")]));
    l.push(c("worktree_unlock", Cat::Modules, "Arbeitsordner entsperren", "worktree unlock", C, "Hebt die Sperre wieder auf.", vec![t("path", "Ordner", "", "")]));
    l.push(c("worktree_prune", Cat::Modules, "Verwaiste Arbeitsordner aufräumen", "worktree prune", C, "Entfernt Einträge für Arbeitsordner, die es nicht mehr gibt.", vec![f("-n", "Nur testen", "")]));
    l.push(c("worktree_repair", Cat::Modules, "Arbeitsordner reparieren", "worktree repair", C, "Repariert Verknüpfungen nach manuellem Verschieben.", vec![]));
    l.push(c("sparse_set", Cat::Modules, "Nur Teile auschecken (sparse-checkout set)", "sparse-checkout set", C,
        "Bei sehr großen Repositories nur bestimmte Ordner im Arbeitsordner haben.", vec![fon("--cone", "Ordner-Modus", ""), mr("paths", "Ordner", "src docs", "")]));
    l.push(c("sparse_add", Cat::Modules, "Sparse-Checkout: Ordner hinzufügen", "sparse-checkout add", C, "Nimmt weitere Ordner in den Arbeitsordner auf.", vec![mr("paths", "Ordner", "tests", "")]));
    l.push(c("sparse_list", Cat::Modules, "Sparse-Checkout: Liste", "sparse-checkout list", R, "Zeigt die ausgewählten Ordner.", vec![]));
    l.push(c("sparse_disable", Cat::Modules, "Sparse-Checkout abschalten", "sparse-checkout disable", C, "Holt wieder alle Dateien in den Arbeitsordner.", vec![]));
    l.push(c("sparse_reapply", Cat::Modules, "Sparse-Checkout neu anwenden", "sparse-checkout reapply", C, "Wendet die Auswahl erneut an.", vec![]));

    // ------------------------------------------------------------------ Patches
    l.push(c("format_patch", Cat::Patches, "Commits als Patch-Dateien speichern", "format-patch", R,
        "Speichert Commits als .patch-Dateien, z.B. zum Verschicken per E-Mail.",
        vec![v("-o", "dir", "Zielordner", "patches", ""), f("--cover-letter", "Deckblatt", ""), f("--stdout", "Nur anzeigen", "Patches ausgeben statt Dateien schreiben"),
             v("-v", "reroll", "Version", "2", ""), t("range", "Commits", "-3 oder main..feature", "welche Commits")]));
    l.push(c("am", Cat::Patches, "Patch-Dateien als Commits anwenden (am)", "am", C, "Wendet per E-Mail/Datei erhaltene Patches als Commits an.",
        vec![f("-3", "3-Wege-Merge", "bei Konflikten besser zusammenführen"), f("--signoff", "Signed-off-by", ""), mr("files", "Dateien", "0001-fix.patch", "")]));
    l.push(c("am_continue", Cat::Patches, "am fortsetzen", "am --continue", C, "Setzt nach gelösten Konflikten fort.", vec![]));
    l.push(c("am_skip", Cat::Patches, "am: Patch überspringen", "am --skip", C, "Überspringt den aktuellen Patch.", vec![]));
    l.push(c("am_abort", Cat::Patches, "am abbrechen", "am --abort", C, "Bricht ab und stellt den Stand davor her.", vec![]));
    l.push(c("apply", Cat::Patches, "Patch auf Dateien anwenden (apply)", "apply", C, "Wendet eine .patch/.diff-Datei auf deine Dateien an, ohne Commit.",
        vec![f("--check", "Nur prüfen", "prüft, ob der Patch passt"), f("--stat", "Nur Statistik", ""), f("--index", "Auch vormerken", ""),
             f("--cached", "Nur in Vormerkung", ""), f("-R", "Umgekehrt", "Patch rückwärts anwenden"), f("--3way", "3-Wege", ""), t("file", "Patch-Datei", "fix.patch", "")]));
    l.push(c("archive", Cat::Patches, "Stand als ZIP/TAR exportieren", "archive", R, "Packt einen Stand des Projekts (ohne Git-Daten) in ein Archiv.",
        vec![ch("format", "Format", ARCHIVE_FORMATS), vr("-o", "file", "Ausgabedatei", "projekt.zip", ""), v("--prefix=", "prefix", "Ordner im Archiv", "projekt/", ""),
             t("commit", "Stand", "HEAD", "")]));
    l.push(c("bundle_create", Cat::Patches, "Bundle-Datei erstellen", "bundle create", R, "Packt das Repository in eine einzelne Datei, z.B. für den Transport per USB-Stick.",
        vec![t("file", "Datei", "projekt.bundle", ""), mr("refs", "Was", "--all", "z.B. --all oder main")]));
    l.push(c("bundle_verify", Cat::Patches, "Bundle-Datei prüfen", "bundle verify", R, "Prüft, ob die Bundle-Datei vollständig ist.", vec![t("file", "Datei", "projekt.bundle", "")]));
    l.push(c("bundle_list", Cat::Patches, "Bundle-Inhalt anzeigen", "bundle list-heads", R, "Zeigt die Branches in einer Bundle-Datei.", vec![t("file", "Datei", "projekt.bundle", "")]));
    l.push(c("bundle_unbundle", Cat::Patches, "Bundle entpacken", "bundle unbundle", C, "Liest die Objekte einer Bundle-Datei ein.", vec![t("file", "Datei", "projekt.bundle", "")]));
    l.push(c("send_email", Cat::Patches, "Patches per E-Mail senden", "send-email", C, "Verschickt Patch-Dateien per E-Mail (SMTP muss eingerichtet sein).",
        vec![v("--to=", "to", "An", "liste@beispiel.de", ""), f("--dry-run", "Nur testen", ""), mr("files", "Dateien", "*.patch", "")]));
    l.push(c("imap_send", Cat::Patches, "Patches in IMAP-Ordner legen", "imap-send", C, "Legt Patches als Entwürfe in einen IMAP-Ordner (Einrichtung nötig).",
        vec![stdin("Patch-Text (mbox)", "Ausgabe von format-patch --stdout")]));
    l.push(c("quiltimport", Cat::Patches, "Quilt-Patches importieren", "quiltimport", C, "Übernimmt eine Quilt-Patchreihe als Commits.", vec![f("--dry-run", "Nur testen", ""), v("--patches=", "dir", "Patch-Ordner", "patches", "")]));

    // ------------------------------------------------------------------ Maintain
    l.push(c("gc", Cat::Maintain, "Aufräumen und komprimieren (gc)", "gc", C, "Räumt das Repository auf und packt Daten platzsparend zusammen.",
        vec![f("--aggressive", "Gründlich", "dauert länger, spart mehr Platz"), f("--auto", "Nur wenn nötig", ""), ch("prune", "Unerreichbare Objekte", GC_PRUNE)]));
    l.push(c("maintenance_run", Cat::Maintain, "Wartung ausführen", "maintenance run", C, "Führt Wartungsaufgaben aus.", vec![ch("task", "Aufgabe", MAINT_TASKS)]));
    l.push(c("maintenance_start", Cat::Maintain, "Automatische Wartung einschalten", "maintenance start", C, "Plant regelmäßige Wartung im Hintergrund.", vec![]));
    l.push(c("maintenance_stop", Cat::Maintain, "Automatische Wartung ausschalten", "maintenance stop", C, "Beendet die geplante Wartung.", vec![]));
    l.push(c("maintenance_register", Cat::Maintain, "Repository für Wartung anmelden", "maintenance register", C, "Nimmt dieses Repository in die automatische Wartung auf.", vec![]));
    l.push(c("maintenance_unregister", Cat::Maintain, "Repository von Wartung abmelden", "maintenance unregister", C, "Nimmt dieses Repository aus der automatischen Wartung.", vec![]));
    l.push(c("fsck", Cat::Maintain, "Repository auf Fehler prüfen (fsck)", "fsck", R, "Prüft, ob alle Daten im Repository vollständig und unbeschädigt sind.",
        vec![f("--full", "Vollständig", ""), f("--unreachable", "Unerreichbare zeigen", ""), f("--lost-found", "Verlorenes retten", "schreibt verlorene Objekte nach .git/lost-found"),
             f("--connectivity-only", "Nur Verbindungen (schnell)", "")]));
    l.push(c("prune", Cat::Maintain, "Unerreichbare Objekte löschen", "prune", X, "Löscht Objekte, die von nichts mehr erreicht werden.",
        vec![f("-n", "Nur testen", ""), f("-v", "Ausführlich", ""), v("--expire=", "expire", "Älter als", "2.weeks.ago", "")]));
    l.push(c("repack", Cat::Maintain, "Neu packen (repack)", "repack", C, "Packt die Objektdatenbank neu.",
        vec![fon("-a", "Alles in ein Paket", ""), fon("-d", "Alte Pakete entfernen", ""), f("-f", "Deltas neu berechnen", "")]));
    l.push(c("count_objects", Cat::Maintain, "Größe des Repositories", "count-objects -vH", R, "Zeigt, wie viele Objekte es gibt und wie viel Platz sie brauchen.", vec![]));
    l.push(c("pack_refs", Cat::Maintain, "Referenzen zusammenfassen", "pack-refs", C, "Fasst viele Branch-/Tag-Dateien in einer Datei zusammen.", vec![fon("--all", "Alle", "")]));
    l.push(c("commit_graph", Cat::Maintain, "Commit-Graph schreiben", "commit-graph write", C, "Beschleunigt Verlaufsanzeigen bei großen Repositories.", vec![fon("--reachable", "Alle erreichbaren", "")]));
    l.push(c("commit_graph_verify", Cat::Maintain, "Commit-Graph prüfen", "commit-graph verify", R, "Prüft die Commit-Graph-Datei.", vec![]));
    l.push(c("midx_write", Cat::Maintain, "Multi-Pack-Index schreiben", "multi-pack-index write", C, "Ein Index über mehrere Pakete beschleunigt Zugriffe.", vec![]));
    l.push(c("midx_verify", Cat::Maintain, "Multi-Pack-Index prüfen", "multi-pack-index verify", R, "Prüft den Multi-Pack-Index.", vec![]));
    l.push(c("reflog_expire", Cat::Maintain, "Alte Reflog-Einträge löschen", "reflog expire", X, "Löscht alte Einträge aus dem Reflog (danach sind alte Stände schwerer wiederzufinden).",
        vec![v("--expire=", "expire", "Älter als", "90.days.ago", ""), fon("--all", "Alle Referenzen", ""), f("--dry-run", "Nur testen", "")]));
    l.push(c("filter_branch", Cat::Maintain, "Geschichte umschreiben (filter-branch)", "filter-branch", X,
        "Schreibt die gesamte Geschichte um, z.B. um eine Datei aus allen Commits zu entfernen. Sehr gefährlich, alle Commits ändern sich!",
        vec![f("-f", "Erzwingen", ""), v("--index-filter", "index", "Index-Filter", "git rm --cached --ignore-unmatch geheim.txt", "Befehl für jeden Commit"),
             v("--msg-filter", "msg", "Nachrichten-Filter", "sed s/alt/neu/", ""), fon("--prune-empty", "Leere Commits entfernen", ""),
             lit("--", "Trennzeichen"), m("range", "Branches", "--all", "")]));
    l.push(c("replace_list", Cat::Maintain, "Ersetzungen anzeigen", "replace -l", R, "Git kann Objekte durch andere ersetzen (z.B. Geschichte anfügen).", vec![]));
    l.push(c("replace", Cat::Maintain, "Objekt ersetzen", "replace", C, "Lässt Git ein Objekt überall durch ein anderes ersetzen.", vec![f("-f", "Erzwingen", ""), t("object", "Original", "SHA", ""), t("replacement", "Ersatz", "SHA", "")]));
    l.push(c("replace_delete", Cat::Maintain, "Ersetzung entfernen", "replace -d", C, "Hebt eine Ersetzung auf.", vec![t("object", "Original", "SHA", "")]));
    l.push(c("verify_pack", Cat::Maintain, "Paketdatei prüfen", "verify-pack -v", R, "Prüft eine Paketdatei und zeigt ihren Inhalt.", vec![t("file", "Pack-Index", ".git/objects/pack/pack-….idx", "")]));
    l.push(c("prune_packed", Cat::Maintain, "Bereits gepackte lose Objekte löschen", "prune-packed", C, "Entfernt doppelte lose Objekte.", vec![f("-n", "Nur testen", "")]));
    l.push(c("rerere_gc", Cat::Maintain, "Alte Konfliktlösungen aufräumen", "rerere gc", C, "Löscht alte gemerkte Konfliktlösungen.", vec![]));
    l.push(c("fast_export", Cat::Maintain, "Repository als Datenstrom exportieren", "fast-export", R, "Gibt die Geschichte als Text-Datenstrom aus (für Migrationen).",
        vec![mr("refs", "Was", "--all", "")]));
    l.push(c("fast_import", Cat::Maintain, "Datenstrom importieren", "fast-import", C, "Liest einen fast-export-Datenstrom ein.", vec![stdin("Datenstrom", "Ausgabe von fast-export")]));
    l.push(c("for_each_repo", Cat::Maintain, "Befehl in mehreren Repositories", "for-each-repo", C, "Führt einen Git-Befehl in allen Repositories aus einer Liste in der Konfiguration aus.",
        vec![vr("--config=", "config", "Konfig-Schlüssel", "maintenance.repo", ""), lit("--", "Trennzeichen"), mr("command", "Befehl", "status", "")]));

    // ------------------------------------------------------------------ LFS
    l.push(c("lfs_install", Cat::Lfs, "Git LFS einrichten", "lfs install", C, "Schaltet die Unterstützung für große Dateien ein.", vec![]));
    l.push(c("lfs_uninstall", Cat::Lfs, "Git LFS abschalten", "lfs uninstall", C, "Schaltet die LFS-Unterstützung wieder aus.", vec![]));
    l.push(c("lfs_track", Cat::Lfs, "Dateityp mit LFS verwalten", "lfs track", C, "Große Dateien dieses Typs werden künftig außerhalb des Repositories gespeichert.",
        vec![m("pattern", "Muster", "\"*.psd\"", "leer = aktuelle Liste anzeigen")]));
    l.push(c("lfs_untrack", Cat::Lfs, "LFS-Verwaltung beenden", "lfs untrack", C, "Dateien dieses Typs wieder normal speichern.", vec![mr("pattern", "Muster", "\"*.psd\"", "")]));
    l.push(c("lfs_ls_files", Cat::Lfs, "LFS-Dateien auflisten", "lfs ls-files", R, "Zeigt alle Dateien, die in LFS liegen.", vec![]));
    l.push(c("lfs_status", Cat::Lfs, "LFS-Status", "lfs status", R, "Zeigt geänderte LFS-Dateien.", vec![]));
    l.push(c("lfs_pull", Cat::Lfs, "LFS-Dateien herunterladen", "lfs pull", C, "Lädt die großen Dateien des aktuellen Stands herunter.", vec![o("remote", "Remote", "origin", "")]));
    l.push(c("lfs_fetch", Cat::Lfs, "LFS-Dateien holen", "lfs fetch", R, "Lädt große Dateien herunter, ohne sie einzusetzen.", vec![f("--all", "Alle", ""), o("remote", "Remote", "origin", "")]));
    l.push(c("lfs_push", Cat::Lfs, "LFS-Dateien hochladen", "lfs push", C, "Lädt große Dateien auf den Server.", vec![f("--all", "Alle", ""), t("remote", "Remote", "origin", "")]));
    l.push(c("lfs_prune", Cat::Lfs, "Alte LFS-Dateien löschen", "lfs prune", C, "Entfernt lokal nicht mehr benötigte große Dateien.", vec![f("--dry-run", "Nur testen", "")]));
    l.push(c("lfs_lock", Cat::Lfs, "Datei sperren", "lfs lock", C, "Sperrt eine Datei auf dem Server, damit niemand anderes sie gleichzeitig ändert.", vec![t("file", "Datei", "grafik.psd", "")]));
    l.push(c("lfs_unlock", Cat::Lfs, "Datei entsperren", "lfs unlock", C, "Gibt eine gesperrte Datei wieder frei.", vec![f("--force", "Erzwingen", ""), t("file", "Datei", "grafik.psd", "")]));
    l.push(c("lfs_locks", Cat::Lfs, "Gesperrte Dateien anzeigen", "lfs locks", R, "Zeigt, wer welche Datei gesperrt hat.", vec![]));
    l.push(c("lfs_migrate", Cat::Lfs, "Vorhandene Dateien nach LFS umziehen", "lfs migrate import", X, "Schreibt die Geschichte um, damit große Dateien in LFS liegen.",
        vec![vr("--include=", "pattern", "Muster", "*.psd", ""), f("--everything", "Alle Branches", "")]));
    l.push(c("lfs_env", Cat::Lfs, "LFS-Umgebung", "lfs env", R, "Zeigt die LFS-Konfiguration.", vec![]));

    // ------------------------------------------------------------------ Flow
    l.push(c("flow_init", Cat::Flow, "Git Flow einrichten", "flow init", C, "Richtet die Git-Flow-Branches (main, develop, feature/…) ein. Braucht die git-flow-Erweiterung.", vec![fon("-d", "Standardnamen", "")]));
    l.push(c("flow_feature_start", Cat::Flow, "Feature beginnen", "flow feature start", C, "Legt einen Feature-Branch von develop an.", vec![t("name", "Name", "login", "")]));
    l.push(c("flow_feature_finish", Cat::Flow, "Feature abschließen", "flow feature finish", C, "Führt das Feature in develop zusammen.", vec![t("name", "Name", "login", "")]));
    l.push(c("flow_feature_publish", Cat::Flow, "Feature hochladen", "flow feature publish", C, "Lädt den Feature-Branch auf den Server.", vec![t("name", "Name", "login", "")]));
    l.push(c("flow_release_start", Cat::Flow, "Release beginnen", "flow release start", C, "Legt einen Release-Branch von develop an.", vec![t("name", "Version", "1.2.0", "")]));
    l.push(c("flow_release_finish", Cat::Flow, "Release abschließen", "flow release finish", C, "Führt in main und develop zusammen und erstellt einen Tag.", vec![v("-m", "message", "Tag-Text", "", ""), t("name", "Version", "1.2.0", "")]));
    l.push(c("flow_hotfix_start", Cat::Flow, "Hotfix beginnen", "flow hotfix start", C, "Legt einen Hotfix-Branch von main an.", vec![t("name", "Version", "1.2.1", "")]));
    l.push(c("flow_hotfix_finish", Cat::Flow, "Hotfix abschließen", "flow hotfix finish", C, "Führt den Hotfix in main und develop zusammen.", vec![v("-m", "message", "Tag-Text", "", ""), t("name", "Version", "1.2.1", "")]));

    // ------------------------------------------------------------------ Settings
    l.push(c("config_name", Cat::Settings, "Deinen Namen festlegen", "config", C, "Dieser Name steht in jedem Commit, den du erstellst.",
        vec![ch("scope", "Gültig für", SCOPE), lit("user.name", "Einstellung: Name des Autors"), t("value", "Name", "Max Mustermann", "")]).norepo());
    l.push(c("config_email", Cat::Settings, "Deine E-Mail festlegen", "config", C, "Diese E-Mail steht in jedem Commit, den du erstellst.",
        vec![ch("scope", "Gültig für", SCOPE), lit("user.email", "Einstellung: E-Mail des Autors"), t("value", "E-Mail", "max@beispiel.de", "")]).norepo());
    l.push(c("config_list", Cat::Settings, "Alle Einstellungen anzeigen", "config --list --show-origin", R, "Zeigt alle Git-Einstellungen und aus welcher Datei sie kommen.",
        vec![ch("scope", "Welche", &[("", "Alle", "alle Ebenen"), ("--local", "Nur Repository", ""), ("--global", "Nur global", ""), ("--system", "Nur System", "")])]).norepo());
    l.push(c("config_get", Cat::Settings, "Einstellung lesen", "config --get", R, "Liest den Wert einer Einstellung.", vec![t("key", "Schlüssel", "pull.rebase", "")]).norepo());
    l.push(c("config_set", Cat::Settings, "Einstellung setzen", "config", C, "Setzt eine beliebige Git-Einstellung.",
        vec![ch("scope", "Gültig für", SCOPE), t("key", "Schlüssel", "pull.rebase", ""), t("value", "Wert", "true", "")]).norepo());
    l.push(c("config_unset", Cat::Settings, "Einstellung entfernen", "config --unset", C, "Entfernt eine Einstellung.", vec![ch("scope", "Gültig für", SCOPE), t("key", "Schlüssel", "pull.rebase", "")]).norepo());
    l.push(c("config_alias", Cat::Settings, "Abkürzung (Alias) anlegen", "config", C, "Legt eine eigene Abkürzung für einen Git-Befehl an, z.B. „co“ für „checkout“.",
        vec![ch("scope", "Gültig für", SCOPE), vr("alias.", "name", "Abkürzung", "co", ""), t("value", "Befehl", "checkout", "")]).norepo());
    l.push(c("config_autocrlf", Cat::Settings, "Zeilenenden (Windows/Linux)", "config", C, "Legt fest, wie Git mit Zeilenenden umgeht.",
        vec![ch("scope", "Gültig für", SCOPE), lit("core.autocrlf", "Einstellung: Zeilenenden umwandeln"),
             ch("value", "Wert", &[("true", "true (Windows-Standard)", "beim Auschecken CRLF, beim Speichern LF"), ("input", "input", "beim Speichern LF"), ("false", "false", "nichts umwandeln")])]).norepo());
    l.push(c("credential_fill", Cat::Settings, "Gespeicherte Zugangsdaten abfragen", "credential fill", R, "Fragt den Zugangsdaten-Speicher (Ausgabe enthält das Passwort!).",
        vec![stdin("Anfrage", "protocol=https\nhost=github.com")]));
    l.push(c("credential_reject", Cat::Settings, "Gespeicherte Zugangsdaten löschen", "credential reject", C, "Löscht gespeicherte Zugangsdaten für einen Server, z.B. nach Passwortwechsel.",
        vec![stdin("Server", "protocol=https\nhost=github.com")]));
    l.push(c("help", Cat::Settings, "Hilfe zu einem Befehl", "help", R, "Öffnet die offizielle Anleitung zu einem Git-Befehl.",
        vec![f("-a", "Alle Befehle auflisten", ""), f("-g", "Anleitungen auflisten", ""), o("command", "Befehl", "rebase", "")]).norepo());
    l.push(c("version", Cat::Settings, "Git-Version", "version", R, "Zeigt, welche Git-Version installiert ist.", vec![f("--build-options", "Details", "")]).norepo());
    l.push(c("bugreport", Cat::Settings, "Fehlerbericht erstellen", "bugreport", R, "Erstellt eine Textdatei mit Systeminformationen für einen Fehlerbericht.", vec![v("-o", "dir", "Ordner", "", "")]));
    l.push(c("diagnose", Cat::Settings, "Diagnose-Archiv erstellen", "diagnose", R, "Packt Diagnose-Informationen über das Repository in ein ZIP.", vec![v("-o", "dir", "Ordner", "", "")]));
    l.push(c("var", Cat::Settings, "Git-Variablen anzeigen", "var -l", R, "Zeigt interne Werte wie Editor und Autor.", vec![]));
    l.push(c("hook_run", Cat::Settings, "Hook ausführen", "hook run", C, "Führt ein Hook-Skript (z.B. pre-commit) von Hand aus.", vec![t("name", "Hook", "pre-commit", "")]));

    // ------------------------------------------------------------------ Expert
    let e = Cat::Expert;
    l.push(c("cat_file", e, "Objekt ansehen (cat-file)", "cat-file", R, "Zeigt ein Objekt der Git-Datenbank (Commit, Baum, Datei).",
        vec![ch("mode", "Was", CAT_FILE), t("object", "Objekt", "HEAD oder HEAD:README.md", "")]));
    l.push(c("hash_object", e, "Prüfsumme berechnen (hash-object)", "hash-object", R, "Berechnet die Git-ID für einen Dateiinhalt; mit -w wird er gespeichert.",
        vec![f("-w", "Speichern", "Objekt in die Datenbank schreiben"), f("--stdin", "Text unten verwenden", ""), o("file", "Datei", "", ""), stdin("Inhalt", "")]));
    l.push(c("ls_files", e, "Verfolgte Dateien auflisten (ls-files)", "ls-files", R, "Listet die Dateien, die Git kennt.",
        vec![f("-s", "Mit Index-Infos", ""), f("-o", "Nicht verfolgte", ""), f("-i --exclude-standard", "Ignorierte", ""), f("-m", "Geänderte", ""), f("-u", "Mit Konflikt", ""), paths()]));
    l.push(c("ls_tree", e, "Baum eines Commits (ls-tree)", "ls-tree", R, "Listet Dateien und Ordner eines Commits.",
        vec![f("-r", "Rekursiv", ""), f("--name-only", "Nur Namen", ""), f("-l", "Mit Größe", ""), t("commit", "Commit", "HEAD", "")]));
    l.push(c("rev_parse", e, "Namen in SHA auflösen (rev-parse)", "rev-parse", R, "Wandelt Namen wie „HEAD~2“ oder „main“ in Commit-IDs um.",
        vec![f("--abbrev-ref", "Kurzname statt SHA", ""), f("--show-toplevel", "Hauptordner zeigen", ""), f("--git-dir", "Git-Ordner zeigen", ""), o("ref", "Name", "HEAD", "")]));
    l.push(c("rev_list", e, "Commits auflisten (rev-list)", "rev-list", R, "Listet Commit-IDs; mit „Anzahl“ nur die Zahl.",
        vec![f("--count", "Nur Anzahl", ""), f("--all", "Alle", ""), f("--left-right --count", "Vorsprung/Rückstand", "z.B. bei main...feature"), o("range", "Bereich", "HEAD", "")]));
    l.push(c("for_each_ref", e, "Referenzen auflisten (for-each-ref)", "for-each-ref", R, "Listet Branches, Tags usw. mit frei wählbarem Format.",
        vec![v("--format=", "format", "Format", "%(refname:short) %(objectname:short)", ""), ch("sort", "Sortierung", SORT), o("pattern", "Muster", "refs/heads", "")]));
    l.push(c("show_ref", e, "Referenzen mit SHA (show-ref)", "show-ref", R, "Listet alle Referenzen mit ihrer SHA.", vec![f("--heads", "Nur Branches", ""), f("--tags", "Nur Tags", "")]));
    l.push(c("update_ref", e, "Referenz direkt setzen (update-ref)", "update-ref", X, "Setzt eine Referenz (Branch) direkt auf eine SHA – ohne Prüfungen.",
        vec![f("-d", "Löschen", ""), t("ref", "Referenz", "refs/heads/test", ""), o("commit", "Neue SHA", "", "")]));
    l.push(c("symbolic_ref", e, "Symbolische Referenz (symbolic-ref)", "symbolic-ref", C, "Liest oder setzt, worauf z.B. HEAD zeigt.",
        vec![f("--short", "Kurzform", ""), t("ref", "Name", "HEAD", ""), o("target", "Ziel", "refs/heads/main", "leer = nur lesen")]));
    l.push(c("merge_base", e, "Gemeinsamer Vorfahre (merge-base)", "merge-base", R, "Findet den letzten gemeinsamen Commit zweier Branches.",
        vec![f("--is-ancestor", "Ist A Vorfahre von B?", "Ergebnis über Exit-Code"), t("a", "A", "main", ""), t("b", "B", "feature", "")]));
    l.push(c("name_rev", e, "Commit benennen (name-rev)", "name-rev", R, "Gibt einem Commit einen lesbaren Namen relativ zu Branches/Tags.", vec![f("--tags", "Nur Tags", ""), t("commit", "Commit", "SHA", "")]));
    l.push(c("diff_tree", e, "Commit-Bäume vergleichen (diff-tree)", "diff-tree", R, "Vergleicht die Dateibäume von Commits.", vec![f("-r", "Rekursiv", ""), f("--name-status", "Nur Namen", ""), f("--root", "Auch erster Commit", ""), t("commit", "Commit", "HEAD", ""), o("b", "Zweiter", "", "")]));
    l.push(c("diff_index", e, "Index mit Commit vergleichen (diff-index)", "diff-index", R, "Vergleicht Vormerkung/Dateien mit einem Commit.", vec![f("--cached", "Nur Index", ""), f("--name-status", "Nur Namen", ""), t("commit", "Commit", "HEAD", "")]));
    l.push(c("diff_files", e, "Dateien mit Index vergleichen (diff-files)", "diff-files", R, "Vergleicht Dateien mit der Vormerkung.", vec![f("--name-status", "Nur Namen", "")]));
    l.push(c("read_tree", e, "Baum in den Index lesen (read-tree)", "read-tree", X, "Ersetzt die Vormerkung durch einen Dateibaum.", vec![f("-m", "Zusammenführen", ""), f("-u", "Dateien aktualisieren", ""), t("commit", "Baum", "HEAD", "")]));
    l.push(c("write_tree", e, "Index als Baum speichern (write-tree)", "write-tree", C, "Speichert die Vormerkung als Baum-Objekt.", vec![]));
    l.push(c("commit_tree", e, "Commit-Objekt erzeugen (commit-tree)", "commit-tree", C, "Erzeugt einen Commit aus einem Baum, ohne Branch zu bewegen.",
        vec![vr("-m", "message", "Beschreibung", "", ""), v("-p", "parent", "Eltern-Commit", "", ""), t("tree", "Baum-SHA", "", "")]));
    l.push(c("mktree", e, "Baum aus Liste erzeugen (mktree)", "mktree", C, "Erzeugt ein Baum-Objekt aus einer Liste.", vec![stdin("ls-tree-Ausgabe", "100644 blob <sha>\tdatei.txt")]));
    l.push(c("mktag", e, "Tag-Objekt erzeugen (mktag)", "mktag", C, "Erzeugt ein Tag-Objekt aus Text.", vec![stdin("Tag-Text", "object <sha>\ntype commit\ntag name\ntagger …")]));
    l.push(c("update_index", e, "Index direkt bearbeiten (update-index)", "update-index", C, "Bearbeitet die Vormerkung auf niedriger Ebene.",
        vec![f("--add", "Hinzufügen", ""), f("--remove", "Entfernen", ""), f("--refresh", "Auffrischen", ""), f("--chmod=+x", "Ausführbar machen", ""), f("--chmod=-x", "Nicht ausführbar", ""), m("paths", "Dateien", "", "")]));
    l.push(c("checkout_index", e, "Dateien aus Index schreiben (checkout-index)", "checkout-index", X, "Schreibt Dateien aus der Vormerkung in den Ordner.", vec![f("-a", "Alle", ""), f("-f", "Überschreiben", ""), m("paths", "Dateien", "", "")]));
    l.push(c("check_attr", e, "Attribute prüfen (check-attr)", "check-attr -a", R, "Zeigt .gitattributes-Werte für Dateien.", vec![mr("paths", "Dateien", "bild.png", "")]));
    l.push(c("check_mailmap", e, "Mailmap prüfen", "check-mailmap", R, "Zeigt, wie ein Kontakt über .mailmap umgeschrieben wird.", vec![t("contact", "Kontakt", "Name <mail>", "")]));
    l.push(c("check_ref_format", e, "Branch-Name prüfen (check-ref-format)", "check-ref-format --branch", R, "Prüft, ob ein Name als Branch-Name erlaubt ist.", vec![t("name", "Name", "feature/neu", "")]).norepo());
    l.push(c("interpret_trailers", e, "Trailer bearbeiten (interpret-trailers)", "interpret-trailers", R, "Fügt Zeilen wie „Signed-off-by:“ an eine Nachricht an.",
        vec![v("--trailer", "trailer", "Trailer", "Reviewed-by: Max", ""), stdin("Nachricht", "")]).norepo());
    l.push(c("stripspace", e, "Leerraum bereinigen (stripspace)", "stripspace", R, "Entfernt überflüssige Leerzeilen und Leerzeichen.", vec![f("-s", "Kommentare entfernen", ""), stdin("Text", "")]).norepo());
    l.push(c("patch_id", e, "Patch-ID berechnen", "patch-id", R, "Berechnet eine ID, die gleiche Änderungen erkennt.", vec![stdin("Patch", "")]));
    l.push(c("merge_tree", e, "Merge testen ohne Dateien (merge-tree)", "merge-tree --write-tree", R, "Berechnet das Merge-Ergebnis zweier Branches, ohne etwas zu ändern.",
        vec![t("a", "A", "main", ""), t("b", "B", "feature", "")]));
    l.push(c("verify_commit", e, "Commit-Signatur prüfen", "verify-commit", R, "Prüft die GPG-Signatur eines Commits.", vec![t("commit", "Commit", "HEAD", "")]));
    l.push(c("unpack_file", e, "Blob in Temp-Datei (unpack-file)", "unpack-file", C, "Schreibt einen Dateiinhalt in eine temporäre Datei.", vec![t("object", "Blob-SHA", "", "")]));
    l.push(c("get_tar_commit_id", e, "Commit-ID aus TAR lesen", "get-tar-commit-id", R, "Liest die Commit-ID aus einem mit git archive erzeugten TAR.", vec![stdin("TAR (nur Text möglich)", "")]));
    l.push(c("column", e, "Text in Spalten (column)", "column --mode=column", R, "Ordnet Zeilen in Spalten an.", vec![stdin("Zeilen", "")]).norepo());
    l.push(c("fmt_merge_msg", e, "Merge-Nachricht formatieren", "fmt-merge-msg", R, "Erzeugt den Standardtext für Merge-Commits.", vec![stdin("FETCH_HEAD-Inhalt", "")]));
    l.push(c("replay", e, "Commits neu abspielen (replay)", "replay", C, "Experimentell: spielt Commits auf neuer Basis ab und gibt Referenz-Updates aus.",
        vec![vr("--onto", "onto", "Auf", "main", ""), t("range", "Bereich", "main..feature", "")]));
    l.push(c("pack_objects", e, "Objekte packen (pack-objects)", "pack-objects --stdout", R, "Packt Objekte zu einem Paket.", vec![stdin("SHA-Liste", "")]));
    l.push(c("index_pack", e, "Pack-Index erzeugen (index-pack)", "index-pack", C, "Erzeugt einen Index für eine Paketdatei.", vec![t("file", "Pack-Datei", "", "")]));
    l.push(c("unpack_objects", e, "Paket entpacken (unpack-objects)", "unpack-objects", C, "Entpackt ein Paket in lose Objekte.", vec![f("-n", "Nur testen", "")]));
    l.push(c("show_index", e, "Pack-Index lesen (show-index)", "show-index", R, "Zeigt den Inhalt eines Pack-Index.", vec![stdin("Index-Inhalt", "")]));
    l.push(c("mailsplit", e, "Mailbox aufteilen (mailsplit)", "mailsplit", C, "Teilt eine Mailbox in einzelne Nachrichten.", vec![vr("-o", "dir", "Zielordner", "", ""), t("file", "Mbox", "", "")]));
    l.push(c("mailinfo", e, "Mail auswerten (mailinfo)", "mailinfo", C, "Trennt eine E-Mail in Nachricht und Patch.", vec![t("msg", "Nachricht-Datei", "msg", ""), t("patch", "Patch-Datei", "patch", ""), stdin("E-Mail", "")]));
    l.push(c("merge_index", e, "Merge für Index-Einträge (merge-index)", "merge-index", C, "Ruft ein Merge-Programm für Konfliktdateien auf.", vec![t("program", "Programm", "git-merge-one-file", ""), lit("-a", "alle Dateien")]));
    l.push(c("pack_redundant", e, "Überflüssige Pakete finden", "pack-redundant --all", R, "Veraltet, aber noch vorhanden.", vec![]));
    l.push(c("daemon", e, "Einfacher Git-Server (daemon)", "daemon", C, "Startet einen schreibgeschützten Git-Server. Läuft, bis er beendet wird.",
        vec![f("--export-all", "Alle Repositories freigeben", ""), v("--base-path=", "path", "Basisordner", "", ""), v("--port=", "port", "Port", "9418", "")]).ext());

    // ------------------------------------------------------------------ Tools
    l.push(c("gitk", Cat::Tools, "Verlaufs-Fenster (gitk)", "!gitk", R, "Öffnet das klassische Git-Verlaufsfenster.", vec![f("--all", "Alle Branches", "")]).ext());
    l.push(c("gui", Cat::Tools, "Git GUI", "gui", R, "Öffnet die mitgelieferte Git-Oberfläche.", vec![]).ext());
    l.push(c("citool", Cat::Tools, "Commit-Werkzeug (citool)", "citool", C, "Öffnet ein Fenster nur zum Committen.", vec![]).ext());
    l.push(c("difftool", Cat::Tools, "Externes Vergleichsprogramm (difftool)", "difftool", R, "Öffnet die Unterschiede in deinem eingestellten Vergleichsprogramm.",
        vec![f("-d", "Ordnervergleich", ""), f("--staged", "Vorgemerktes", ""), fon("-y", "Ohne Nachfrage", ""), o("a", "Von", "", ""), o("b", "Bis", "", "")]).ext());
    l.push(c("mergetool", Cat::Tools, "Externes Merge-Programm (mergetool)", "mergetool", C, "Öffnet Konflikte in deinem eingestellten Merge-Programm.",
        vec![v("--tool=", "tool", "Programm", "kdiff3, meld, winmerge…", ""), fon("-y", "Ohne Nachfrage", ""), m("paths", "Dateien", "", "")]).ext());
    l.push(c("instaweb", Cat::Tools, "Repository im Browser (instaweb)", "instaweb", R, "Startet einen kleinen Webserver mit gitweb (braucht Perl/Webserver).", vec![]).ext());
    l.push(c("svn_clone", Cat::Tools, "Subversion-Repository klonen", "svn clone", C, "Wandelt ein SVN-Repository in Git um.",
        vec![f("--stdlayout", "Standard-Layout (trunk/branches/tags)", ""), t("url", "SVN-Adresse", "", ""), o("path", "Zielordner", "", "")]).norepo());
    l.push(c("svn_rebase", Cat::Tools, "SVN: Neues holen", "svn rebase", C, "Holt neue SVN-Revisionen und setzt deine Commits darauf.", vec![]));
    l.push(c("svn_dcommit", Cat::Tools, "SVN: Commits hochladen", "svn dcommit", C, "Überträgt deine Commits nach SVN.", vec![]));
    l.push(c("p4_clone", Cat::Tools, "Perforce-Depot klonen", "p4 clone", C, "Wandelt ein Perforce-Depot in Git um.", vec![t("depot", "Depot-Pfad", "//depot/projekt@all", "")]).norepo());
    l.push(c("p4_sync", Cat::Tools, "Perforce: Neues holen", "p4 sync", C, "Holt neue Perforce-Änderungen.", vec![]));
    l.push(c("p4_submit", Cat::Tools, "Perforce: Hochladen", "p4 submit", C, "Überträgt deine Commits nach Perforce.", vec![]));
    l.push(c("cvsimport", Cat::Tools, "CVS importieren", "cvsimport", C, "Übernimmt ein CVS-Modul.", vec![v("-d", "root", "CVSROOT", "", ""), t("module", "Modul", "", "")]));
    l.push(c("archimport", Cat::Tools, "GNU Arch importieren", "archimport", C, "Übernimmt ein GNU-Arch-Archiv.", vec![t("archive", "Archiv", "", "")]));
    l.push(c("free", Cat::Tools, "Beliebiger Git-Befehl", "", C,
        "Gib selbst einen Git-Befehl ein (ohne „git“ davor). So ist wirklich jeder Befehl mit jeder Option erreichbar.", vec![]).kind(Kind::Free).norepo());

    l
}

pub fn find(list: &[Cmd], id: &str) -> usize {
    list.iter().position(|c| c.id == id).unwrap_or_else(|| panic!("unknown command {id}"))
}
