//! Drawing a parsed diff (`diffmodel::Diff`) and the diff toolbar.
//!
//! Only the rows in view are laid out and painted, so a long diff costs the same per frame as a
//! short one. Without word wrap, long lines scroll sideways (wheel with Shift, touchpad or the
//! bar at the bottom) while the line numbers stay in place, and only the visible part of a line
//! is laid out, so even megabyte-long lines (minified files) stay cheap. Side by side shows the
//! old lines on the left and the new ones on the right; a missing counterpart is hatched.

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, Rect, Sense, Stroke, Vec2, pos2, vec2};

use super::diffmodel::{cells, grid_exact, Diff, Kind, Line, Row};
use crate::git::DiffOpts;
use crate::i18n::{tr, trs};
use crate::icons::{self, Icon};
use crate::theme;

/// How a diff is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Look {
    pub split: bool,
    pub wrap: bool,
    pub show_ws: bool,
}

/// Below this width side by side falls back to the unified view.
pub const SPLIT_MIN_WIDTH: f32 = 520.0;

/// The diff toolbar's buttons, from left to right.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    IgnoreWs,
    ShowWs,
    Wrap,
    Less,
    More,
    Full,
    Split,
}

/// What the toolbar shows: how diffs are drawn and fetched.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Bar {
    pub look: Look,
    pub opts: DiffOpts,
}

const CONTEXT_STEPS: [u32; 10] = [0, 1, 2, 3, 5, 8, 12, 20, 40, 100];

/// One step fewer lines of context.
pub fn less_context(c: u32) -> u32 {
    CONTEXT_STEPS.iter().rev().copied().find(|s| *s < c).unwrap_or(0)
}

/// One step more lines of context.
pub fn more_context(c: u32) -> u32 {
    CONTEXT_STEPS.iter().copied().find(|s| *s > c).unwrap_or(c)
}

/// A small icon button that can be on (accent) or unavailable (faint, with the reason).
fn toggle(ui: &mut egui::Ui, icon: Icon, on: bool, enabled: bool, tip: &str, cmd: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if on && enabled {
        p.rect_filled(rect, 4, theme::ROW_SEL());
    } else if enabled && resp.hovered() {
        p.rect_filled(rect, 4, theme::ROW_HOVER());
    }
    let col = if !enabled {
        theme::TEXT_FAINT()
    } else if on {
        theme::ACCENT()
    } else if resp.hovered() {
        theme::TEXT_STRONG()
    } else {
        theme::TEXT_DIM()
    };
    icons::draw(p, rect.shrink(4.0), icon, col);
    let resp = super::ui::tip_with_cmd(resp, tip, cmd);
    enabled && resp.clicked()
}

