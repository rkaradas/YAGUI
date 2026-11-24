//! A diff taken apart once, when it is loaded: lines with their old and new numbers, the hunks
//! with their raw text (for staging single hunks with `git apply`), and the rows of the unified
//! and the side-by-side view. Drawing then only looks up what is visible; nothing is parsed in
//! a frame.

use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Context,
    Add,
    Del,
    /// "\ No newline at end of file" (belongs to the line before it).
    NoNewline,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub kind: Kind,
    /// Line number in the old file (0 = not there).
    pub old: u32,
    /// Line number in the new file (0 = not there).
    pub new: u32,
    /// The text without the +/-/space prefix and without the line end; tabs are kept.
    pub text: String,
    /// The line ended with CR LF (shown with "Show invisible characters").
    pub cr: bool,
    /// Width in columns (see `Cells`), for scrolling and wrapping.
    pub cols: usize,
    /// The column the last character starts in (wrapping: which visual row it is on).
    pub last_col: usize,
    /// Plain ASCII without tabs: a column is a byte, so a part of the line is a plain slice.
    pub simple: bool,
    /// For long lines that aren't simple: (column, byte offset) of a character every
    /// `MARK_EVERY` columns or so, so a part far to the right is found without walking the line.
    pub marks: Vec<(usize, usize)>,
}

/// How often a long line that isn't simple gets a mark (see `Line::marks`).
pub const MARK_EVERY: usize = 512;

impl Line {
    pub(super) fn new(kind: Kind, old: u32, new: u32, text: &str, cr: bool, tab: usize) -> Self {
        let simple = text.is_ascii() && !text.contains('\t');
        let (cols, last_col, marks) = if simple { (text.len(), text.len().saturating_sub(1), vec![]) } else { columns_marked(text, tab) };
        Line { kind, old, new, text: text.to_string(), cr, cols, last_col, simple, marks }
    }

    /// Where to start walking the line to reach column `col`: (column, byte offset) of a
    /// character at or before it.
    pub fn start_for(&self, col: usize) -> (usize, usize) {
        let i = self.marks.partition_point(|m| m.0 <= col);
        if i == 0 { (0, 0) } else { self.marks[i - 1] }
    }
}

/// Columns a user-perceived character (a grapheme cluster: a letter with its accents, an emoji
/// sequence …) takes in the diff's monospaced grid, like a terminal (Unicode East Asian Width):
/// wide characters and emoji two, others one, a cluster of only invisible marks none. (The
/// drawing fits every piece of text into its columns, see `diffview`; this only decides how
/// many there are.)
pub fn cluster_width(g: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    let mut chars = g.chars();
    let Some(first) = chars.next() else { return 0 };
    let w = first.width().unwrap_or(1);
    // An emoji: wide, or made one by a variation selector, a skin tone or a joiner.
    let emoji = |c: char| matches!(c as u32, 0xFE0F | 0x200D | 0x1F3FB..=0x1F3FF);
    if w == 2 || (w > 0 && g.chars().skip(1).any(emoji)) {
        return 2;
    }
    // Tamil, Malayalam and Sinhala letters are drawn about 1.5 to 1.7 columns wide: two, so
    // they keep their size.
    if matches!(first as u32, 0x0B80..=0x0BFF | 0x0D00..=0x0DFF) && w == 1 {
        return 2;
    }
    // Several letters in one cluster (an Indic conjunct) take what they take apart.
    w + chars.map(|c| c.width().unwrap_or(0)).sum::<usize>()
}

/// Characters the diff font (Consolas) draws exactly one column wide: ASCII, Latin, Greek,
/// Cyrillic, common punctuation, currency signs and box drawing. Text is painted in runs of
/// one kind (this or not), each run at its column and fitted into its columns (see
/// `diffview::segment`).
pub fn grid_exact(c: char) -> bool {
    matches!(c as u32,
        0x0000..=0x052F // ASCII, Latin, IPA, combining marks, Greek, Cyrillic
        | 0x1E00..=0x1EFF // Latin extended additional
        | 0x2000..=0x206F // general punctuation
        | 0x20A0..=0x20CF // currency signs
        | 0x2500..=0x257F // box drawing
    )
}


/// Walks a text by grapheme clusters: their byte offset, the cluster, the column it starts in
/// and how many columns it takes (a tab up to the next tab stop, others per `cluster_width`).
pub struct Cells<'a> {
    clusters: unicode_segmentation::GraphemeIndices<'a>,
    col: usize,
    tab: usize,
}

impl<'a> Iterator for Cells<'a> {
    type Item = (usize, &'a str, usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let (i, g) = self.clusters.next()?;
        let col = self.col;
        let w = if g == "\t" { (col / self.tab + 1) * self.tab - col } else { cluster_width(g) };
        self.col += w;
        Some((i, g, col, w))
    }
}

/// `Cells` of `text`, whose first cluster starts in column `col`.
pub fn cells(text: &str, col: usize, tab: usize) -> Cells<'_> {
    use unicode_segmentation::UnicodeSegmentation;
    Cells { clusters: text.grapheme_indices(true), col, tab: tab.max(1) }
}

/// Columns of a text, the column its last cluster starts in, and, for a long text, marks every
/// `MARK_EVERY` columns (each at the start of a cluster).
fn columns_marked(text: &str, tab: usize) -> (usize, usize, Vec<(usize, usize)>) {
    let (mut end, mut last, mut marks, mut next) = (0usize, 0usize, Vec::new(), MARK_EVERY);
    for (i, _, col, w) in cells(text, 0, tab) {
        if col >= next {
            marks.push((col, i));
            next = col + MARK_EVERY;
        }
        if w > 0 {
            last = col;
        }
        end = col + w;
    }
    (end, last, marks)
}


#[derive(Clone, Debug)]
pub struct Hunk {
    /// The hunk exactly as git wrote it, from its @@ line on (header + this = a patch).
    pub raw: String,
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// What git shows after the second @@ (usually the function the hunk is in).
    pub context: String,
    /// The hunk's lines in `Diff::lines`.
    pub lines: Range<usize>,
}

/// One row of a view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    /// The header row of a hunk (index into `hunks`).
    Hunk(usize),
    /// Unified view: one line.
    Line(usize),
    /// Side by side: the old line on the left, the new one on the right (None = filler).
    Pair(Option<usize>, Option<usize>),
}

