//! Annotation Object Scale List dialog for annotative objects.
//!
//! Opens for the current selection when it contains at least one annotative object.
//! Lists the selected objects' scales (Name, Paper units, Drawing units), supports
//! adding from the drawing's scale list and deleting highlighted ones, and applies
//! all changes with OBJECTSCALE on OK in one undo step. Cancel discards edits.

use egui::{RichText, vec2};
use serde_json::json;

use crate::CadApp;

/// Dialog id in `app.ui.dialog`.
pub const DIALOG: &str = "objectscale";

/// Dialog state kept while it is open.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct State {
    /// Annotative objects (hex handles).
    pub handles: Vec<String>,
    /// Names of scales added in the dialog (not yet applied).
    pub added: Vec<String>,
    /// Names of scales marked for deletion in the dialog (not yet applied).
    pub removed: Vec<String>,
    /// Highlighted rows (scale names).
    pub highlighted: Vec<String>,
    /// When true, only scales common to all selected objects are shown.
    pub common_only: bool,
    /// The "Add Scales to Object" picker is open with these names ticked.
    pub adding: Option<Vec<String>>,
}

/// Selected annotative objects (hex handles).
fn annotative_selection(app: &CadApp) -> Vec<String> {
    let Ok(d) = app.session.doc() else { return Vec::new() };
    app.session.selection().into_iter().filter(|h| d.entity(*h).is_some_and(|e| draftcraft_doc::is_annotative(e))).map(|h| h.hex()).collect()
}

/// Open the dialog for the current selection; false when there is nothing to edit.
pub fn open(app: &mut CadApp) -> bool {
    let handles = annotative_selection(app);
    if handles.is_empty() {
        return false;
    }
    app.objscale_dlg = Some(State { handles, ..Default::default() });
    app.ui.dialog = Some(DIALOG.into());
    true
}

fn scales_of(d: &draftcraft_doc::Drawing, h: &str) -> Vec<draftcraft_doc::AnnoScale> {
    draftcraft_doc::Handle::parse_hex(h).and_then(|h| d.entity(h)).map(|e| e.common.anno_scales.clone()).unwrap_or_default()
}

/// Rows to show: the selected objects' scales (all or only those in common), minus removed
/// and plus added, ordered as the drawing's scale list.
pub fn rows(d: &draftcraft_doc::Drawing, st: &State) -> Vec<draftcraft_doc::AnnoScale> {
    let lists: Vec<Vec<draftcraft_doc::AnnoScale>> = st.handles.iter().map(|h| scales_of(d, h)).collect();
    let has = |list: &[draftcraft_doc::AnnoScale], n: &str| list.iter().any(|s| s.name.eq_ignore_ascii_case(n));
    let mut out: Vec<draftcraft_doc::AnnoScale> = Vec::new();
    for sc in lists.iter().flatten() {
        let shown = if st.common_only { lists.iter().all(|l| has(l, &sc.name)) } else { true };
        if shown && !has(&out, &sc.name) && !st.removed.iter().any(|r| r.eq_ignore_ascii_case(&sc.name)) {
            out.push(sc.clone());
        }
    }
    for n in &st.added {
        if !has(&out, n)
            && let Some(sc) = d.anno_scale(n)
        {
            out.push(sc.clone());
        }
    }
    let pos = |s: &draftcraft_doc::AnnoScale| d.scales.iter().position(|x| x.name.eq_ignore_ascii_case(&s.name)).unwrap_or(usize::MAX);
    out.sort_by_key(pos);
    out
}

