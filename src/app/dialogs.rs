//! Dialogs: the compact command dialog (options on top, the git command shown subtly in the
//! footer), the interactive-rebase editor, Quick Launch, the activity window and the console.

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, RichText, Stroke, Vec2};

use super::ui::{icon, icon_button};
use super::{App, Form, RB_ACTIONS, rb_label};
use crate::catalog::{self, Arg, Cmd, Danger, Kind, P, Value, CATS};
use crate::git::{Job, JobState};
use crate::i18n::{c, tr, trs};
use crate::icons::Icon;
use crate::theme;

struct Suggest {
    branches: Vec<String>,
    remotes: Vec<String>,
    tags: Vec<String>,
    stashes: Vec<String>,
    commits: Vec<(String, String)>,
    files: Vec<String>,
}

impl Suggest {
    fn from(r: Option<&super::Repo>) -> Self {
        let Some(r) = r else {
            return Suggest { branches: vec![], remotes: vec![], tags: vec![], stashes: vec![], commits: vec![], files: vec![] };
        };
        let mut branches: Vec<String> = r.local.iter().map(|b| b.name.clone()).collect();
        branches.extend(r.remote_branches.iter().map(|b| b.name.clone()));
        Suggest {
            branches,
            remotes: r.remotes.iter().map(|x| x.0.clone()).collect(),
            tags: r.tags.iter().map(|t| t.name.clone()).collect(),
            stashes: r.stashes.iter().map(|s| s.0.clone()).collect(),
            commits: r.commits.iter().take(40).map(|c| (c.short.clone(), c.subject.clone())).collect(),
            files: r.status.iter().map(|f| f.path.clone()).collect(),
        }
    }

    fn for_key(&self, key: &str) -> Vec<(String, String)> {
        let pairs = |v: &Vec<String>| v.iter().map(|x| (x.clone(), x.clone())).collect::<Vec<_>>();
        match key {
            "branch" | "upstream" | "onto" | "a" | "b" | "start" | "head" | "bad" | "good" | "range" | "branches" => {
                let mut v = pairs(&self.branches);
                v.extend(pairs(&self.tags));
                v
            }
            "remote" => pairs(&self.remotes),
            "tag" => pairs(&self.tags),
            "stash" => pairs(&self.stashes),
            "commit" | "commits" | "object" | "ref" => {
                let mut v: Vec<(String, String)> = self.commits.iter().map(|(s, m)| (s.clone(), format!("{s}  {m}"))).collect();
                v.extend(pairs(&self.branches));
                v.extend(pairs(&self.tags));
                v
            }
            "paths" | "file" | "files" => pairs(&self.files),
            _ => vec![],
        }
    }
}

#[derive(PartialEq)]
enum FormAction {
    None,
    Run,
    Cancel,
}

/// The verb on a dialog's main button.
fn primary_label(cmd: &Cmd) -> &'static str {
    let id = cmd.id;
    match id {
        "fetch" => "Fetch",
        "pull" => "Pull",
        "push" | "push_tag" | "push_tags" => "Push",
        "merge" => "Merge",
        "rebase" => "Rebase",
        "commit" | "commit_amend" | "commit_fixup" => "Commit",
        "stash_push" => "Stash",
        "stash_pop" => tr("Pop", "Zurückholen"),
        "stash_apply" => tr("Apply", "Anwenden"),
        "clone" => tr("Clone", "Klonen"),
        "init" => tr("Create", "Anlegen"),
        "switch" | "checkout" | "checkout_track" => tr("Checkout", "Auschecken"),
        "switch_create" | "branch_create" | "stash_branch" => tr("Create Branch", "Branch anlegen"),
        "tag_create" => tr("Create Tag", "Tag erstellen"),
        "cherry_pick" => "Cherry-pick",
        "revert" => "Revert",
        "reset" => "Reset",
        "branch_rename" | "remote_rename" => tr("Rename", "Umbenennen"),
        "remote_add" | "submodule_add" | "worktree_add" => tr("Add", "Hinzufügen"),
        _ if id.contains("delete") || id.contains("remove") || id.contains("drop") || id.contains("clear") || id == "clean" => tr("Delete", "Löschen"),
        _ if id.ends_with("_abort") => tr("Abort", "Abbrechen"),
        _ if id.ends_with("_continue") => tr("Continue", "Fortsetzen"),
        _ if cmd.danger == Danger::Read => tr("Show", "Anzeigen"),
        _ => tr("Run", "Ausführen"),
    }
}