#[derive(Default, Debug)]
pub struct Diff {
    /// The file header (everything before the first hunk), needed for patches.
    pub header: String,
    /// Plain-language facts from the header (German; the view translates them).
    pub facts: Vec<String>,
    pub hunks: Vec<Hunk>,
    pub lines: Vec<Line>,
    pub binary: bool,
    /// A combined diff (a merge conflict, `diff --cc`): shown unified, hunks can't be applied.
    pub combined: bool,
    /// More than one file section (e.g. a file turned into a symlink): hunks can't be applied.
    pub multi: bool,
    pub unified: Vec<Row>,
    pub split: Vec<Row>,
    /// The widest line in columns.
    pub max_cols: usize,
    /// The largest line number.
    pub max_no: u32,
    /// The tab width the column counts were made with (drawing uses the same).
    pub tab: usize,
    /// A submodule's commit changed ("Subproject commit …"): nothing a hunk patch could do.
    pub gitlink: bool,
    /// git's output was not valid UTF-8 (a file in Windows-1252, for example); see `hunk_safe`.
    pub lossy: bool,
    /// Different for every parsed diff (drawing caches per diff).
    pub id: u64,
}

impl Diff {
    /// The patch that applies only hunk `h` with `git apply [--cached] [--reverse]
    /// --unidiff-zero`: forward to stage it, `reverse` to unstage or discard it.
    ///
    /// git apply looks for a hunk from the line its header gives for the result side, counted
    /// as if all earlier hunks of the diff were applied too. Applied alone, a hunk after other
    /// changes would be looked for in the wrong place, and without context lines (`-U0`) it
    /// would land there. So the header is rewritten for applying this hunk alone: forward the
    /// new start follows from the old start (the index is the old side), reverse the old start
    /// from the new start (the file is the new side).
    ///
    /// The file header is cut down to the file itself: a rename or mode change in it would be
    /// staged or undone together with the hunk. New and deleted files keep theirs (their one
    /// hunk is the whole file, so it creates or removes the file).
    pub fn hunk_patch(&self, h: usize, reverse: bool) -> String {
        let hk = &self.hunks[h];
        let (mut a, b, mut c, d) = (hk.old_start, hk.old_len, hk.new_start, hk.new_len);
        // In a range of length 0 the start is the line before it.
        if reverse {
            let before = if d > 0 { c.saturating_sub(1) } else { c };
            a = if b > 0 { before + 1 } else { before };
        } else {
            let before = if b > 0 { a.saturating_sub(1) } else { a };
            c = if d > 0 { before + 1 } else { before };
        }
        let first_end = hk.raw.find('\n').map(|i| i + 1).unwrap_or(hk.raw.len());
        let (first, body) = hk.raw.split_at(first_end);
        // What follows the second "@@" (the function name and the line end) stays as it is.
        let rest = first.get(2..).and_then(|s| s.find("@@").map(|i| &s[i + 2..])).unwrap_or("\n");
        format!("{}@@ -{a},{b} +{c},{d} @@{rest}{body}", self.file_header())
    }

    /// The header for a hunk patch: the file's path on both sides (see `hunk_patch`).
    fn file_header(&self) -> String {
        let lines: Vec<&str> = self.header.lines().collect();
        let whole_file = lines.iter().any(|l| l.starts_with("new file mode ") || l.starts_with("deleted file mode ") || *l == "--- /dev/null" || *l == "+++ /dev/null");
        let Some(b_name) = lines.iter().find_map(|l| l.strip_prefix("+++ ")) else {
            return self.header.clone();
        };
        if whole_file {
            return self.header.clone();
        }
        // git ends the name with a tab when it contains a space.
        let b_name = b_name.strip_suffix('\t').unwrap_or(b_name);
        let a_name = if let Some(p) = b_name.strip_prefix("\"b/") {
            format!("\"a/{p}")
        } else if let Some(p) = b_name.strip_prefix("b/") {
            format!("a/{p}")
        } else {
            return self.header.clone();
        };
        let tab = |n: &str| if n.contains(' ') { "\t" } else { "" };
        // The "index a..b mode" line says what kind of file it is (a symlink, executable):
        // without it git apply would write a symlink's target as a plain file. (Its mode is
        // only there when unchanged; a mode change's own lines stay out, see above.)
        let index = lines.iter().find(|l| l.starts_with("index ") && l.split_whitespace().count() == 3).map(|l| format!("{l}\n")).unwrap_or_default();
        format!("diff --git {a_name} {b_name}\n{index}--- {a_name}{}\n+++ {b_name}{}\n", tab(&a_name), tab(b_name))
    }

    /// Single hunks can be applied (staged, unstaged, discarded).
    pub fn hunks_applicable(&self) -> bool {
        !self.binary && !self.combined && !self.multi && !self.gitlink
    }

    /// Hunk `h` can be written back exactly. Bytes that aren't UTF-8 became U+FFFD when the
    /// diff was read; a patch with them would put U+FFFD into the file or fail to match.
    pub fn hunk_safe(&self, h: usize) -> bool {
        !self.lossy || !(self.header.contains('\u{FFFD}') || self.hunks[h].raw.contains('\u{FFFD}'))
    }
}

/// Columns a text takes with tab stops every `tab` columns (wide characters per `cell_width`).
#[cfg(test)]
pub fn columns(text: &str, tab: usize) -> usize {
    columns_marked(text, tab).0
}

