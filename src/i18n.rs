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
