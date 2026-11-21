//! Version information of this build.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMMIT: &str = env!("YAGUI_COMMIT");
pub const COMMIT_DATE: &str = env!("YAGUI_COMMIT_DATE");
/// Where releases are published (GitHub owner/name).
pub const REPO: &str = "rkaradas/YAGUI";

/// "windows-x64", "linux-x64", "macos-universal" …: the part of a release file name that says
/// which computer it is for.
pub fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") { "windows-arm64" } else { "windows-x64" }
    } else if cfg!(target_os = "macos") {
        "macos-universal"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x64"
    }
}

/// One line: "YAGUI 1.0.0 (abc12345, 2026-11-21, windows-x64)".
pub fn long() -> String {
    format!("YAGUI {VERSION} ({COMMIT}, {COMMIT_DATE}, {})", platform())
}

/// Parses "v1.2.3" or "1.2.3" (a "-beta" suffix is ignored).
pub fn parse(v: &str) -> Option<(u32, u32, u32)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|x| x.parse::<u32>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

/// Whether `other` is newer than this build.
pub fn is_newer(other: &str) -> bool {
    match (parse(other), parse(VERSION)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert_eq!(parse("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse("2.0"), Some((2, 0, 0)));
        assert_eq!(parse("1.4.0-beta.2"), Some((1, 4, 0)));
        assert!(parse("abc").is_none());
        assert!(is_newer("v999.0.0"));
        assert!(!is_newer("v0.0.1"));
    }
}