/// Takes a diff of one file apart. `tab` is the tab width used for the column counts.
pub fn parse(text: &str, tab: usize) -> Diff {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let tab = tab.max(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut d = Diff { tab, id, ..Default::default() };
    let mut in_hunks = false;
    // After a second "diff --git" (multi-section diffs) its header lines are skipped.
    let mut skipping_header = false;
    let mut cur: Option<usize> = None;
    let (mut old_no, mut new_no) = (0u32, 0u32);
    // Lines the current hunk still has on each side (from its header).
    let (mut old_left, mut new_left) = (0u32, 0u32);
    let mut parents = 1usize;

    for raw in text.split_inclusive('\n') {
        if raw.starts_with("@@") {
            if let Some(h) = cur {
                d.hunks[h].lines.end = d.lines.len();
            }
            let at = raw.chars().take_while(|c| *c == '@').count();
            if at > 2 {
                d.combined = true;
                parents = at - 1;
            }
            let (os, ol, ns, nl, ctx) = hunk_header(raw);
            d.hunks.push(Hunk { raw: raw.to_string(), old_start: os, old_len: ol, new_start: ns, new_len: nl, context: ctx, lines: d.lines.len()..d.lines.len() });
            cur = Some(d.hunks.len() - 1);
            old_no = os;
            new_no = ns;
            (old_left, new_left) = (ol, nl);
            in_hunks = true;
            skipping_header = false;
            continue;
        }
        if !in_hunks {
            d.header.push_str(raw);
            header_fact(&mut d, raw.trim_end());
            if raw.starts_with("diff --cc ") || raw.starts_with("diff --combined ") {
                d.combined = true;
            }
            continue;
        }
        if raw.starts_with("diff --git ") || raw.starts_with("diff --cc ") {
            d.multi = true;
            if let Some(h) = cur.take() {
                d.hunks[h].lines.end = d.lines.len();
            }
            skipping_header = true;
            continue;
        }
        let Some(h) = cur else {
            if skipping_header && (raw.starts_with("Binary files ") || raw.trim_end() == "GIT binary patch") {
                d.binary = true;
            }
            continue;
        };
        d.hunks[h].raw.push_str(raw);
        let body = raw.strip_suffix('\n').unwrap_or(raw);
        let (body, cr) = match body.strip_suffix('\r') {
            Some(b) => (b, true),
            None => (body, false),
        };
        if d.combined {
            let split = body.char_indices().nth(parents).map(|(i, _)| i).unwrap_or(body.len());
            let (pre, rest) = body.split_at(split);
            let kind = if pre.contains('-') {
                Kind::Del
            } else if pre.contains('+') {
                Kind::Add
            } else if pre.starts_with('\\') {
                Kind::NoNewline
            } else {
                Kind::Context
            };
            let text = if kind == Kind::NoNewline { body } else { rest };
            d.lines.push(Line::new(kind, 0, 0, text, cr, tab));
            continue;
        }
        let mut chars = body.chars();
        let (kind, text) = match chars.next() {
            Some('+') => (Kind::Add, chars.as_str()),
            Some('-') => (Kind::Del, chars.as_str()),
            Some('\\') => (Kind::NoNewline, body),
            Some(' ') => (Kind::Context, chars.as_str()),
            // An empty context line written without its space (git does that with
            // diff.suppressBlankEmpty; git apply takes it too).
            None if old_left > 0 && new_left > 0 => (Kind::Context, ""),
            // Otherwise an empty line is not part of the hunk; it stays in the patch only.
            None => continue,
            Some(_) => (Kind::Context, body),
        };
        let (old, new) = match kind {
            Kind::Add => {
                new_no += 1;
                new_left = new_left.saturating_sub(1);
                (0, new_no - 1)
            }
            Kind::Del => {
                old_no += 1;
                old_left = old_left.saturating_sub(1);
                (old_no - 1, 0)
            }
            Kind::Context => {
                old_no += 1;
                new_no += 1;
                old_left = old_left.saturating_sub(1);
                new_left = new_left.saturating_sub(1);
                (old_no - 1, new_no - 1)
            }
            Kind::NoNewline => (0, 0),
        };
        d.lines.push(Line::new(kind, old, new, text, cr, tab));
    }
    if let Some(h) = cur {
        d.hunks[h].lines.end = d.lines.len();
    }
    d.max_cols = d.lines.iter().map(|l| l.cols).max().unwrap_or(0);
    d.max_no = d.lines.iter().map(|l| l.old.max(l.new)).max().unwrap_or(0);
    // A submodule: its "file" is a commit id (mode 160000), shown as "Subproject commit …".
    let mut changed = d.lines.iter().filter(|l| matches!(l.kind, Kind::Add | Kind::Del)).peekable();
    let only_subproject = changed.peek().is_some() && changed.all(|l| l.text.starts_with("Subproject commit "));
    d.gitlink = only_subproject || d.header.lines().any(|l| (l.starts_with("index ") && l.ends_with(" 160000")) || l.ends_with(" mode 160000"));
    build_rows(&mut d);
    d
}

/// "@@ -12,5 +12,7 @@ fn context" -> (12, 5, 12, 7, "fn context"); a missing length is 1.
/// Combined headers ("@@@ -1,3 -1,3 +1,7 @@@") give the first old and the new range.
fn hunk_header(line: &str) -> (u32, u32, u32, u32, String) {
    let at = line.chars().take_while(|c| *c == '@').count();
    let marker = "@".repeat(at);
    let mut parts = line.trim_end().splitn(2, &marker).nth(1).unwrap_or("").splitn(2, &marker);
    let ranges = parts.next().unwrap_or("");
    let ctx = parts.next().unwrap_or("").trim().to_string();
    let range = |r: &str| -> (u32, u32) {
        let mut it = r.split(',');
        let start = it.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let len = it.next().and_then(|n| n.parse().ok()).unwrap_or(1);
        (start, len)
    };
    let (mut os, mut ol, mut ns, mut nl) = (0, 0, 0, 0);
    let mut seen_old = false;
    for r in ranges.split_whitespace() {
        if let Some(o) = r.strip_prefix('-') {
            if !seen_old {
                (os, ol) = range(o);
                seen_old = true;
            }
        } else if let Some(n) = r.strip_prefix('+') {
            (ns, nl) = range(n);
        }
    }
    (os, ol, ns, nl, ctx)
}

fn header_fact(d: &mut Diff, l: &str) {
    let fact = if let Some(m) = l.strip_prefix("new file mode ") {
        Some(format!("Neue Datei (Modus {m})"))
    } else if let Some(m) = l.strip_prefix("deleted file mode ") {
        Some(format!("Datei gelöscht (Modus {m})"))
    } else if let Some(m) = l.strip_prefix("old mode ") {
        Some(format!("Dateimodus vorher {m}"))
    } else if let Some(m) = l.strip_prefix("new mode ") {
        Some(format!("Dateimodus jetzt {m}"))
    } else if let Some(s) = l.strip_prefix("similarity index ") {
        Some(format!("Ähnlichkeit {s}"))
    } else if let Some(p) = l.strip_prefix("rename from ") {
        Some(format!("Umbenannt von {p}"))
    } else if let Some(p) = l.strip_prefix("rename to ") {
        Some(format!("Umbenannt nach {p}"))
    } else if let Some(p) = l.strip_prefix("copy from ") {
        Some(format!("Kopiert von {p}"))
    } else if let Some(p) = l.strip_prefix("copy to ") {
        Some(format!("Kopiert nach {p}"))
    } else if l.starts_with("Binary files ") || l == "GIT binary patch" {
        d.binary = true;
        Some("Binärdatei – Inhalt kann nicht zeilenweise gezeigt werden".to_string())
    } else {
        None
    };
    if let Some(f) = fact {
        d.facts.push(f);
    }
}

fn build_rows(d: &mut Diff) {
    for (h, hunk) in d.hunks.iter().enumerate() {
        d.unified.push(Row::Hunk(h));
        d.unified.extend(hunk.lines.clone().map(Row::Line));
    }
    if d.combined {
        d.split = d.unified.clone();
        return;
    }
    for (h, hunk) in d.hunks.iter().enumerate() {
        d.split.push(Row::Hunk(h));
        let end = hunk.lines.end;
        let mut i = hunk.lines.start;
        // The side of the line before, for "\ No newline" markers: 0 both, 1 left, 2 right.
        let mut side = 0u8;
        while i < end {
            match d.lines[i].kind {
                Kind::Context => {
                    d.split.push(Row::Pair(Some(i), Some(i)));
                    side = 0;
                    i += 1;
                }
                Kind::NoNewline => {
                    d.split.push(match side {
                        1 => Row::Pair(Some(i), None),
                        2 => Row::Pair(None, Some(i)),
                        _ => Row::Pair(Some(i), Some(i)),
                    });
                    i += 1;
                }
                Kind::Del | Kind::Add => {
                    // A change block: removed lines, then added lines, paired up row by row.
                    let mut dels = Vec::new();
                    while i < end && (d.lines[i].kind == Kind::Del || (d.lines[i].kind == Kind::NoNewline && !dels.is_empty())) {
                        dels.push(i);
                        i += 1;
                    }
                    let mut adds = Vec::new();
                    while i < end && (d.lines[i].kind == Kind::Add || (d.lines[i].kind == Kind::NoNewline && !adds.is_empty())) {
                        adds.push(i);
                        i += 1;
                    }
                    for k in 0..dels.len().max(adds.len()) {
                        d.split.push(Row::Pair(dels.get(k).copied(), adds.get(k).copied()));
                    }
                    side = if !adds.is_empty() { 2 } else { 1 };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODIFY: &str = "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,5 @@ fn main\n one\n-two\n-three\n+TWO\n+THREE\n+FOUR\n four\n";

    #[test]
    fn lines_get_their_old_and_new_numbers() {
        let d = parse(MODIFY, 4);
        assert_eq!(d.hunks.len(), 1);
        let h = &d.hunks[0];
        assert_eq!((h.old_start, h.old_len, h.new_start, h.new_len, h.context.as_str()), (1, 4, 1, 5, "fn main"));
        let got: Vec<(Kind, u32, u32, &str)> = d.lines.iter().map(|l| (l.kind, l.old, l.new, l.text.as_str())).collect();
        assert_eq!(
            got,
            vec![
                (Kind::Context, 1, 1, "one"),
                (Kind::Del, 2, 0, "two"),
                (Kind::Del, 3, 0, "three"),
                (Kind::Add, 0, 2, "TWO"),
                (Kind::Add, 0, 3, "THREE"),
                (Kind::Add, 0, 4, "FOUR"),
                (Kind::Context, 4, 5, "four"),
            ]
        );
        assert_eq!(d.max_no, 5);
        assert!(d.hunks_applicable());
    }

    #[test]
    fn a_hunk_patch_is_the_file_and_the_hunk() {
        let d = parse(MODIFY, 4);
        let want = "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,5 @@ fn main\n one\n-two\n-three\n+TWO\n+THREE\n+FOUR\n four\n";
        assert_eq!(d.hunk_patch(0, false), want);
        assert_eq!(d.hunk_patch(0, true), want);
        assert!(d.header.starts_with("diff --git a/f.txt b/f.txt\n") && d.header.ends_with("+++ b/f.txt\n"));
    }

    #[test]
    fn hunk_patches_start_where_the_hunk_is_alone() {
        // Hunk 1 comes after a hunk that added two lines: on its own it starts at the old line
        // (staging into the index) or at the new line (undoing in the file).
        let t = "--- a/x\n+++ b/x\n@@ -1 +1,3 @@\n-a\n+b\n+c\n+d\n@@ -10,2 +12,2 @@ ctx\n x\n-y\n+z\n";
        let d = parse(t, 4);
        assert!(d.hunk_patch(1, false).ends_with("@@ -10,2 +10,2 @@ ctx\n x\n-y\n+z\n"));
        assert!(d.hunk_patch(1, true).ends_with("@@ -12,2 +12,2 @@ ctx\n x\n-y\n+z\n"));
        // Without context: an insertion after old line 5 / a deletion of old line 20.
        // (An empty range gives the line before it: old 19 is new 23 after the 4 added lines.)
        let t = "--- a/x\n+++ b/x\n@@ -1 +1,3 @@\n-a\n+b\n+c\n+d\n@@ -5,0 +8,2 @@\n+n1\n+n2\n@@ -20 +23,0 @@\n-gone\n";
        let d = parse(t, 4);
        assert!(d.hunk_patch(1, false).contains("@@ -5,0 +6,2 @@\n"));
        assert!(d.hunk_patch(1, true).contains("@@ -7,0 +8,2 @@\n"));
        assert!(d.hunk_patch(2, false).contains("@@ -20,1 +19,0 @@\n"));
        assert!(d.hunk_patch(2, true).contains("@@ -24,1 +23,0 @@\n"));
    }

    #[test]
    fn hunk_patches_leave_renames_and_modes_out() {
        let t = "diff --git a/old name.txt b/new name.txt\nsimilarity index 90%\nrename from old name.txt\nrename to new name.txt\nold mode 100644\nnew mode 100755\nindex 1..2\n--- a/old name.txt\t\n+++ b/new name.txt\t\n@@ -1 +1 @@\n-a\n+b\n";
        let p = parse(t, 4).hunk_patch(0, true);
        assert_eq!(p, "diff --git a/new name.txt b/new name.txt\n--- a/new name.txt\t\n+++ b/new name.txt\t\n@@ -1,1 +1,1 @@\n-a\n+b\n");
        let quoted = "diff --git \"a/\\303\\244.txt\" \"b/\\303\\244.txt\"\nindex 1..2 100644\n--- \"a/\\303\\244.txt\"\n+++ \"b/\\303\\244.txt\"\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(parse(quoted, 4).hunk_patch(0, false).starts_with("diff --git \"a/\\303\\244.txt\" \"b/\\303\\244.txt\"\nindex 1..2 100644\n--- \"a/\\303\\244.txt\"\n+++ \"b/\\303\\244.txt\"\n"));
        // New files keep their header: their one hunk creates or removes the file.
        let added = "diff --git a/n.txt b/n.txt\nnew file mode 100644\nindex 0..1\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,2 @@\n+a\n+b\n";
        assert_eq!(parse(added, 4).hunk_patch(0, true), added);
    }

    #[test]
    fn blank_context_lines_without_their_space_count() {
        // diff.suppressBlankEmpty writes an empty context line as "".
        let t = "--- a/x\n+++ b/x\n@@ -1,4 +1,4 @@\n a\n\n-b\n+B\n c\n";
        let d = parse(t, 4);
        let got: Vec<(Kind, u32, u32, &str)> = d.lines.iter().map(|l| (l.kind, l.old, l.new, l.text.as_str())).collect();
        assert_eq!(got, vec![(Kind::Context, 1, 1, "a"), (Kind::Context, 2, 2, ""), (Kind::Del, 3, 0, "b"), (Kind::Add, 0, 3, "B"), (Kind::Context, 4, 4, "c")]);
    }

    #[test]
    fn side_by_side_pairs_removed_and_added_lines() {
        let d = parse(MODIFY, 4);
        assert_eq!(d.unified.len(), 1 + 7);
        assert_eq!(
            d.split,
            vec![Row::Hunk(0), Row::Pair(Some(0), Some(0)), Row::Pair(Some(1), Some(3)), Row::Pair(Some(2), Some(4)), Row::Pair(None, Some(5)), Row::Pair(Some(6), Some(6))]
        );
    }

    #[test]
    fn new_file_and_several_hunks() {
        let added = "diff --git a/n.txt b/n.txt\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,2 @@\n+a\n+b\n";
        let d = parse(added, 4);
        assert_eq!(d.facts, vec!["Neue Datei (Modus 100644)".to_string()]);
        assert_eq!(d.lines.iter().map(|l| (l.old, l.new)).collect::<Vec<_>>(), vec![(0, 1), (0, 2)]);
        assert_eq!(d.split, vec![Row::Hunk(0), Row::Pair(None, Some(0)), Row::Pair(None, Some(1))]);

        let two = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n@@ -10,2 +10,2 @@\n x\n-y\n+z\n";
        let d = parse(two, 4);
        assert_eq!(d.hunks.len(), 2);
        assert_eq!((d.hunks[0].old_len, d.hunks[0].new_len), (1, 1));
        assert_eq!(d.hunks[1].lines, 2..5);
        assert_eq!(d.lines[2].old, 10);
        assert_eq!((d.lines[3].old, d.lines[4].new), (11, 11));
        assert_eq!(d.hunk_patch(1, false), "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -10,2 +10,2 @@\n x\n-y\n+z\n");
    }

    #[test]
    fn no_newline_markers_stay_in_the_patch_and_follow_their_side() {
        let t = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n";
        let d = parse(t, 4);
        assert!(d.hunks[0].raw.ends_with("+new\n\\ No newline at end of file\n"));
        assert_eq!(d.lines.iter().map(|l| l.kind).collect::<Vec<_>>(), vec![Kind::Del, Kind::NoNewline, Kind::Add, Kind::NoNewline]);
        assert_eq!(d.split, vec![Row::Hunk(0), Row::Pair(Some(0), Some(2)), Row::Pair(Some(1), Some(3))]);
    }

    #[test]
    fn crlf_line_ends_are_noticed_and_cut_off() {
        let d = parse("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\r\n+a\n", 4);
        assert!(d.lines[0].cr && !d.lines[1].cr);
        assert_eq!(d.lines[0].text, "a");
    }

    #[test]
    fn tabs_count_to_the_next_tab_stop() {
        assert_eq!(columns("\tx", 4), 5);
        assert_eq!(columns("ab\tx", 4), 5);
        assert_eq!(columns("abcd\tx", 4), 9);
        let d = parse("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-\t\tx\n+y\n", 4);
        assert_eq!(d.max_cols, 9);
    }

    #[test]
    fn binary_combined_and_multi_section_diffs_are_not_applicable() {
        let bin = parse("diff --git a/i.png b/i.png\nindex 1..2 100644\nBinary files a/i.png and b/i.png differ\n", 4);
        assert!(bin.binary && bin.hunks.is_empty() && !bin.hunks_applicable());

        let cc = "diff --cc c.txt\nindex 1,2..3\n--- a/c.txt\n+++ b/c.txt\n@@@ -1,3 -1,3 +1,7 @@@\n  same\n++<<<<<<< HEAD\n +ours\n+ theirs\n";
        let d = parse(cc, 4);
        assert!(d.combined && !d.hunks_applicable());
        assert_eq!((d.hunks[0].old_start, d.hunks[0].new_start, d.hunks[0].new_len), (1, 1, 7));
        assert_eq!(d.lines.iter().map(|l| (l.kind, l.text.as_str())).collect::<Vec<_>>(), vec![(Kind::Context, "same"), (Kind::Add, "<<<<<<< HEAD"), (Kind::Add, "ours"), (Kind::Add, "theirs")]);
        assert_eq!(d.split, d.unified);

        let multi = "diff --git a/l b/l\ndeleted file mode 100644\n--- a/l\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\ndiff --git a/l b/l\nnew file mode 120000\n--- /dev/null\n+++ b/l\n@@ -0,0 +1 @@\n+target\n";
        let d = parse(multi, 4);
        assert!(d.multi && !d.hunks_applicable());
        assert_eq!(d.hunks.len(), 2);
        assert!(!d.hunks[0].raw.contains("new file mode"));
        assert_eq!(d.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["x", "target"]);
    }

    #[test]
    fn rename_facts_and_empty_diffs() {
        let d = parse("diff --git a/a b/b\nsimilarity index 100%\nrename from a\nrename to b\n", 4);
        assert_eq!(d.facts, vec!["Ähnlichkeit 100%", "Umbenannt von a", "Umbenannt nach b"]);
        assert!(d.hunks.is_empty() && d.unified.is_empty());
        assert!(parse("", 4).hunks.is_empty());
    }

    use crate::git::{self, DiffOpts, FileStatus};
    use std::path::{Path, PathBuf};

    /// A fresh repository in the temp folder, independent of the user's git settings.
    fn test_repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yagui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            &["init", "-q"][..],
            &["config", "user.name", "Test"],
            &["config", "user.email", "test@example.invalid"],
            &["config", "core.autocrlf", "false"],
            &["config", "commit.gpgsign", "false"],
        ] {
            ok(&dir, args);
        }
        dir
    }

    fn ok(dir: &Path, args: &[&str]) -> String {
        let o = git::run(dir, args);
        assert!(o.ok(), "git {args:?}: {}", o.stderr);
        o.stdout
    }

    /// `git apply <flags> --unidiff-zero -` with the patch, as the Changes view runs it.
    fn apply(dir: &Path, flags: &[&str], patch: &str) {
        let mut args: Vec<String> = vec!["apply".into()];
        args.extend(flags.iter().map(|s| s.to_string()));
        args.extend(["--whitespace=nowarn".to_string(), "--unidiff-zero".into(), "-".into()]);
        let o = git::run_owned(Some(dir), &args, Some(patch));
        assert!(o.ok(), "git apply {flags:?} failed: {}\n{patch}", o.stderr);
    }

    fn write(dir: &Path, file: &str, text: &str) {
        std::fs::write(dir.join(file), text).unwrap();
    }

    /// Staging, discarding and unstaging every single hunk the way the Changes view does it,
    /// with real git and the context sizes of the diff toolbar: exactly that hunk changes.
    /// (git apply looks for a hunk where it would be with the earlier hunks applied; without
    /// context lines a hunk applied alone used to land in the wrong place.)
    #[test]
    fn single_hunks_with_git_apply() {
        let dir = test_repo("hunks");
        let old: Vec<String> = (1..=40).map(|i| format!("line {i}")).collect();
        // Changes far enough apart to be separate hunks even with 3 lines of context:
        // (first old line touched, 0-based; old lines removed; new lines).
        let edits: [(usize, usize, &[&str]); 4] = [(1, 0, &["ins A", "ins B"]), (11, 1, &[]), (21, 1, &["line 22 changed"]), (32, 0, &["ins C"])];
        // The file with only the edits in `with` made.
        let text = |with: &[usize]| -> String {
            let (mut out, mut i) = (Vec::<String>::new(), 0);
            for (k, (at, del, ins)) in edits.iter().enumerate() {
                out.extend(old[i..*at].iter().cloned());
                i = *at;
                if with.contains(&k) {
                    out.extend(ins.iter().map(|s| s.to_string()));
                    i += del;
                }
            }
            out.extend(old[i..].iter().cloned());
            out.join("\n") + "\n"
        };
        let all = [0, 1, 2, 3];
        let but = |h: usize| -> Vec<usize> { all.iter().copied().filter(|k| *k != h).collect() };
        write(&dir, "f.txt", &text(&[]));
        ok(&dir, &["add", "f.txt"]);
        ok(&dir, &["commit", "-q", "--no-verify", "-m", "one"]);
        let unstaged = FileStatus { path: "f.txt".into(), orig: None, index: ' ', worktree: 'M' };
        let staged = FileStatus { path: "f.txt".into(), orig: None, index: 'M', worktree: ' ' };
        let index = |dir: &Path| ok(dir, &["show", ":f.txt"]);
        for (context, full) in [(3, false), (1, false), (0, false), (0, true)] {
            let opts = DiffOpts { context, ignore_ws: false, full };
            let n = if full { 1 } else { 4 };
            let what = format!("context {context}, whole file {full}");
            // Stage Hunk: the index gets only that hunk.
            write(&dir, "f.txt", &text(&all));
            ok(&dir, &["reset", "-q"]);
            let d = parse(&git::working_diff(&dir, &unstaged, false, &opts).stdout, 4);
            assert_eq!(d.hunks.len(), n, "{what}");
            for h in 0..n {
                ok(&dir, &["reset", "-q"]);
                apply(&dir, &["--cached"], &d.hunk_patch(h, false));
                assert_eq!(index(&dir), if full { text(&all) } else { text(&[h]) }, "stage hunk {h}, {what}");
            }
            // Discard Hunk: the file loses only that hunk.
            ok(&dir, &["reset", "-q"]);
            for h in 0..n {
                write(&dir, "f.txt", &text(&all));
                apply(&dir, &["--reverse"], &d.hunk_patch(h, true));
                let now = std::fs::read_to_string(dir.join("f.txt")).unwrap();
                assert_eq!(now, if full { text(&[]) } else { text(&but(h)) }, "discard hunk {h}, {what}");
            }
            // Unstage Hunk: the index loses only that hunk.
            write(&dir, "f.txt", &text(&all));
            ok(&dir, &["add", "f.txt"]);
            let d = parse(&git::working_diff(&dir, &staged, true, &opts).stdout, 4);
            assert_eq!(d.hunks.len(), n, "{what}");
            for h in 0..n {
                ok(&dir, &["add", "f.txt"]);
                apply(&dir, &["--cached", "--reverse"], &d.hunk_patch(h, true));
                assert_eq!(index(&dir), if full { text(&[]) } else { text(&but(h)) }, "unstage hunk {h}, {what}");
            }
            ok(&dir, &["reset", "-q"]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Unstaging a hunk of a staged rename keeps the rename (the patch used to carry it along
    /// and undo it).
    #[test]
    fn unstaging_a_hunk_keeps_a_staged_rename() {
        let dir = test_repo("rename");
        let old: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
        write(&dir, "old name.txt", &(old.join("\n") + "\n"));
        ok(&dir, &["add", "-A"]);
        ok(&dir, &["commit", "-q", "--no-verify", "-m", "one"]);
        ok(&dir, &["mv", "old name.txt", "new name.txt"]);
        let mut new = old.clone();
        new[1] = "line 2 changed".into();
        new[24] = "line 25 changed".into();
        write(&dir, "new name.txt", &(new.join("\n") + "\n"));
        ok(&dir, &["add", "-A"]);
        let f = FileStatus { path: "new name.txt".into(), orig: Some("old name.txt".into()), index: 'R', worktree: ' ' };
        let d = parse(&git::working_diff(&dir, &f, true, &DiffOpts::default()).stdout, 4);
        assert!(d.facts.iter().any(|f| f.starts_with("Umbenannt")), "{:?}", d.facts);
        assert_eq!(d.hunks.len(), 2);
        apply(&dir, &["--cached", "--reverse"], &d.hunk_patch(0, true));
        let files = ok(&dir, &["ls-files"]);
        assert_eq!(files.lines().collect::<Vec<_>>(), vec!["new name.txt"]);
        let mut want = old.clone();
        want[24] = "line 25 changed".into();
        assert_eq!(ok(&dir, &["show", ":new name.txt"]), want.join("\n") + "\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With diff.suppressBlankEmpty git writes empty context lines as "": the line numbers
    /// still count them and the hunk still stages.
    #[test]
    fn blank_context_lines_from_git() {
        let dir = test_repo("blank");
        ok(&dir, &["config", "diff.suppressBlankEmpty", "true"]);
        write(&dir, "b.txt", "a\n\nb\nc\n\nd\n");
        ok(&dir, &["add", "-A"]);
        ok(&dir, &["commit", "-q", "--no-verify", "-m", "one"]);
        write(&dir, "b.txt", "a\n\nb\nC\n\nd\n");
        let f = FileStatus { path: "b.txt".into(), orig: None, index: ' ', worktree: 'M' };
        let text = git::working_diff(&dir, &f, false, &DiffOpts::default()).stdout;
        assert!(text.contains("\n\n"), "git wrote no bare empty line:\n{text}");
        let d = parse(&text, 4);
        let added: Vec<(u32, &str)> = d.lines.iter().filter(|l| l.kind == Kind::Add).map(|l| (l.new, l.text.as_str())).collect();
        assert_eq!(added, vec![(4, "C")]);
        assert_eq!(d.lines.iter().filter(|l| l.kind == Kind::Context).count(), 5);
        apply(&dir, &["--cached"], &d.hunk_patch(0, false));
        assert_eq!(ok(&dir, &["show", ":b.txt"]), "a\n\nb\nC\n\nd\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A Windows-1252 file: the hunk with such bytes is not offered (its patch would write
    /// U+FFFD), a hunk with plain text still stages and leaves the other bytes alone.
    #[test]
    fn hunks_with_bytes_that_arent_utf8() {
        let dir = test_repo("latin1");
        let mut old: Vec<Vec<u8>> = (1..=30).map(|i| format!("line {i}").into_bytes()).collect();
        old[1] = b"Gr\xf6\xdfe 2".to_vec();
        let join = |v: &[Vec<u8>]| v.iter().flat_map(|l| l.iter().copied().chain([b'\n'])).collect::<Vec<u8>>();
        std::fs::write(dir.join("l.txt"), join(&old)).unwrap();
        ok(&dir, &["add", "-A"]);
        ok(&dir, &["commit", "-q", "--no-verify", "-m", "one"]);
        let mut new = old.clone();
        new[1] = b"Gr\xf6\xdfe 2 ge\xe4ndert".to_vec();
        new[24] = b"line 25 changed".to_vec();
        std::fs::write(dir.join("l.txt"), join(&new)).unwrap();
        let f = FileStatus { path: "l.txt".into(), orig: None, index: ' ', worktree: 'M' };
        let o = git::working_diff(&dir, &f, false, &DiffOpts::default());
        assert!(o.lossy);
        let mut d = parse(&o.stdout, 4);
        d.lossy = o.lossy;
        assert_eq!(d.hunks.len(), 2);
        assert!(!d.hunk_safe(0) && d.hunk_safe(1));
        apply(&dir, &["--cached"], &d.hunk_patch(1, false));
        let index = git::run_owned(Some(&dir), &["cat-file".into(), "blob".into(), ":l.txt".into()], None);
        let mut want = old.clone();
        want[24] = b"line 25 changed".to_vec();
        assert_eq!(index.stdout.as_bytes(), &String::from_utf8_lossy(&join(&want)).as_bytes()[..]);
        assert!(index.lossy, "the umlaut bytes of line 2 stay as they were");
        // A UTF-8 diff that happens to contain U+FFFD is fine.
        let mut u = parse("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+\u{FFFD}\n", 4);
        assert!(u.hunk_safe(0));
        u.lossy = true;
        assert!(!u.hunk_safe(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A text conversion (diff.<driver>.textconv) doesn't reach the Changes view's diff, whose
    /// hunks are written back: they must be the file's real text.
    #[test]
    fn working_diffs_have_no_text_conversion() {
        let dir = test_repo("textconv");
        write(&dir, ".gitattributes", "*.txt diff=upper\n");
        ok(&dir, &["config", "diff.upper.textconv", "tr a-z A-Z <"]);
        write(&dir, "t.txt", "one\ntwo\nthree\n");
        ok(&dir, &["add", "-A"]);
        ok(&dir, &["commit", "-q", "--no-verify", "-m", "one"]);
        write(&dir, "t.txt", "one\ntwo more\nthree\n");
        // The driver works (the commit diff of the history uses it) …
        assert!(ok(&dir, &["diff"]).contains("+TWO MORE"));
        // … but not here.
        let f = FileStatus { path: "t.txt".into(), orig: None, index: ' ', worktree: 'M' };
        let d = parse(&git::working_diff(&dir, &f, false, &DiffOpts::default()).stdout, 4);
        assert!(d.lines.iter().any(|l| l.kind == Kind::Add && l.text == "two more"));
        apply(&dir, &["--cached"], &d.hunk_patch(0, false));
        assert_eq!(ok(&dir, &["show", ":t.txt"]), "one\ntwo more\nthree\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Where the line-end rule allows Discard Hunk, git apply --reverse gives back exactly the
    /// file without that hunk, byte for byte, under several line-end settings.
    #[test]
    fn discard_keeps_line_ends_where_allowed() {
        // (core.autocrlf, .gitattributes, line end in the index, line end in the work tree)
        let cases: &[(&str, &str, &str, &str)] = &[
            ("false", "", "\n", "\n"),
            ("false", "", "\r\n", "\r\n"),
            ("true", "", "\n", "\r\n"),
            ("true", "", "\r\n", "\r\n"),
            ("true", "", "\n", "\n"),
            ("input", "", "\n", "\r\n"),
            ("input", "", "\n", "\n"),
            ("false", "*.txt text eol=crlf\n", "\n", "\r\n"),
            ("true", "*.txt -text\n", "\n", "\n"),
            ("input", "", "\r\n", "\r\n"),
            ("true", "* text=auto eol=lf\n", "\r\n", "\r\n"),
        ];
        let mut allowed = 0;
        for (n, (autocrlf, attrs, i_eol, w_eol)) in cases.iter().enumerate() {
            let dir = test_repo(&format!("eol-{n}"));
            if !attrs.is_empty() {
                write(&dir, ".gitattributes", attrs);
            }
            let lines: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
            let join = |v: &[String], eol: &str| v.iter().map(|l| format!("{l}{eol}")).collect::<String>();
            write(&dir, "f.txt", &join(&lines, i_eol));
            ok(&dir, &["add", "-A"]);
            ok(&dir, &["commit", "-q", "--no-verify", "-m", "one"]);
            ok(&dir, &["config", "core.autocrlf", autocrlf]);
            let before = join(&lines, w_eol);
            let mut changed = lines.clone();
            changed[19] = "line 20 changed".into();
            write(&dir, "f.txt", &join(&changed, w_eol));
            let eol = ok(&dir, &["ls-files", "--eol", "--", "f.txt"]);
            if !git::discard_changes_file(&dir, "f.txt") {
                allowed += 1;
                let f = FileStatus { path: "f.txt".into(), orig: None, index: ' ', worktree: 'M' };
                let d = parse(&git::working_diff(&dir, &f, false, &DiffOpts::default()).stdout, 4);
                apply(&dir, &["--reverse"], &d.hunk_patch(0, true));
                let after = String::from_utf8(std::fs::read(dir.join("f.txt")).unwrap()).unwrap();
                assert_eq!(after, before, "case {n} ({autocrlf}, {attrs:?}, {}) changed line ends", eol.trim());
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
        // The common cases are allowed (plain LF, Git for Windows' CRLF work tree, -text …).
        assert!(allowed >= 8, "only {allowed} cases allowed");
    }

    /// Discard Hunk is left alone for files git converts in other ways, and a `core.autocrlf`
    /// written without a value counts as true, as git reads it.
    #[test]
    fn discard_check_sees_encodings_and_bare_settings() {
        let dir = test_repo("discard-attrs");
        write(&dir, ".gitattributes", "*.u16 working-tree-encoding=UTF-16\n*.id ident\n");
        assert!(git::discard_changes_file(&dir, "a.u16"));
        assert!(git::discard_changes_file(&dir, "b.id"));
        assert!(!git::discard_changes_file(&dir, "c.txt"));
        // A file name with pattern characters is taken literally.
        assert!(git::discard_changes_file(&dir, "[a].u16"));
        assert_eq!(git::eol_config(&dir).0, "false");
        let cfg = std::fs::read_to_string(dir.join(".git").join("config")).unwrap();
        std::fs::write(dir.join(".git").join("config"), cfg.replace("autocrlf = false", "autocrlf")).unwrap();
        assert_eq!(git::eol_config(&dir).0, "true");
        // An empty value is false.
        std::fs::write(dir.join(".git").join("config"), cfg.replace("autocrlf = false", "autocrlf =")).unwrap();
        assert_eq!(git::eol_config(&dir).0, "false");
        std::fs::write(dir.join(".git").join("config"), &cfg).unwrap();
        // A .gitattributes deleted from the work tree: git apply wouldn't see what check-attr
        // still reads from the index.
        ok(&dir, &["add", ".gitattributes"]);
        ok(&dir, &["commit", "-q", "--no-verify", "-m", "attrs"]);
        std::fs::remove_file(dir.join(".gitattributes")).unwrap();
        assert!(git::discard_changes_file(&dir, "c.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A linked worktree's watch sees commits and branch moves, which live in the repository's
    /// common folder.
    #[test]
    fn a_worktree_watch_sees_the_common_folder() {
        let dir = test_repo("wt-main");
        write(&dir, "a.txt", "a\n");
        ok(&dir, &["add", "-A"]);
        ok(&dir, &["commit", "-q", "--no-verify", "-m", "one"]);
        let wt = std::env::temp_dir().join(format!("yagui-wt-linked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&wt);
        ok(&dir, &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()]);
        let g = git::git_dir(&wt).unwrap();
        let common = git::common_dir(&wt).unwrap();
        assert_ne!(g, common);
        let before = git::snapshot(&g, &common);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        // A new branch made in the main worktree (a loose ref in the common folder).
        ok(&dir, &["branch", "other"]);
        let after = git::snapshot(&g, &common);
        assert_ne!(git::changed_flags(&before, &after) & git::RELOAD_REFS, 0);
        let _ = std::fs::remove_dir_all(&wt);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn submodule_commits_are_not_hunks() {
        let t = "diff --git a/sub b/sub\nindex 1111111..2222222 160000\n--- a/sub\n+++ b/sub\n@@ -1 +1 @@\n-Subproject commit 1111111111111111111111111111111111111111\n+Subproject commit 2222222222222222222222222222222222222222\n";
        assert!(parse(t, 4).gitlink && !parse(t, 4).hunks_applicable());
        let dirty = "diff --git a/sub b/sub\n--- a/sub\n+++ b/sub\n@@ -1 +1 @@\n-Subproject commit 1111111111111111111111111111111111111111\n+Subproject commit 1111111111111111111111111111111111111111-dirty\n";
        assert!(!parse(dirty, 4).hunks_applicable());
        assert!(parse(MODIFY, 4).hunks_applicable());
    }
}
