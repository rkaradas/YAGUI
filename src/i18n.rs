//! Interface language: English (default) and German.
//!
//! UI strings are written inline with both languages: `tr("Fetch", "Fetch")`.
//! The command catalogue is written in German; `c()` looks up its English text.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize, Default)]
pub enum Lang {
    #[default]
    En,
    De,
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    LANG.store(if lang == Lang::De { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn get() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 { Lang::De } else { Lang::En }
}

pub fn is_de() -> bool {
    get() == Lang::De
}

/// Pick the string for the current language.
pub fn tr(en: &'static str, de: &'static str) -> &'static str {
    if is_de() { de } else { en }
}

/// Same for owned strings built with `format!`.
pub fn trs(en: String, de: String) -> String {
    if is_de() { de } else { en }
}

/// Shortcut labels are written as on Windows and Linux ("Ctrl+Shift+T", "Strg+Umschalt+T").
/// On macOS they turn into the Mac symbols, which also stand for the keys YAGUI listens to there
/// (app.rs): Command for Ctrl, in Apple's order ⌃⌥⇧⌘; switching tabs stays on Control (⌘Tab
/// belongs to macOS), Show HEAD is ⇧⌘H (⌘H hides the app) and Refresh is ⌘R (F5 needs Fn).
pub fn keys(label: &str) -> std::borrow::Cow<'_, str> {
    if cfg!(target_os = "macos") { std::borrow::Cow::Owned(mac_keys(label)) } else { std::borrow::Cow::Borrowed(label) }
}

fn mac_keys(label: &str) -> String {
    const RULES: [(&str, &str); 13] = [
        ("Ctrl+Shift+Tab", "⌃⇧Tab"),
        ("Strg+Umschalt+Tab", "⌃⇧Tab"),
        ("Ctrl+Tab", "⌃Tab"),
        ("Strg+Tab", "⌃Tab"),
        ("Ctrl+Shift+", "⇧⌘"),
        ("Strg+Umschalt+", "⇧⌘"),
        ("Ctrl+Alt+", "⌥⌘"),
        ("Strg+Alt+", "⌥⌘"),
        ("Ctrl+H", "⇧⌘H"),
        ("Strg+H", "⇧⌘H"),
        ("Ctrl+", "⌘"),
        ("Strg+", "⌘"),
        ("F5", "⌘R"),
    ];
    let mut s = label.to_string();
    for (from, to) in RULES {
        s = s.replace(from, to);
    }
    s
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| crate::catalog_en::EN.iter().copied().collect())
}

/// Catalogue text in the current language (the catalogue itself is German).
pub fn c(de: &'static str) -> &'static str {
    if is_de() {
        return de;
    }
    table().get(de).copied().unwrap_or(de)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_labels_turn_into_mac_symbols() {
        assert_eq!(mac_keys("New Tab   Ctrl+T"), "New Tab   ⌘T");
        assert_eq!(mac_keys("Neuer Tab   Strg+T"), "Neuer Tab   ⌘T");
        assert_eq!(mac_keys("Restore Closed Tab   Ctrl+Shift+T"), "Restore Closed Tab   ⇧⌘T");
        assert_eq!(mac_keys("Geschlossenen Tab wiederherstellen   Strg+Umschalt+T"), "Geschlossenen Tab wiederherstellen   ⇧⌘T");
        assert_eq!(mac_keys("In Konsole öffnen   Strg+Alt+T"), "In Konsole öffnen   ⌥⌘T");
        assert_eq!(mac_keys("Ctrl+Tab / Ctrl+Shift+Tab"), "⌃Tab / ⌃⇧Tab");
        assert_eq!(mac_keys("Vorheriger Tab   Strg+Umschalt+Tab"), "Vorheriger Tab   ⌃⇧Tab");
        assert_eq!(mac_keys("Show HEAD   Ctrl+H"), "Show HEAD   ⇧⌘H");
        assert_eq!(mac_keys("Ctrl++ / Ctrl+- / Ctrl+0"), "⌘+ / ⌘- / ⌘0");
        assert_eq!(mac_keys("Preferences…   Ctrl+,"), "Preferences…   ⌘,");
        assert_eq!(mac_keys("Ctrl+Enter"), "⌘Enter");
        assert_eq!(mac_keys("Find any Git command (Ctrl+Shift+P)"), "Find any Git command (⇧⌘P)");
        assert_eq!(mac_keys("Neu laden   F5"), "Neu laden   ⌘R");
        assert_eq!(mac_keys("no shortcut here"), "no shortcut here");
    }

    #[test]
    fn shortcut_labels_stay_as_written_off_macos() {
        if !cfg!(target_os = "macos") {
            assert_eq!(keys("New Tab   Ctrl+T"), "New Tab   Ctrl+T");
        }
    }
}