/// The diff toolbar (for the right side of a diff's header, in a right-to-left layout).
/// Returns the button that was clicked. `split_possible`: the diff below can be shown side by
/// side (it is wide enough and no merge conflict); if not, it is shown in one column, so word
/// wrap is available there even with side by side on.
pub fn toolbar(ui: &mut egui::Ui, b: &Bar, split_possible: bool) -> Option<Opt> {
    let mut hit = None;
    let (look, o) = (b.look, b.opts);
    let split_shown = look.split && split_possible;
    let spacing = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = 2.0;
    // Right to left: the last button first.
    let split_tip = if look.split && !split_possible {
        tr("Side by side: old lines left, new lines right (not here: too narrow, or a merge conflict)", "Nebeneinander: alte Zeilen links, neue rechts (hier nicht: zu schmal oder ein Merge-Konflikt)")
    } else {
        tr("Side by side: old lines left, new lines right", "Nebeneinander: alte Zeilen links, neue rechts")
    };
    if toggle(ui, Icon::SplitView, look.split, true, split_tip, "") {
        hit = Some(Opt::Split);
    }
    if toggle(ui, Icon::FullFile, o.full, true, tr("Show the entire file", "Ganze Datei zeigen"), "-U1000000") {
        hit = Some(Opt::Full);
    }
    let more = more_context(o.context);
    let more_tip = trs(format!("More lines around each change ({} → {more})", o.context), format!("Mehr Zeilen um jede Änderung ({} → {more})", o.context));
    if toggle(ui, Icon::ContextMore, false, !o.full && more != o.context, &more_tip, &format!("-U{more}")) {
        hit = Some(Opt::More);
    }
    let less = less_context(o.context);
    let less_tip = trs(format!("Fewer lines around each change ({} → {less})", o.context), format!("Weniger Zeilen um jede Änderung ({} → {less})", o.context));
    if toggle(ui, Icon::ContextLess, false, !o.full && less != o.context, &less_tip, &format!("-U{less}")) {
        hit = Some(Opt::Less);
    }
    let wrap_tip = if split_shown { tr("Word wrap (not side by side)", "Zeilenumbruch (nicht nebeneinander)") } else { tr("Word wrap", "Zeilenumbruch") };
    if toggle(ui, Icon::Wrap, look.wrap && !split_shown, !split_shown, wrap_tip, "") {
        hit = Some(Opt::Wrap);
    }
    if toggle(ui, Icon::Invisibles, look.show_ws, true, tr("Show invisible characters: · space, → tab, ¶ Windows line end (CR)", "Unsichtbare Zeichen zeigen: · Leerzeichen, → Tab, ¶ Windows-Zeilenende (CR)"), "") {
        hit = Some(Opt::ShowWs);
    }
    if toggle(ui, Icon::Whitespace, o.ignore_ws, true, tr("Ignore changes in whitespace", "Änderungen an Leerzeichen ignorieren"), "-w") {
        hit = Some(Opt::IgnoreWs);
    }
    ui.spacing_mut().item_spacing.x = spacing;
    hit
}

/// The facts from a diff's header as small pills (new file, renamed, mode, binary …).
pub fn facts(ui: &mut egui::Ui, d: &Diff) {
    if d.facts.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for f in &d.facts {
            let txt = if crate::i18n::is_de() { f.clone() } else { super::ui::fact_en(f) };
            super::ui::pill(ui, &txt, theme::dk(0x2c, 0x37, 0x46), if theme::is_light() { Color32::from_rgb(0x2a, 0x4a, 0x70) } else { Color32::from_rgb(0xb4, 0xcf, 0xee) });
        }
    });
    ui.add_space(4.0);
}

/// Shown instead of the lines when a diff has none.
pub fn empty_hint(ui: &mut egui::Ui, d: &Diff, opts: &DiffOpts) {
    if d.binary {
        return; // the facts say it is a binary file
    }
    ui.add_space(8.0);
    let text = if opts.ignore_ws { tr("No changes apart from whitespace (Ignore whitespace is on).", "Keine Änderungen außer an Leerzeichen („Leerzeichen ignorieren“ ist an).") } else { tr("No textual changes.", "Keine Textänderungen.") };
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(egui::RichText::new(text).color(theme::TEXT_DIM()));
    });
}

/// Sizes that stay the same for every row of one frame.
struct Geo {
    font: FontId,
    num_font: FontId,
    char_w: f32,
    row_h: f32,
    num_w: f32,
    wrap: bool,
    show_ws: bool,
    tab: usize,
    /// Left edge of the text in a row (unified) or in a half (side by side), from its left.
    text_x: f32,
    /// Width of the text area.
    text_w: f32,
    /// Characters per visual line when wrapping.
    per_line: usize,
    hoff: f32,
}

/// Visual rows of a wrapped line: up to the row its last character starts in (a wide character
/// or the ¶ after it may reach into the two spare columns of that row).
fn visual_lines(l: &Line, per_line: usize) -> usize {
    if l.cols == 0 { 1 } else { l.last_col / per_line + 1 }
}

/// What the row heights of a drawn diff depend on.
#[derive(Clone, Copy, PartialEq)]
struct TopsKey {
    view: Id,
    diff: u64,
    split: bool,
    wrap: bool,
    per_line: usize,
    show_ws: bool,
    row_h: u32,
}