fn job_output(ui: &mut egui::Ui, j: &Job, max_h: f32) {
    let out = j.output();
    let running = j.state() == JobState::Running;
    egui::Frame::new().fill(Color32::from_rgb(0x1c, 0x1c, 0x1c)).corner_radius(4).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if running && out.trim().is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tr("Running…", "Läuft…")).color(theme::TEXT_DIM));
            });
        } else {
            egui::ScrollArea::vertical().id_salt(("out", j.id)).max_height(max_h).stick_to_bottom(true).show(ui, |ui| {
                let text = if out.trim().is_empty() { tr("(no output)", "(keine Ausgabe)").to_string() } else { out };
                ui.add(egui::Label::new(RichText::new(text).monospace().size(12.0)).wrap());
            });
        }
    });
}

/// The body of a command dialog: description, options, and a footer with the command shown
/// small and grey, Cancel and the main action.
#[allow(clippy::too_many_arguments)]
fn form_ui(ui: &mut egui::Ui, cmds: &[Cmd], f: &mut Form, sug: &Suggest, jobs: &[Job], line: &str, args: &[Arg], show_more: &mut bool) -> FormAction {
    let cmd = &cmds[f.cmd];
    let mut action = FormAction::None;
    ui.set_width(500.0);

    ui.label(RichText::new(c(cmd.summary)).size(12.5).color(theme::TEXT_DIM));
    if cmd.danger == Danger::Risky {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            icon(ui, Icon::Warning, 14.0, theme::ORANGE);
            ui.label(RichText::new(tr("This can discard work or rewrite history.", "Das kann Arbeit verwerfen oder die Geschichte umschreiben.")).size(12.5).color(theme::ORANGE));
        });
    }
    if cmd.kind == Kind::External {
        ui.add_space(2.0);
        ui.label(RichText::new(tr("Opens in its own window.", "Öffnet sich in einem eigenen Fenster.")).size(12.0).color(theme::TEXT_FAINT));
    }
    ui.add_space(10.0);

    if cmd.kind == Kind::Free {
        ui.horizontal(|ui| {
            ui.label(RichText::new("git").monospace().color(theme::TEXT_DIM));
            ui.add(egui::TextEdit::singleline(&mut f.free).hint_text("log --oneline -5").desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
        });
        ui.add_space(6.0);
    }

    let label_w = 120.0;
    let field_w = 340.0;
    let mut first_flag = true;
    egui::Grid::new(("form", f.cmd)).num_columns(2).spacing([12.0, 8.0]).min_col_width(label_w).show(ui, |ui| {
        for (pi, (p, v)) in cmd.params.iter().zip(f.values.iter_mut()).enumerate() {
            match (p, v) {
                (P::Text { key, label, hint, help, multi, .. }, Value::Text(t)) => {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let l = ui.label(RichText::new(c(label)).color(theme::TEXT_DIM));
                        if !help.is_empty() {
                            l.on_hover_text(c(help));
                        }
                    });
                    ui.horizontal(|ui| {
                        let opts = sug.for_key(key);
                        let w = if opts.is_empty() { field_w } else { field_w - 28.0 };
                        let te = ui.add(egui::TextEdit::singleline(t).hint_text(c(hint)).desired_width(w));
                        if !help.is_empty() {
                            te.on_hover_text(c(help));
                        }
                        if !opts.is_empty() {
                            let resp = icon_button(ui, Icon::ChevronDown, tr("Suggestions", "Vorschläge"));
                            egui::Popup::menu(&resp).show(|ui| {
                                ui.set_min_width(220.0);
                                ui.set_max_height(300.0);
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    for (val, lab) in opts.iter().take(200) {
                                        if ui.selectable_label(t == val, lab).clicked() {
                                            if *multi && !t.trim().is_empty() && !t.split_whitespace().any(|x| x == val) {
                                                t.push(' ');
                                                t.push_str(val);
                                            } else {
                                                *t = val.clone();
                                            }
                                            ui.close();
                                        }
                                    }
                                });
                            });
                        }
                    });
                    ui.end_row();
                }
                (P::Choice { label, options, .. }, Value::Choice(sel)) => {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(c(label)).color(theme::TEXT_DIM));
                    });
                    egui::ComboBox::from_id_salt(("choice", f.cmd, pi)).width(field_w).selected_text(options.get(*sel).map(|o| c(o.1)).unwrap_or("")).show_ui(ui, |ui| {
                        for (i, (_, l, h)) in options.iter().enumerate() {
                            ui.selectable_value(sel, i, c(l)).on_hover_text(c(h));
                        }
                    });
                    ui.end_row();
                }
                (P::Flag { args, label, help, .. }, Value::Bool(b)) => {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if first_flag {
                            ui.label(RichText::new(tr("Options", "Optionen")).color(theme::TEXT_DIM));
                        }
                    });
                    first_flag = false;
                    let h = if help.is_empty() { args.to_string() } else { format!("{}\n{}", c(help), args) };
                    ui.checkbox(b, c(label)).on_hover_text(h);
                    ui.end_row();
                }
                (P::Stdin { label, hint }, Value::Text(t)) => {
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        ui.label(RichText::new(c(label)).color(theme::TEXT_DIM));
                    });
                    ui.add(egui::TextEdit::multiline(t).hint_text(c(hint)).desired_rows(4).desired_width(field_w).font(egui::TextStyle::Monospace));
                    ui.end_row();
                }
                _ => {}
            }
        }
    });

    if cmd.kind == Kind::Normal || cmd.kind == Kind::External {
        ui.add_space(6.0);
        let resp = ui.add(egui::Button::new(RichText::new(if *show_more { tr("▾ Advanced", "▾ Erweitert") } else { tr("▸ Advanced", "▸ Erweitert") }).size(12.0).color(theme::TEXT_FAINT)).frame(false));
        if resp.clicked() {
            *show_more = !*show_more;
        }
        if *show_more {
            ui.horizontal(|ui| {
                ui.add_space(label_w - 60.0);
                ui.label(RichText::new(tr("Extra arguments", "Weitere Argumente")).size(12.0).color(theme::TEXT_DIM));
                ui.add(egui::TextEdit::singleline(&mut f.extra).hint_text("--verbose").desired_width(field_w - 30.0).font(egui::TextStyle::Monospace));
            });
        }
    }

    // Output for read-only commands
    if let Some(j) = f.last_job.and_then(|id| jobs.iter().find(|j| j.id == id)) {
        ui.add_space(8.0);
        job_output(ui, j, 240.0);
    }

    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0, theme::LINE);
    ui.add_space(8.0);

    let missing = catalog::missing(cmd, &f.values);
    ui.horizontal(|ui| {
        // Subtle command preview; hover explains each part, click copies.
        let text = if line.len() > 58 { format!("{}…", &line[..line.char_indices().nth(57).map(|x| x.0).unwrap_or(line.len())]) } else { line.to_string() };
        let resp = ui.add(egui::Label::new(RichText::new(text).monospace().size(11.0).color(theme::TEXT_FAINT)).sense(egui::Sense::click()));
        let args_owned: Vec<Arg> = args.to_vec();
        let full = line.to_string();
        let resp = resp.on_hover_ui(move |ui| {
            ui.set_max_width(460.0);
            ui.label(RichText::new(&full).monospace().size(12.0).color(theme::TEXT_STRONG));
            ui.add_space(4.0);
            egui::Grid::new("explain").num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
                for a in &args_owned {
                    ui.label(RichText::new(catalog::quote_for_display(a.text.trim_start_matches('!'))).monospace().size(11.0).color(theme::TEXT_DIM));
                    ui.label(RichText::new(&a.help).size(11.5));
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Click to copy", "Klicken zum Kopieren")).size(11.0).color(theme::TEXT_FAINT));
        });
        if resp.clicked() {
            ui.ctx().copy_text(line.to_string());
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can = missing.is_empty() && (cmd.kind != Kind::Free || !f.free.trim().is_empty());
            let risky = cmd.danger == Danger::Risky;
            let fill = if !can { Color32::from_rgb(0x36, 0x36, 0x39) } else if risky { Color32::from_rgb(0xa8, 0x3c, 0x3c) } else { theme::ACCENT_DARK };
            let btn = egui::Button::new(RichText::new(primary_label(cmd)).color(Color32::WHITE)).fill(fill).min_size(Vec2::new(88.0, 26.0));
            let missing_txt: Vec<&str> = missing.iter().map(|m| c(m)).collect();
            let r = ui.add_enabled(can, btn).on_disabled_hover_text(trs(format!("Please fill in: {}", missing_txt.join(", ")), format!("Bitte ausfüllen: {}", missing_txt.join(", "))));
            if r.clicked() {
                action = FormAction::Run;
            }
            if ui.add(egui::Button::new(tr("Cancel", "Abbrechen")).min_size(Vec2::new(80.0, 26.0))).clicked() {
                action = FormAction::Cancel;
            }
        });
    });
    action
}