fn units(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// One scale row: Name (click to highlight) | Paper units | = Drawing units.
fn row(ui: &mut egui::Ui, sc: &draftcraft_doc::AnnoScale, on: bool) -> bool {
    let clicked = ui.add_sized(vec2(170.0, 18.0), egui::Button::selectable(on, &sc.name)).clicked();
    ui.label(units(sc.paper_units));
    ui.label(format!("= {}", units(sc.drawing_units)));
    ui.end_row();
    clicked
}

fn header(ui: &mut egui::Ui) {
    ui.label(RichText::new(crate::tl!("Name")).strong());
    ui.label(RichText::new(crate::tl!("Paper units")).strong());
    ui.label(RichText::new(crate::tl!("Drawing units")).strong());
    ui.end_row();
}

fn toggle(list: &mut Vec<String>, name: &str) {
    if let Some(i) = list.iter().position(|n| n.eq_ignore_ascii_case(name)) {
        list.remove(i);
    } else {
        list.push(name.to_string());
    }
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let Some(mut st) = app.objscale_dlg.take() else {
        *open = false;
        return;
    };
    let Ok(d) = app.session.doc() else {
        *open = false;
        return;
    };
    let d = d.clone();
    let shown = rows(&d, &st);
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(crate::tl!("Annotation Object Scale List"))
        .id(egui::Id::unique("objectscale"))
        .open(open)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.label(crate::tl!("Object Scale List"));
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_size(vec2(340.0, 160.0));
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    egui::Grid::new("objectscale_rows").num_columns(3).striped(true).show(ui, |ui| {
                        header(ui);
                        for sc in &shown {
                            let on = st.highlighted.iter().any(|n| n.eq_ignore_ascii_case(&sc.name));
                            if row(ui, sc, on) {
                                toggle(&mut st.highlighted, &sc.name);
                            }
                        }
                    });
                });
            });
            ui.horizontal(|ui| {
                if ui.button("+").on_hover_text(crate::tl!("Add")).clicked() {
                    st.adding = Some(Vec::new());
                }
                // An object must keep at least one scale: only allow deletion if at least one remains.
                let can_delete = !st.highlighted.is_empty() && shown.len() > st.highlighted.len();
                let del = ui.add_enabled(can_delete, egui::Button::new("-")).on_hover_text(crate::tl!("Delete"));
                if del.clicked() {
                    for n in std::mem::take(&mut st.highlighted) {
                        if let Some(i) = st.added.iter().position(|a| a.eq_ignore_ascii_case(&n)) {
                            st.added.remove(i);
                        } else if !st.removed.iter().any(|r| r.eq_ignore_ascii_case(&n)) {
                            st.removed.push(n);
                        }
                    }
                }
            });
            ui.radio_value(&mut st.common_only, false, crate::tl!("List all scales for selected objects"));
            ui.radio_value(&mut st.common_only, true, crate::tl!("List scales common to all selected objects only"));
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ok = ui.button(crate::tl!("OK")).clicked();
                    cancel = ui.button(crate::tl!("Cancel")).clicked();
                });
            });
        });
    if let Some(mut ticked) = st.adding.take() {
        let (mut add_ok, mut add_cancel) = (false, false);
        let mut still_open = true;
        egui::Window::new(crate::tl!("Add Scales to Object"))
            .id(egui::Id::unique("objectscale_add"))
            .open(&mut still_open)
            .resizable(false)
            .collapsible(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center() + vec2(30.0, 30.0))
            .show(ctx, |ui| {
                ui.label(crate::tl!("Scale List"));
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        egui::Grid::new("objectscale_add_rows").num_columns(3).striped(true).show(ui, |ui| {
                            header(ui);
                            for sc in d.scales.iter().filter(|s| s.is_valid()) {
                                let on = ticked.iter().any(|n| n.eq_ignore_ascii_case(&sc.name));
                                if row(ui, sc, on) {
                                    toggle(&mut ticked, &sc.name);
                                }
                            }
                        });
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        add_ok = ui.button(crate::tl!("OK")).clicked();
                        add_cancel = ui.button(crate::tl!("Cancel")).clicked();
                    });
                });
            });
        if add_ok {
            for n in ticked {
                if let Some(i) = st.removed.iter().position(|r| r.eq_ignore_ascii_case(&n)) {
                    st.removed.remove(i);
                } else if !st.added.iter().any(|a| a.eq_ignore_ascii_case(&n)) && !shown.iter().any(|s| s.name.eq_ignore_ascii_case(&n)) {
                    st.added.push(n);
                }
            }
        } else if !add_cancel && still_open {
            st.adding = Some(ticked);
        }
    }
    if ok {
        apply(app, &st);
    }
    if ok || cancel || !*open {
        *open = false;
    } else {
        app.objscale_dlg = Some(st);
    }
}

/// Apply the dialog's additions and deletions through OBJECTSCALE (one undo step).
pub fn apply(app: &mut CadApp, st: &State) {
    if st.added.is_empty() && st.removed.is_empty() {
        return;
    }
    // Group in one undo step.
    let _ = app.run("undo", json!({ "mode": "begin" }));
    let mut p = json!({ "handles": st.handles });
    if !st.added.is_empty() {
        p["add"] = json!(st.added);
    }
    if !st.removed.is_empty() {
        p["delete"] = json!(st.removed);
    }
    if let Err(e) = app.run("objectscale", p) {
        app.session.echo(e.to_string());
    }
    let _ = app.run("undo", json!({ "mode": "end" }));
}