thread_local! {
    /// The row tops (and the end) of the diffs drawn lately, newest last, so a frame doesn't
    /// walk all rows of a long diff again. One entry per view.
    static TOPS: std::cell::RefCell<Vec<(TopsKey, std::rc::Rc<Vec<f32>>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn row_tops(key: TopsKey, make: impl FnOnce() -> Vec<f32>) -> std::rc::Rc<Vec<f32>> {
    if let Some(v) = TOPS.with(|c| c.borrow().iter().find(|e| e.0 == key).map(|e| e.1.clone())) {
        return v;
    }
    let v = std::rc::Rc::new(make());
    TOPS.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|e| e.0.view != key.view);
        c.push((key, v.clone()));
        // Room for every diff of a big commit opened with "Expand All".
        if c.len() > 256 {
            c.remove(0);
        }
    });
    v
}

/// Draws the rows of a diff into the current place of `ui` (inside a vertical scroll area or
/// another scrolling page). `bar` draws the right side of a hunk's header row (e.g. buttons).
pub fn draw(ui: &mut egui::Ui, id: Id, d: &Diff, look: Look, mut bar: Option<&mut dyn FnMut(&mut egui::Ui, usize)>) {
    let rows_all_empty = d.unified.is_empty();
    if rows_all_empty {
        return;
    }
    let font = FontId::monospace(super::extra::diff_font_size());
    let num_font = FontId::monospace(11.0);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'M')).max(1.0);
    let num_cw = ui.fonts_mut(|f| f.glyph_width(&num_font, '0')).max(1.0);
    let row_h = (font.size + 6.0).max(16.0).round();
    let hunk_h = 26.0;
    let digits = d.max_no.max(1).to_string().len().max(3) as f32;
    let num_w = (digits * num_cw + 14.0).round();
    let width = ui.available_width().max(200.0);
    let split = look.split && !d.combined && width >= SPLIT_MIN_WIDTH;
    let wrap = look.wrap && !split;
    let (text_x, text_w) = if split {
        let half = (width / 2.0).floor();
        (num_w + 8.0, (half - num_w - 12.0).max(20.0))
    } else {
        (2.0 * num_w + 10.0, (width - 2.0 * num_w - 14.0).max(20.0))
    };
    // Columns per visual line when wrapping, with two to spare: a wide character may start in
    // the last column and reach past it (see `diffmodel::cell_width`).
    let per_line = ((text_w / char_w).floor() as usize).saturating_sub(2).max(8);
    let rows: &[Row] = if split { &d.split } else { &d.unified };

    // Row tops (and the end), so the visible rows are found by binary search; kept from the
    // frames before while nothing they depend on changes.
    let key = TopsKey { view: id, diff: d.id, split, wrap, per_line, show_ws: look.show_ws, row_h: row_h.to_bits() };
    let tops = row_tops(key, || {
        let mut ys = Vec::with_capacity(rows.len() + 1);
        let mut y = 0.0;
        for r in rows {
            ys.push(y);
            y += match r {
                Row::Hunk(_) => hunk_h,
                Row::Line(i) if wrap => row_h * visual_lines(&d.lines[*i], per_line) as f32,
                _ => row_h,
            };
        }
        ys.push(y);
        ys
    });
    let ys: &[f32] = &tops;
    let y = ys.last().copied().unwrap_or(0.0);
    // Sideways scrolling, shared by both halves; below the last row there is room for its bar.
    let content_w = (d.max_cols + 2) as f32 * char_w + 8.0;
    let max_h = if wrap { 0.0 } else { (content_w - text_w).max(0.0) };
    let bar_room = if max_h > 0.0 { 12.0 } else { 0.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(width, y + bar_room), Sense::hover());
    let clip = ui.clip_rect().intersect(rect);
    if !clip.is_positive() {
        return;
    }
    let h_id = id.with("hscroll");
    let mut hoff: f32 = ui.ctx().data(|m| m.get_temp(h_id)).unwrap_or(0.0);
    if max_h > 0.0 && ui.rect_contains_pointer(clip) {
        let dx = ui.input(|i| i.smooth_scroll_delta.x);
        let moved = (hoff - dx).clamp(0.0, max_h);
        if dx != 0.0 && moved != hoff {
            hoff = moved;
            // Used here, so no scroll area around the diff scrolls as well.
            ui.input_mut(|i| i.smooth_scroll_delta.x = 0.0);
        }
    }
    hoff = hoff.clamp(0.0, max_h);

    // The tab width the diff's column counts were made with, so widths and text agree.
    let g = Geo { font, num_font, char_w, row_h, num_w, wrap, show_ws: look.show_ws, tab: d.tab.max(1), text_x, text_w, per_line, hoff };
    let painter = ui.painter_at(clip);
    let top = clip.top() - rect.top();
    let bottom = clip.bottom() - rect.top();
    let first = ys[1..].partition_point(|end| *end <= top);
    for k in first..rows.len() {
        if ys[k] >= bottom {
            break;
        }
        let r = Rect::from_min_size(pos2(rect.left(), rect.top() + ys[k]), vec2(width, ys[k + 1] - ys[k]));
        match rows[k] {
            Row::Hunk(h) => hunk_row(ui, &painter, r, d, h, id, bar.as_deref_mut()),
            Row::Line(i) => unified_row(&painter, r, &d.lines[i], &g),
            Row::Pair(l, rr) => pair_row(&painter, r, l.map(|i| &d.lines[i]), rr.map(|i| &d.lines[i]), &g),
        }
    }

    // The sideways scroll bar at the bottom of what is visible: thin, wider under the pointer.
    if max_h > 0.0 {
        let left = if split { rect.left() } else { rect.left() + text_x };
        let zone = Rect::from_min_max(pos2(left, clip.bottom() - 12.0), pos2(rect.right() - 2.0, clip.bottom()));
        let resp = ui.interact(zone, h_id.with("bar"), Sense::click_and_drag());
        let thumb_w = (zone.width() * text_w / content_w).clamp(24.0, zone.width());
        let free = (zone.width() - thumb_w).max(1.0);
        if resp.dragged() {
            hoff = (hoff + resp.drag_delta().x * max_h / free).clamp(0.0, max_h);
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                hoff = (((p.x - zone.left() - thumb_w / 2.0) / free) * max_h).clamp(0.0, max_h);
            }
        }
        let hot = resp.hovered() || resp.dragged();
        let th = if hot { 8.0 } else { 4.0 };
        let track = Rect::from_min_max(pos2(zone.left(), zone.bottom() - 2.0 - th), pos2(zone.right(), zone.bottom() - 2.0));
        let thumb = Rect::from_min_size(pos2(track.left() + free * (hoff / max_h), track.top()), vec2(thumb_w, th));
        let painter = ui.painter_at(clip);
        if hot {
            painter.rect_filled(track, 4, theme::LINE().gamma_multiply(0.7));
        }
        painter.rect_filled(thumb, 4, if hot { theme::TEXT_DIM() } else { theme::TEXT_FAINT() });
    }
    ui.ctx().data_mut(|m| m.insert_temp(h_id, hoff));
}