impl App {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let sug = Suggest::from(self.tabs.get(self.active));
        if let Some(mut f) = self.dialog.take() {
            let cmd = self.cmds[f.cmd].clone();
            let args = self.form_args(&f);
            let line = self.display_line(&f);
            let more_id = Id::unique(("more", f.cmd));
            let mut more = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(more_id, || false));
            let mut open = true;
            let mut act = FormAction::None;
            egui::Window::new(RichText::new(c(cmd.title)).font(FontId::new(14.5, theme::semibold())))
                .id(Id::unique("cmd_dialog"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .pivot(Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)))
                .show(ctx, |ui| {
                    act = form_ui(ui, &self.cmds, &mut f, &sug, &self.jobs, &line, &args, &mut more);
                });
            ctx.data_mut(|d| d.insert_temp(more_id, more));
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                open = false;
            }
            match act {
                FormAction::Cancel => open = false,
                FormAction::Run => {
                    let keep = cmd.danger == Danger::Read && cmd.kind == Kind::Normal;
                    let (fc, values, extra, free) = (f.cmd, f.values.clone(), f.extra.clone(), f.free.clone());
                    let id = self.run_form(f, ctx);
                    f = Form { cmd: fc, values, extra, free, last_job: id, open_after: None };
                    if !keep || self.rebase.is_some() {
                        open = false;
                    }
                }
                FormAction::None => {}
            }
            if open {
                self.dialog = Some(f);
            }
        }

        self.rebase_dialog(ctx);
        self.quick_launch(ctx);
    }

    fn rebase_dialog(&mut self, ctx: &egui::Context) {
        if self.rebase.is_none() {
            return;
        }
        let mut open = true;
        let mut run = false;
        let mut cancel = false;
        egui::Window::new(RichText::new(tr("Interactive Rebase", "Interaktiver Rebase")).font(FontId::new(14.5, theme::semibold())))
            .id(Id::unique("rebase_dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                ui.set_width(680.0);
                let d = self.rebase.as_mut().unwrap();
                ui.label(RichText::new(tr("Choose what happens to each commit. The oldest commit is at the top; use the arrows to reorder.", "Lege fest, was mit jedem Commit passiert. Oben steht der älteste; mit den Pfeilen änderst du die Reihenfolge.")).size(12.5).color(theme::TEXT_DIM));
                ui.horizontal(|ui| {
                    icon(ui, Icon::Warning, 14.0, theme::ORANGE);
                    ui.label(RichText::new(tr("Rewrites history – don't use on commits others already have.", "Schreibt die Geschichte um – nicht für Commits, die andere schon haben.")).size(12.5).color(theme::ORANGE));
                });
                if d.has_merges {
                    ui.label(RichText::new(tr("Merge commits in this range are left out.", "Merge-Commits in diesem Bereich werden ausgelassen.")).size(12.0).color(theme::YELLOW));
                }
                ui.add_space(8.0);
                if d.items.is_empty() {
                    ui.label(RichText::new(tr("There are no commits after this one on the current branch.", "Nach diesem Commit gibt es keine Commits im aktuellen Branch.")).color(theme::TEXT_DIM));
                }
                let mut swap: Option<(usize, usize)> = None;
                egui::Frame::new().fill(theme::PANEL_DARK).corner_radius(4).inner_margin(6).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                        let n = d.items.len();
                        for (i, it) in d.items.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                if ui.add_enabled(i > 0, egui::Button::new("↑").small()).clicked() {
                                    swap = Some((i, i - 1));
                                }
                                if ui.add_enabled(i + 1 < n, egui::Button::new("↓").small()).clicked() {
                                    swap = Some((i, i + 1));
                                }
                                ui.add_space(4.0);
                                let act = RB_ACTIONS[it.action];
                                let col = match act {
                                    "drop" => theme::RED,
                                    "squash" | "fixup" => theme::YELLOW,
                                    "reword" | "edit" => theme::ACCENT,
                                    _ => theme::TEXT,
                                };
                                egui::ComboBox::from_id_salt(("rb", i, &it.short)).width(120.0).selected_text(RichText::new(rb_label(act).0).color(col)).show_ui(ui, |ui| {
                                    for (ai, a) in RB_ACTIONS.iter().enumerate() {
                                        if i == 0 && (*a == "squash" || *a == "fixup") {
                                            continue;
                                        }
                                        let (l, h) = rb_label(a);
                                        ui.selectable_value(&mut it.action, ai, l).on_hover_text(h);
                                    }
                                });
                                ui.add_space(6.0);
                                ui.label(RichText::new(&it.short).monospace().size(12.0).color(theme::TEXT_FAINT));
                                if act == "reword" {
                                    ui.add(egui::TextEdit::singleline(&mut it.new_msg).desired_width(380.0));
                                } else {
                                    let s = RichText::new(&it.subject);
                                    ui.label(if act == "drop" { s.strikethrough().color(theme::TEXT_FAINT) } else { s });
                                }
                            });
                        }
                    });
                });
                if let Some((a, b)) = swap {
                    d.items.swap(a, b);
                }
                ui.add_space(10.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 0, theme::LINE);
                ui.add_space(8.0);
                let todo = App::rebase_todo(d, &[]);
                ui.horizontal(|ui| {
                    let r = ui.label(RichText::new(format!("git rebase -i {}", &d.base[..d.base.len().min(12)])).monospace().size(11.0).color(theme::TEXT_FAINT));
                    r.on_hover_ui(|ui| {
                        ui.label(RichText::new(tr("Todo list passed to git:", "Aufgabenliste für git:")).size(11.5));
                        ui.label(RichText::new(&todo).monospace().size(11.5));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let can = !d.items.is_empty();
                        if ui.add_enabled(can, egui::Button::new(RichText::new(tr("Start Rebase", "Rebase starten")).color(Color32::WHITE)).fill(Color32::from_rgb(0xa8, 0x3c, 0x3c)).min_size(Vec2::new(110.0, 26.0))).clicked() {
                            run = true;
                        }
                        if ui.add(egui::Button::new(tr("Cancel", "Abbrechen")).min_size(Vec2::new(80.0, 26.0))).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if run {
            self.run_rebase(ctx);
        } else if cancel || !open {
            self.rebase = None;
        }
    }

    fn quick_launch(&mut self, ctx: &egui::Context) {
        let Some(mut q) = self.quick.take() else { return };
        let sel_id = Id::unique("ql_sel");
        let mut sel = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(sel_id, || 0usize));
        let mut chosen: Option<usize> = None;
        let mut close = false;
        let ql = q.to_lowercase();
        let words: Vec<&str> = ql.split_whitespace().collect();
        let hits: Vec<usize> = self
            .cmds
            .iter()
            .enumerate()
            .filter(|(_, cm)| {
                let hay = format!("{} {} {} {} {}", c(cm.title), cm.title, cm.base, c(cm.summary), cm.summary).to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .map(|(i, _)| i)
            .take(12)
            .collect();
        ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowDown) {
                sel += 1;
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                sel = sel.saturating_sub(1);
            }
            if i.key_pressed(egui::Key::Escape) {
                close = true;
            }
        });
        sel = sel.min(hits.len().saturating_sub(1));
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            chosen = hits.get(sel).copied();
        }
        let screen = ctx.content_rect();
        let w = 560.0;
        egui::Area::new(Id::unique("quick_launch"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(screen.center().x - w / 2.0, screen.top() + 90.0))
            .show(ctx, |ui| {
                egui::Frame::window(&ctx.global_style()).inner_margin(10).show(ui, |ui| {
                    ui.set_width(w);
                    ui.horizontal(|ui| {
                        icon(ui, Icon::Search, 16.0, theme::TEXT_DIM);
                        let resp = ui.add(egui::TextEdit::singleline(&mut q).hint_text(tr("What do you want to do? e.g. \"delete branch\", \"undo\", \"push\"", "Was möchtest du tun? z.B. „Branch löschen“, „rückgängig“, „Push“")).desired_width(f32::INFINITY).frame(egui::Frame::NONE).font(egui::TextStyle::Body));
                        resp.request_focus();
                        if resp.changed() {
                            sel = 0;
                        }
                    });
                    ui.add_space(6.0);
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0, theme::LINE);
                    ui.add_space(4.0);
                    if hits.is_empty() {
                        ui.label(RichText::new(tr("No matching command", "Kein passender Befehl")).color(theme::TEXT_DIM));
                    }
                    for (k, i) in hits.iter().enumerate() {
                        let cm = &self.cmds[*i];
                        let group = CATS.iter().find(|x| x.0 == cm.cat).map(|x| c(x.1)).unwrap_or("");
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), egui::Sense::click());
                        let p = ui.painter_at(rect);
                        if k == sel || resp.hovered() {
                            p.rect_filled(rect, 4, if k == sel { theme::ROW_SEL } else { theme::ROW_HOVER });
                        }
                        p.text(egui::pos2(rect.left() + 10.0, rect.top() + 12.0), Align2::LEFT_CENTER, c(cm.title), FontId::proportional(13.5), theme::TEXT_STRONG);
                        p.text(egui::pos2(rect.right() - 10.0, rect.top() + 12.0), Align2::RIGHT_CENTER, group, FontId::proportional(11.0), theme::TEXT_FAINT);
                        p.text(egui::pos2(rect.left() + 10.0, rect.bottom() - 11.0), Align2::LEFT_CENTER, c(cm.summary), FontId::proportional(11.5), theme::TEXT_DIM);
                        if resp.clicked() {
                            chosen = Some(*i);
                        }
                    }
                });
            });
        ctx.data_mut(|d| d.insert_temp(sel_id, sel));
        if let Some(i) = chosen {
            ctx.data_mut(|d| d.insert_temp(sel_id, 0usize));
            let id = self.cmds[i].id;
            self.open_cmd(id, vec![]);
        } else if !close {
            self.quick = Some(q);
        }
    }

    pub(super) fn activity_window(&mut self, ctx: &egui::Context) {
        if !self.show_activity {
            return;
        }
        let mut open = true;
        let mut clear = false;
        egui::Window::new(RichText::new(tr("Activity", "Aktivität")).font(FontId::new(14.0, theme::semibold())))
            .id(Id::unique("activity"))
            .open(&mut open)
            .default_size([700.0, 420.0])
            .pivot(Align2::RIGHT_BOTTOM)
            .default_pos(ctx.content_rect().right_bottom() - Vec2::new(24.0, 40.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Every Git command started from this window, newest first.", "Jeder Git-Befehl aus diesem Fenster, neueste zuerst.")).size(12.0).color(theme::TEXT_DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button(tr("Clear", "Leeren")).clicked() {
                            clear = true;
                        }
                    });
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    if self.jobs.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| ui.label(RichText::new(tr("Nothing has run yet.", "Noch nichts ausgeführt.")).color(theme::TEXT_FAINT)));
                    }
                    for j in self.jobs.iter_mut().rev() {
                        let st = j.state();
                        let (ic, col) = match st {
                            JobState::Running => (Icon::Activity, theme::ACCENT),
                            JobState::Done(0) | JobState::Started => (Icon::Check, theme::GREEN),
                            _ => (Icon::Warning, theme::RED),
                        };
                        let dur = j.finished.lock().unwrap().map(|f| f.duration_since(j.started).as_secs_f32()).unwrap_or_else(|| j.started.elapsed().as_secs_f32());
                        let resp = ui
                            .horizontal(|ui| {
                                if st == JobState::Running {
                                    ui.spinner();
                                } else {
                                    icon(ui, ic, 14.0, col);
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&j.title).font(FontId::new(13.0, theme::semibold())));
                                        ui.label(RichText::new(format!("{dur:.1}s")).size(11.0).color(theme::TEXT_FAINT));
                                        if st == JobState::Running && ui.small_button(tr("Cancel", "Abbrechen")).clicked() {
                                            j.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                                        }
                                    });
                                    ui.label(RichText::new(j.display.lines().next().unwrap_or("")).monospace().size(11.0).color(theme::TEXT_DIM));
                                });
                            })
                            .response
                            .interact(egui::Sense::click());
                        if resp.clicked() {
                            j.expanded = !j.expanded;
                        }
                        if j.expanded || st == JobState::Running {
                            ui.indent(("ind", j.id), |ui| job_output(ui, j, 200.0));
                        }
                        ui.add_space(4.0);
                    }
                });
            });
        if clear {
            self.jobs.retain(|j| j.state() == JobState::Running);
        }
        if !open {
            self.show_activity = false;
        }
    }

    pub(super) fn console_window(&mut self, ctx: &egui::Context) {
        if !self.show_console {
            return;
        }
        let mut open = true;
        let mut run = false;
        egui::Window::new(RichText::new(tr("Console", "Konsole")).font(FontId::new(14.0, theme::semibold())))
            .id(Id::unique("console"))
            .open(&mut open)
            .default_size([660.0, 380.0])
            .show(ctx, |ui| {
                egui::Panel::bottom("console_input").frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 0 })).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("git").monospace().color(theme::TEXT_DIM));
                        let resp = ui.add(egui::TextEdit::singleline(&mut self.console).hint_text("status").desired_width(ui.available_width() - 70.0).font(egui::TextStyle::Monospace));
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            run = true;
                            resp.request_focus();
                        }
                        if ui.button(tr("Run", "Ausführen")).clicked() {
                            run = true;
                        }
                    });
                });
                egui::ScrollArea::vertical().auto_shrink([false; 2]).stick_to_bottom(true).show(ui, |ui| {
                    egui::Frame::new().fill(Color32::from_rgb(0x1c, 0x1c, 0x1c)).inner_margin(8).corner_radius(4).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if self.console_jobs.is_empty() {
                            ui.label(RichText::new(tr("Type a Git command below, e.g. \"log --oneline -10\".", "Unten einen Git-Befehl eingeben, z.B. „log --oneline -10“.")).size(12.0).color(theme::TEXT_FAINT));
                        }
                        for id in &self.console_jobs {
                            if let Some(j) = self.jobs.iter().find(|j| j.id == *id) {
                                ui.label(RichText::new(format!("$ {}", j.display)).monospace().size(12.0).color(Color32::from_rgb(0x8f, 0xb8, 0xe6)));
                                ui.add(egui::Label::new(RichText::new(j.output()).monospace().size(12.0)).wrap());
                                ui.add_space(6.0);
                            }
                        }
                    });
                });
            });
        if run && !self.console.trim().is_empty() {
            let idx = catalog::find(&self.cmds, "free");
            let mut f = self.make_form(idx, &[]);
            f.free = self.console.clone();
            if let Some(id) = self.run_form(f, ctx) {
                self.console_jobs.push(id);
            }
            self.console.clear();
        }
        if !open {
            self.show_console = false;
        }
    }
}

#[allow(dead_code)]
fn _unused(_: Stroke) {}
