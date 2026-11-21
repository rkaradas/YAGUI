//! A folder tree over a flat list of repository paths (`src/app/ui.rs`), used by the local
//! changes, the commit's changed files and the commit's file tree. Folders come first, chains
//! of folders with a single sub-folder are shown as one row ("src/app"), and whether a folder
//! is open is kept per list.

use std::collections::BTreeMap;

use eframe::egui::{self, Align2, Color32, FontId, Id, Sense, Vec2};

use crate::icons::{self, Icon};
use crate::theme;

pub const INDENT: f32 = 14.0;

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<(String, usize)>,
    count: usize,
}

/// One visible row of the tree.
pub enum Row {
    /// A folder: its full path without a trailing slash, the label shown (may be "a/b"), depth,
    /// how many files are below it, and whether it is open.
    Dir { path: String, label: String, depth: usize, count: usize, open: bool },
    /// A file: its index in the list given to [`rows`], its own name and depth.
    File { idx: usize, name: String, depth: usize },
}

fn insert(root: &mut Node, path: &str, idx: usize) {
    let mut node = root;
    node.count += 1;
    let mut parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    let file = parts.pop().unwrap_or(path).to_string();
    for p in parts {
        node = node.dirs.entry(p.to_string()).or_default();
        node.count += 1;
    }
    node.files.push((file, idx));
}

fn key(salt: &str, path: &str) -> Id {
    Id::unique(("tree-open", salt, path))
}

pub fn is_open(ctx: &egui::Context, salt: &str, path: &str, default_open: bool) -> bool {
    ctx.data(|d| d.get_temp::<bool>(key(salt, path))).unwrap_or(default_open)
}

pub fn toggle(ctx: &egui::Context, salt: &str, path: &str, default_open: bool) {
    let open = is_open(ctx, salt, path, default_open);
    ctx.data_mut(|d| d.insert_temp(key(salt, path), !open));
}

/// Opens or closes every folder of a list.
pub fn set_all(ctx: &egui::Context, salt: &str, paths: &[&str], open: bool) {
    for p in paths {
        let mut acc = String::new();
        let parts: Vec<&str> = p.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            ctx.data_mut(|d| d.insert_temp(key(salt, &acc), open));
        }
    }
}

/// The visible rows for `paths`, honouring which folders are open.
pub fn rows(ctx: &egui::Context, salt: &str, paths: &[&str], default_open: bool) -> Vec<Row> {
    let mut root = Node::default();
    for (i, p) in paths.iter().enumerate() {
        insert(&mut root, p, i);
    }
    let mut out = Vec::new();
    walk(ctx, salt, &root, "", 0, default_open, &mut out);
    out
}

fn walk(ctx: &egui::Context, salt: &str, node: &Node, prefix: &str, depth: usize, default_open: bool, out: &mut Vec<Row>) {
    for (name, child) in &node.dirs {
        // Fold single-folder chains into one row.
        let mut label = name.clone();
        let mut path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let mut c = child;
        while c.files.is_empty() && c.dirs.len() == 1 {
            let (n, cc) = c.dirs.iter().next().unwrap();
            label = format!("{label}/{n}");
            path = format!("{path}/{n}");
            c = cc;
        }
        let open = is_open(ctx, salt, &path, default_open);
        out.push(Row::Dir { path: path.clone(), label, depth, count: c.count, open });
        if open {
            walk(ctx, salt, c, &path, depth + 1, default_open, out);
        }
    }
    let mut files: Vec<&(String, usize)> = node.files.iter().collect();
    files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    for (name, idx) in files {
        out.push(Row::File { idx: *idx, name: name.clone(), depth });
    }
}

/// Draws a folder row. `action` is the hover button's icon (stage `+`, unstage `×`) if any.
/// Returns the row's response and whether the hover button was clicked.
pub fn dir_row(ui: &mut egui::Ui, label: &str, depth: usize, count: usize, open: bool, action: Option<Icon>) -> (egui::Response, bool) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    let p = ui.painter_at(rect);
    if resp.hovered() {
        p.rect_filled(rect, 0, theme::ROW_HOVER());
    }
    let x = rect.left() + 10.0 + depth as f32 * INDENT;
    icons::draw(&p, egui::Rect::from_center_size(egui::pos2(x + 4.0, rect.center().y), Vec2::splat(9.0)), if open { Icon::ChevronDown } else { Icon::ChevronRight }, theme::TEXT_DIM());
    icons::draw(&p, egui::Rect::from_center_size(egui::pos2(x + 20.0, rect.center().y), Vec2::splat(14.0)), Icon::Folder, theme::YELLOW().gamma_multiply(0.9));
    let action_w = if action.is_some() && resp.hovered() { 30.0 } else { 0.0 };
    let clip = egui::Rect::from_min_max(egui::pos2(x + 32.0, rect.top()), egui::pos2(rect.right() - 8.0 - action_w, rect.bottom()));
    let pc = ui.painter_at(clip);
    let g = pc.layout_no_wrap(label.to_string(), FontId::proportional(13.0), theme::TEXT());
    let gw = g.size().x;
    pc.galley(egui::pos2(x + 32.0, rect.center().y - g.size().y / 2.0), g, theme::TEXT());
    pc.text(egui::pos2(x + 32.0 + gw + 7.0, rect.center().y), Align2::LEFT_CENTER, count.to_string(), FontId::proportional(11.0), theme::TEXT_FAINT());
    let mut clicked = false;
    if let (Some(ic), true) = (action, action_w > 0.0) {
        let ar = egui::Rect::from_center_size(egui::pos2(rect.right() - 20.0, rect.center().y), Vec2::new(22.0, 18.0));
        let hovered = resp.hover_pos().map(|p| ar.contains(p)).unwrap_or(false);
        p.rect_filled(ar, 4, if hovered { theme::dk(0x4a, 0x4a, 0x4f) } else { theme::dk(0x3a, 0x3a, 0x3d) });
        icons::draw(&p, ar.shrink(4.0), ic, theme::TEXT_STRONG());
        clicked = resp.clicked() && hovered;
    }
    (resp, clicked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_first_and_chains_folded() {
        let ctx = egui::Context::default();
        let paths = ["README.md", "src/app/ui.rs", "src/app/tree.rs", "src/main.rs", "docs/x/y/z.md"];
        let rows = rows(&ctx, "t", &paths, true);
        let shown: Vec<String> = rows
            .iter()
            .map(|r| match r {
                Row::Dir { label, depth, count, .. } => format!("{depth}D {label} {count}"),
                Row::File { name, depth, .. } => format!("{depth}F {name}"),
            })
            .collect();
        assert_eq!(shown, vec!["0D docs/x/y 1", "1F z.md", "0D src 3", "1D app 2", "2F tree.rs", "2F ui.rs", "1F main.rs", "0F README.md"]);
        toggle(&ctx, "t", "src", true);
        let n = super::rows(&ctx, "t", &paths, true).len();
        assert_eq!(n, 4);
    }
}