fn hunk_row(ui: &mut egui::Ui, painter: &egui::Painter, r: Rect, d: &Diff, h: usize, id: Id, bar: Option<&mut (dyn FnMut(&mut egui::Ui, usize) + '_)>) {
    painter.rect_filled(r, 0, theme::DIFF_HUNK());
    let hk = &d.hunks[h];
    let mut label_right = r.right() - 8.0;
    if let Some(bar) = bar {
        // A fixed id per hunk (`scope_id`, not `id_salt`, which mixes in how many hunks are
        // visible above): the buttons keep their ids while scrolling, so clicks register.
        let mut child = ui.new_child(egui::UiBuilder::new().scope_id(id.with(("hunk", h))).max_rect(r.shrink2(vec2(8.0, 1.0))).layout(Layout::right_to_left(Align::Center)));
        bar(&mut child, h);
        label_right = child.min_rect().left() - 8.0;
    }
    let mut label = format!("@@ -{},{} +{},{} @@", hk.old_start, hk.old_len, hk.new_start, hk.new_len);
    if !hk.context.is_empty() {
        label.push_str("  ");
        label.push_str(&hk.context);
    }
    let clip = Rect::from_min_max(r.min, pos2(label_right.max(r.left()), r.bottom())).intersect(painter.clip_rect());
    painter.with_clip_rect(clip).text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, label, FontId::monospace(11.5), theme::DIFF_HUNK_FG());
}

fn colors(kind: Kind) -> (Color32, Color32) {
    match kind {
        Kind::Add => (theme::DIFF_ADD(), theme::DIFF_ADD_FG()),
        Kind::Del => (theme::DIFF_DEL(), theme::DIFF_DEL_FG()),
        Kind::NoNewline => (Color32::TRANSPARENT, theme::TEXT_FAINT()),
        Kind::Context => (Color32::TRANSPARENT, theme::TEXT()),
    }
}

fn number(painter: &egui::Painter, right_x: f32, y_mid: f32, n: u32, g: &Geo) {
    if n > 0 {
        painter.text(pos2(right_x, y_mid), Align2::RIGHT_CENTER, n.to_string(), g.num_font.clone(), theme::TEXT_FAINT());
    }
}

fn unified_row(painter: &egui::Painter, r: Rect, l: &Line, g: &Geo) {
    let (bg, fg) = colors(l.kind);
    if bg != Color32::TRANSPARENT {
        painter.rect_filled(r, 0, bg);
    }
    let mid = r.top() + g.row_h / 2.0;
    number(painter, r.left() + g.num_w - 6.0, mid, l.old, g);
    number(painter, r.left() + 2.0 * g.num_w - 6.0, mid, l.new, g);
    let sep = r.left() + 2.0 * g.num_w + 2.0;
    painter.line_segment([pos2(sep, r.top()), pos2(sep, r.bottom())], Stroke::new(1.0, theme::LINE()));
    let area = Rect::from_min_max(pos2(r.left() + g.text_x, r.top()), pos2(r.right() - 4.0, r.bottom()));
    text(painter, area, l, fg, g);
}

fn pair_row(painter: &egui::Painter, r: Rect, left: Option<&Line>, right: Option<&Line>, g: &Geo) {
    let half = (r.width() / 2.0).floor();
    let lr = Rect::from_min_max(r.min, pos2(r.left() + half, r.bottom()));
    let rr = Rect::from_min_max(pos2(r.left() + half, r.top()), r.max);
    side(painter, lr, left, true, g);
    side(painter, rr, right, false, g);
    painter.line_segment([pos2(lr.right(), r.top()), pos2(lr.right(), r.bottom())], Stroke::new(1.0, theme::LINE()));
}

fn side(painter: &egui::Painter, r: Rect, line: Option<&Line>, old: bool, g: &Geo) {
    let Some(l) = line else {
        hatch(painter, r);
        return;
    };
    let (bg, fg) = colors(l.kind);
    if bg != Color32::TRANSPARENT {
        painter.rect_filled(r, 0, bg);
    }
    number(painter, r.left() + g.num_w - 6.0, r.center().y, if old { l.old } else { l.new }, g);
    let sep = r.left() + g.num_w + 2.0;
    painter.line_segment([pos2(sep, r.top()), pos2(sep, r.bottom())], Stroke::new(1.0, theme::LINE()));
    let area = Rect::from_min_max(pos2(r.left() + g.text_x, r.top()), pos2(r.right() - 4.0, r.bottom()));
    text(painter, area, l, fg, g);
}

/// Diagonal hatching for the empty side of a row; lines continue from row to row.
fn hatch(painter: &egui::Painter, r: Rect) {
    painter.rect_filled(r, 0, theme::DIFF_FILLER());
    let p = painter.with_clip_rect(r.intersect(painter.clip_rect()));
    let step = 7.0;
    // Lines through points with x + y = k * step (so neighbouring rows line up).
    let k0 = ((r.left() + r.top()) / step).floor() as i64;
    let k1 = ((r.right() + r.bottom()) / step).ceil() as i64;
    let s = Stroke::new(1.0, theme::DIFF_HATCH());
    for k in k0..=k1 {
        let c = k as f32 * step;
        // From the bottom edge (x = c - bottom) up to the top edge (x = c - top).
        p.line_segment([pos2(c - r.bottom(), r.bottom()), pos2(c - r.top(), r.top())], s);
    }
}

/// A piece of a line to paint: the column it starts in, how many columns it has, and its
/// characters, each with "is a whitespace symbol".
#[derive(Debug, PartialEq)]
struct Piece {
    col: usize,
    cols: usize,
    chars: Vec<(char, bool)>,
}

/// The pieces of a line whose user-perceived characters (grapheme clusters: a letter with its
/// accents, an emoji sequence) start in the columns [from, from + n), tabs expanded. A piece is
/// a run of one kind of text: what the diff font draws exactly one column wide
/// (`grid_exact`), or anything else (East Asian, emoji, symbols, other scripts). Spaces join
/// the run they follow, so a phrase in Hebrew or Arabic stays one text (shaped, right to
/// left). Each piece is painted at its column and fitted into its columns (`paint_chars`). With
/// `ws` spaces become '·' and a tab starts with '→'. A simple line (ASCII without tabs) is
/// sliced, other long lines are walked from their last mark before `from`, so a part far to
/// the right costs about its own length.
fn segment(l: &Line, tab: usize, from: usize, n: usize, ws: bool) -> Vec<Piece> {
    let end = from.saturating_add(n);
    let mark = |c: char| if ws && c == ' ' { ('·', true) } else { (c, false) };
    if l.simple {
        let t = l.text.as_str();
        let s = &t[from.min(t.len())..end.min(t.len())];
        return if s.is_empty() { vec![] } else { vec![Piece { col: from, cols: s.len(), chars: s.chars().map(mark).collect() }] };
    }
    let mut out: Vec<Piece> = Vec::new();
    // The kind of the last piece (grid-exact or not) while the next cluster may extend it.
    let mut open: Option<bool> = None;
    let (col0, byte) = l.start_for(from);
    for (_, g, col, w) in cells(&l.text[byte..], col0, tab) {
        if w == 0 {
            continue; // nothing to see (a byte order mark, a lone joiner …)
        }
        if col >= end {
            break;
        }
        if col < from {
            open = None;
            continue;
        }
        let first = g.chars().next().unwrap_or(' ');
        let space = g == "\t" || first.is_whitespace();
        let kind = grid_exact(first);
        let joins = match open {
            Some(k) => space || k == kind,
            None => false,
        };
        if !joins {
            out.push(Piece { col, cols: 0, chars: vec![] });
            open = Some(kind);
        }
        let p = out.last_mut().expect("pushed above");
        if g == "\t" {
            p.chars.push(if ws { ('→', true) } else { (' ', false) });
            p.chars.extend(std::iter::repeat_n((' ', false), w - 1));
        } else {
            p.chars.extend(g.chars().map(mark));
        }
        p.cols += w;
    }
    out
}

/// Paints characters at `pos` (left, vertical middle), whitespace symbols faint, into at most
/// `cols` columns: text drawn wider than its columns (a fallback font's letters, an emoji) is
/// made smaller to fit, so nothing runs over the text after it or out of view.
fn paint_chars(painter: &egui::Painter, area: Rect, pos: egui::Pos2, chars: &[(char, bool)], cols: usize, fg: Color32, g: &Geo) {
    if chars.is_empty() {
        return;
    }
    let clip = area.intersect(painter.clip_rect());
    let p = painter.with_clip_rect(clip);
    let faint = theme::TEXT_FAINT();
    let layout = |size: f32| {
        let font = FontId::new(size, g.font.family.clone());
        let mut job = LayoutJob::default();
        let mut run = String::new();
        let mut run_sym = false;
        let flush = |job: &mut LayoutJob, run: &mut String, sym: bool| {
            if !run.is_empty() {
                job.append(run, 0.0, TextFormat { font_id: font.clone(), color: if sym { faint } else { fg }, ..Default::default() });
                run.clear();
            }
        };
        for (c, sym) in chars {
            if *sym != run_sym {
                flush(&mut job, &mut run, run_sym);
                run_sym = *sym;
            }
            run.push(*c);
        }
        flush(&mut job, &mut run, run_sym);
        p.layout_job(job)
    };
    let mut galley = layout(g.font.size);
    let room = cols as f32 * g.char_w;
    if cols > 0 && galley.size().x > room + 0.5 {
        galley = layout((g.font.size * room / galley.size().x).max(g.font.size * 0.4));
    }
    let top = pos.y - galley.size().y / 2.0;
    p.galley(pos2(pos.x, top), galley, fg);
}

/// The text of a line in its area: sideways-scrolled, or wrapped into several visual lines.
/// Each piece is painted at its own column; the ¶ of a CR LF line end after the last one.
fn text(painter: &egui::Painter, area: Rect, l: &Line, fg: Color32, g: &Geo) {
    let cr_marker = g.show_ws && l.cr;
    let pilcrow = [('¶', true)];
    if g.wrap {
        // Only the visual lines in view: a minified line can have thousands of them.
        let n = visual_lines(l, g.per_line);
        let clip = painter.clip_rect();
        let v0 = (((clip.top() - area.top()) / g.row_h).floor().max(0.0) as usize).min(n);
        let v1 = (((clip.bottom() - area.top()) / g.row_h).ceil().max(0.0) as usize).min(n);
        for v in v0..v1 {
            let first = v * g.per_line;
            let y = area.top() + g.row_h * v as f32 + g.row_h / 2.0;
            let x = |col: usize| area.left() + (col - first) as f32 * g.char_w;
            for p in segment(l, g.tab, first, g.per_line, g.show_ws) {
                paint_chars(painter, area, pos2(x(p.col), y), &p.chars, p.cols, fg, g);
            }
            if cr_marker && v + 1 == n {
                // After the last character, but in the row's spare columns at the latest (a
                // tab at the row's end can reach further).
                let at = l.cols.clamp(first, first + g.per_line + 1);
                paint_chars(painter, area, pos2(x(at), y), &pilcrow, 1, fg, g);
            }
        }
        return;
    }
    // Only the columns in view (plus some on each side: a wide character starting just left of
    // the view still shows its right part) are laid out.
    let from = (g.hoff / g.char_w).floor() as usize;
    let n = (g.text_w / g.char_w).ceil() as usize + 2;
    let y = area.top() + g.row_h / 2.0;
    let x = |col: usize| area.left() + col as f32 * g.char_w - g.hoff;
    let start = from.saturating_sub(2);
    for p in segment(l, g.tab, start, n + (from - start), g.show_ws) {
        paint_chars(painter, area, pos2(x(p.col), y), &p.chars, p.cols, fg, g);
    }
    if cr_marker && l.cols >= from && l.cols < from + n {
        paint_chars(painter, area, pos2(x(l.cols), y), &pilcrow, 1, fg, g);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_steps() {
        assert_eq!(less_context(3), 2);
        assert_eq!(less_context(0), 0);
        assert_eq!(less_context(4), 3);
        assert_eq!(more_context(3), 5);
        assert_eq!(more_context(100), 100);
        assert_eq!(more_context(6), 8);
    }

    fn line(t: &str) -> Line {
        Line::new(super::super::diffmodel::Kind::Context, 1, 1, t, false, 4)
    }

    /// The pieces of a part of a line as "column:text", separated by spaces.
    fn s(t: &str, from: usize, n: usize, ws: bool) -> String {
        segment(&line(t), 4, from, n, ws).into_iter().map(|p| format!("{}:{}", p.col, p.chars.iter().map(|c| c.0).collect::<String>())).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn segments_expand_tabs_and_mark_whitespace() {
        assert_eq!(s("a\tb", 0, 10, false), "0:a   b");
        assert_eq!(s("a\tb c", 0, 10, true), "0:a→  b·c");
        assert_eq!(s("abcdef", 2, 3, false), "2:cde");
        // A tab starting left of the part belongs to the part before.
        assert_eq!(s("\tx", 2, 5, true), "4:x");
        assert!(segment(&line("a b"), 4, 0, 9, true)[0].chars[1].1);
        // Simple lines are sliced; past the end there is nothing, even far out.
        assert!(line("abcdef").simple && !line("a\tb").simple && !line("äb").simple);
        assert_eq!(s("ab cd", 1, 3, true), "1:b·c");
        assert_eq!(s("abc", 2, 10, false), "2:c");
        assert_eq!(s("abc", 7, 10, false), "");
        assert_eq!(s("äbc d", 1, 9, true), "1:bc·d");
    }

    #[test]
    fn wide_characters_are_pieces_at_their_own_column() {
        // 中 takes two columns: "a中b" is 4 wide; a part starting inside 中 leaves it to the
        // part before and starts with b at column 3.
        assert_eq!(line("a中b").cols, 4);
        assert_eq!(s("a中b", 0, 2, false), "0:a 1:中");
        assert_eq!(s("a中b", 2, 5, false), "3:b");
        // Runs of one kind; a space joins the run before it.
        assert_eq!(s("ℹ ok ⏳ x", 0, 20, false), "0:ℹ  2:ok  5:⏳  8:x");
        // Emoji made by a variation selector or a skin tone take two columns.
        assert_eq!(line("☝\u{1F3FD}x").cols, 3);
        assert_eq!(line("1\u{FE0F}\u{20E3}x").cols, 3);
        assert_eq!(line("🚀x").cols, 3);
        assert_eq!(line("Größe→").cols, 6);
        // Combining marks and joined emoji stay with their character and take no columns.
        assert_eq!(line("e\u{301}x").cols, 2);
        assert_eq!(s("e\u{301}x", 0, 5, false), "0:e\u{301}x");
        let family = "👨\u{200D}👩\u{200D}👧!";
        assert_eq!(line(family).cols, 3);
        assert_eq!(s(family, 0, 5, false), "0:👨\u{200D}👩\u{200D}👧 2:!");
        assert_eq!((line("a中").cols, line("a中").last_col), (3, 1));
    }

    #[test]
    fn scripts_that_need_shaping_stay_in_one_piece() {
        // Hebrew and Arabic words keep their letters together (shaping, right to left); a Thai
        // vowel mark takes no column and stays with its letter.
        assert_eq!(s("שלום עולם", 0, 20, false), "0:שלום עולם");
        assert_eq!(s("مرحبا بك", 0, 20, false), "0:مرحبا بك");
        assert_eq!(line("กิน").cols, 2);
        assert_eq!(s("กิน", 0, 5, false), "0:กิน");
        // A mark on the last character of a wrapped row stays with it.
        assert_eq!(s("abcde\u{301}f", 0, 5, false), "0:abcde\u{301}");
        assert_eq!(s("abcde\u{301}f", 5, 5, false), "5:f");
    }

    #[test]
    fn long_lines_are_walked_from_their_marks() {
        // A long line with one umlaut: parts far to the right come out the same as walking it
        // from the start, but begin at a mark.
        let text: String = std::iter::once('ä').chain((0..5000).map(|i| char::from(b'a' + (i % 26) as u8))).collect();
        let l = line(&text);
        assert!(!l.simple && l.marks.len() >= 9 && l.cols == 5001);
        let want: String = text.chars().skip(4000).take(10).collect();
        assert_eq!(s(&text, 4000, 10, false), format!("4000:{want}"));
        assert!(l.start_for(4000).0 <= 4000 && l.start_for(4000).0 > 3000);
    }
}
