//! Blocks palette: browse and insert blocks from the current drawing, a recent list, or another
//! drawing file. Shows thumbnail previews, a filter box, and basic insert options.
//!
//! Non-GUI logic (filtering, recent list persistence, and parameter mapping) is kept testable.

use std::sync::Arc;

use draftcraft_engine::doc::{Block, Drawing};
use draftcraft_geom::Vec2;
use draftcraft_render::DisplayList;
use egui::{Rect, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// Max recent entries kept.
const MAX_RECENT: usize = 50;

/// One recent block entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    /// Block name (case-insensitive; stored as defined).
    pub name: String,
    /// Optional source drawing file for library blocks.
    #[serde(default)]
    pub file: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Current,
    Recent,
    Libraries,
}

impl Default for Tab {
    fn default() -> Self {
        Tab::Current
    }
}

/// Insert options from the palette. Mapping to JSON is pure for tests.
#[derive(Clone, Debug, PartialEq)]
pub struct InsertOptions {
    pub name: String,
    pub explode: bool,
    /// If Some, insert immediately at this point; otherwise start interactive insert.
    pub at: Option<Vec2>,
    /// If Some, use uniform scale; else if x/y provided, use non-uniform; else 1.
    pub uniform_scale: Option<f64>,
    pub xy_scale: Option<Vec2>,
    /// If Some, rotation in degrees; else 0.
    pub rotation_deg: Option<f64>,
}

impl InsertOptions {
    /// Map options to parameters for engine.execute("insert", params), or None when interactive.
    pub fn to_insert_params(&self) -> Option<Value> {
        let at = self.at?;
        let mut p = json!({
            "name": if self.explode { format!("*{}", self.name) } else { self.name.clone() },
            "at": [at.x, at.y],
        });
        if let Some(sc) = self.uniform_scale {
            p["scale"] = json!(sc);
        } else if let Some(xy) = self.xy_scale {
            p["scale"] = json!([xy.x, xy.y]);
        }
        if let Some(a) = self.rotation_deg {
            p["rotation"] = json!(a);
        }
        // When explode is requested on non-interactive path, INSERT supports `"explode": true`.
        if self.explode {
            p["explode"] = json!(true);
        }
        Some(p)
    }
}

/// Blocks palette state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    tab: TabSer,
    #[serde(default)]
    filter: String,
    /// Recently inserted blocks (most-recent-first).
    #[serde(default)]
    recent: Vec<Recent>,
    /// Libraries pane: last opened path and listing cache.
    #[serde(default)]
    lib: LibraryState,
}

/// Serializable form of Tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum TabSer {
    #[default]
    Current,
    Recent,
    Libraries,
}

impl From<TabSer> for Tab {
    fn from(s: TabSer) -> Self {
        match s {
            TabSer::Current => Tab::Current,
            TabSer::Recent => Tab::Recent,
            TabSer::Libraries => Tab::Libraries,
        }
    }
}

impl From<Tab> for TabSer {
    fn from(t: Tab) -> Self {
        match t {
            Tab::Current => TabSer::Current,
            Tab::Recent => TabSer::Recent,
            Tab::Libraries => TabSer::Libraries,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct LibraryState {
    /// Path text field.
    #[serde(default)]
    path: String,
    /// Listing returned by ADCENTER for the current path (blocks only).
    #[serde(default)]
    listing_blocks: Vec<(String, usize)>,
    /// Error message to show in the UI.
    #[serde(default)]
    error: Option<String>,
}

impl State {
    pub(crate) fn prefs(&self) -> Value {
        json!({
            "tab": self.tab,
            "filter": self.filter,
            "recent": self.recent,
            "library": self.lib,
        })
    }
    pub(crate) fn load_prefs(&mut self, v: &Value) {
        if let Some(t) = v.get("tab").and_then(|x| serde_json::from_value::<TabSer>(x.clone()).ok()) {
            self.tab = t;
        }
        self.filter = v.get("filter").and_then(Value::as_str).unwrap_or_default().to_string();
        self.recent = v.get("recent").and_then(|x| serde_json::from_value::<Vec<Recent>>(x.clone()).ok()).unwrap_or_default();
        self.lib = v.get("library").and_then(|x| serde_json::from_value::<LibraryState>(x.clone()).ok()).unwrap_or_default();
    }

    /// Record a block as recently inserted; newest first, deduping by (name,file).
    pub fn record_recent(&mut self, name: &str, file: Option<&str>) {
        let key = (name.to_string(), file.map(|s| s.to_string()));
        self.recent.retain(|r| {
            !(r.name.eq_ignore_ascii_case(&key.0)
                && r.file.as_deref().map(|p| p.eq_ignore_ascii_case(key.1.as_deref().unwrap_or(""))).unwrap_or(key.1.is_none()))
        });
        self.recent.insert(0, Recent { name: key.0, file: key.1 });
        if self.recent.len() > MAX_RECENT {
            self.recent.truncate(MAX_RECENT);
        }
    }
}

/// Compute a block preview display list for a block definition inside the current document.
fn block_preview(doc: &Drawing, b: &Block) -> Arc<DisplayList> {
    // Rough list for bounds, then rebuild with an adaptive tolerance.
    let rough = draftcraft_render::build_entities(doc, b.entities.iter().map(|e| e.as_ref()), &draftcraft_render::Options::default());
    if rough.bounds.is_empty() {
        return Arc::new(rough);
    }
    let size = rough.bounds.width().max(rough.bounds.height());
    let opts = draftcraft_render::Options { tolerance: (size / 400.0).max(1e-9), ..Default::default() };
    Arc::new(draftcraft_render::build_entities(doc, b.entities.iter().map(|e| e.as_ref()), &opts))
}

/// Draw one preview tile (thumbnail + label). Returns clicked name.
fn tile(ui: &mut egui::Ui, name: &str, list: &DisplayList, selected: bool) -> bool {
    let t = Tokens::get();
    let label_h = 16.0;
    let w = (ui.available_width() - 16.0).clamp(120.0, 280.0);
    let h = w; // square thumbnail
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h + label_h + 4.0), egui::Sense::click());
    let p = ui.painter();
    // Frame
    p.rect_filled(rect, 3.0, if selected { t.control_hover } else { t.control });
    // Thumbnail rect
    let thumb = Rect::from_min_size(rect.min + vec2(2.0, 2.0), vec2(w - 4.0, h - 4.0));
    // Draw the display list fitted into the thumbnail, as the canvas does.
    if !list.bounds.is_empty() && !list.prims.is_empty() {
        let b = list.bounds;
        let inner = thumb.shrink(6.0);
        let (iw, ih) = (f64::from(inner.width()).max(1e-9), f64::from(inner.height()).max(1e-9));
        let scale = (iw / b.width()).min(ih / b.height()) * 0.95;
        let xf = crate::canvas::Xf {
            rect: inner,
            center: Vec2::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0),
            scale,
            twist: 0.0,
        };
        crate::canvas::draw_list(&p, &xf, list, draftcraft_color::Rgb(t.panel.r(), t.panel.g(), t.panel.b()), false);
    }
    // Label
    p.text(egui::pos2(rect.center().x, rect.bottom() - label_h / 2.0 - 2.0), egui::Align2::CENTER_CENTER, name, crate::theme::small(), t.text);
    resp.clicked()
}

/// Open the Blocks palette.
pub(crate) fn open(app: &mut CadApp) -> Result<Value, String> {
    crate::docking::open_on_demand(app, crate::docking::Panel::Blocks, 320.0)
}

/// The Blocks palette body.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut st = std::mem::take(&mut app.side_palettes.blocks);
    let mut start_insert_interactive: Option<(String, Option<String>, bool)> = None; // (name, file, explode)

    ui.add_space(6.0);
    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
    // Tabs
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        for (tab, label) in
            [(Tab::Current, crate::tl!("Current Drawing")), (Tab::Recent, crate::tl!("Recent")), (Tab::Libraries, crate::tl!("Libraries"))]
        {
            let on = Tab::from(st.tab) == tab;
            let r = ui.selectable_label(on, label);
            if r.clicked() {
                st.tab = TabSer::from(tab);
            }
        }
    });
    // Filter
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let hint = crate::tl!("Filter blocks");
        let te = egui::TextEdit::singleline(&mut st.filter).hint_text(hint).desired_width(ui.available_width() - 130.0);
        ui.add(te);
        // Insert options summary (explode toggle only; interactive handles point/scale/rotation)
        ui.checkbox(&mut app.ui.blocks_explode, crate::tl!("Explode"));
    });
    ui.separator();

    match Tab::from(st.tab) {
        Tab::Current => {
            let Ok(d) = app.session.doc() else {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(crate::tl!("No drawing open.")).color(t.text_dim));
                });
                app.side_palettes.blocks = st;
                return;
            };
            let mut blocks: Vec<_> = d
                .blocks
                .values()
                .filter(|b| !b.anonymous && b.xref_path.is_none())
                .filter(|b| st.filter.is_empty() || b.name.to_lowercase().contains(&st.filter.to_ascii_lowercase()))
                .collect();
            blocks.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
            egui::ScrollArea::vertical().id_salt("blocks-current").auto_shrink([false, false]).show(ui, |ui| {
                let mut row = 0;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                    for b in blocks {
                        // Cache preview per (uid, revision, name).
                        let key =
                            (app.session.state().map(|s| s.uid).unwrap_or(0), app.session.state().map(|s| s.revision).unwrap_or(0), b.name.clone());
                        let list = ui.data(|d| d.get_temp::<Arc<DisplayList>>(egui::Id::new(("block-preview", key.0, key.1, &key.2))));
                        let list = match list {
                            Some(l) => l,
                            None => {
                                let l = block_preview(&d, b);
                                let out = l.clone();
                                ui.data_mut(|dd| dd.insert_temp(egui::Id::new(("block-preview", key.0, key.1, &key.2)), out));
                                l
                            }
                        };
                        if tile(ui, &b.name, &list, false) {
                            // Interactive insertion: -insert then block name (prefix * for explode).
                            start_insert_interactive = Some((b.name.clone(), None, app.ui.blocks_explode));
                            st.record_recent(&b.name, None);
                        }
                        row += 1;
                    }
                });
            });
        }
        Tab::Recent => {
            egui::ScrollArea::vertical().id_salt("blocks-recent").auto_shrink([false, false]).show(ui, |ui| {
                if st.recent.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(crate::tl!("No recent blocks.")).color(t.text_dim));
                    });
                }
                for r in st.recent.clone() {
                    let label = if let Some(f) = &r.file { format!("{}  ({})", r.name, file_name(f)) } else { r.name.clone() };
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        if ui.button(label).clicked() {
                            // If library block, ensure it's copied into the drawing first.
                            if let Some(f) = &r.file
                                && app.session.doc().is_ok_and(|d| d.block(&r.name).is_none())
                            {
                                let _ = app.run("adcenter", json!({ "path": f, "kind": "blocks", "names": [r.name.clone()] }));
                            }
                            start_insert_interactive = Some((r.name.clone(), r.file.clone(), app.ui.blocks_explode));
                        }
                    });
                }
            });
        }
        Tab::Libraries => {
            libraries_body(app, ui, &mut st, &mut start_insert_interactive);
        }
    }

    // Execute interactive insert action
    if let Some((name, file, explode)) = start_insert_interactive {
        // If this came from a library and the block isn't defined yet, copy it first.
        if let Some(f) = &file
            && app.session.doc().is_ok_and(|d| d.block(&name).is_none())
        {
            if let Err(e) = app.run("adcenter", json!({ "path": f, "kind": "blocks", "names": [name.clone()] })) {
                st.lib.error = Some(e);
            }
        }
        app.cmdline("-insert");
        let typed = if explode { format!("*{name}") } else { name };
        if let Err(e) = app.session.input(draftcraft_engine::Input::Text(typed)) {
            st.lib.error = Some(e.to_string());
        }
    }
    app.side_palettes.blocks = st;
}

fn libraries_body(app: &mut CadApp, ui: &mut egui::Ui, st: &mut State, start_insert_interactive: &mut Option<(String, Option<String>, bool)>) {
    let t = Tokens::get();
    // Open...
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        if ui.button(crate::tl!("Open...")).clicked() {
            match app.services.pick_file.as_ref() {
                Some(pick) => {
                    if let Some(p) = pick("Drawings (DWG, DXF)", &["dwg", "dxf"]) {
                        st.lib.path = p.clone();
                        load_listing(app, st, &p);
                    }
                }
                None => st.lib.error = Some(crate::i18n::t("Type the drawing's path, then press Enter.").to_string()),
            }
        }
        let field = egui::TextEdit::singleline(&mut st.lib.path).hint_text(crate::tl!("Drawing file")).desired_width(ui.available_width() - 8.0);
        let r = ui.add(field);
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !st.lib.path.trim().is_empty() {
            let p = st.lib.path.trim().to_string();
            load_listing(app, st, &p);
        }
    });
    if let Some(e) = &st.lib.error {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.colored_label(t.warn, e);
        });
    }
    ui.separator();
    // Grid of blocks in the file (names only; no external preview yet).
    if st.lib.listing_blocks.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(crate::tl!("Open a drawing to see its blocks.")).color(t.text_dim));
        });
        return;
    }
    let mut add_recent: Vec<String> = Vec::new();
    egui::ScrollArea::vertical().id_salt("blocks-library").auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
            for (name, _count) in
                st.lib.listing_blocks.iter().filter(|(n, _)| st.filter.is_empty() || n.to_lowercase().contains(&st.filter.to_ascii_lowercase()))
            {
                // Library tile: use icon + name (no preview for external file without import).
                let (rect, resp) = ui.allocate_exact_size(vec2((ui.available_width() - 16.0).clamp(120.0, 280.0), 40.0), egui::Sense::click());
                let p = ui.painter();
                p.rect_filled(rect, 3.0, Tokens::get().control);
                icons::paint(p, Rect::from_min_size(rect.min + vec2(6.0, 10.0), vec2(18.0, 18.0)), Icon::Blocks, false);
                p.text(egui::pos2(rect.left() + 30.0, rect.center().y), egui::Align2::LEFT_CENTER, name, crate::theme::body(), Tokens::get().text);
                if resp.clicked() {
                    // Copy into drawing, then interactive insert.
                    *start_insert_interactive = Some((name.clone(), Some(st.lib.path.clone()), app.ui.blocks_explode));
                    // Defer recording recent to avoid mutating while iterating.
                    add_recent.push(name.clone());
                }
            }
        });
    });
    let lib_path = st.lib.path.clone();
    for n in add_recent {
        st.record_recent(&n, Some(&lib_path));
    }
}

fn load_listing(app: &mut CadApp, st: &mut State, path: &str) {
    match app.run("adcenter", json!({ "path": path })) {
        Ok(v) => {
            let blocks: Vec<(String, usize)> = v
                .get("blocks")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|b| {
                            Some((
                                b.get("name")?.as_str()?.to_string(),
                                b.get("objects").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut blocks = blocks;
            blocks.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
            st.lib.listing_blocks = blocks;
            st.lib.error = None;
        }
        Err(e) => st.lib.error = Some(e),
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_params_mapping() {
        let p = InsertOptions {
            name: "Door".into(),
            explode: true,
            at: Some(Vec2::new(10.0, 20.0)),
            uniform_scale: Some(2.0),
            xy_scale: None,
            rotation_deg: Some(45.0),
        }
        .to_insert_params()
        .unwrap();
        assert_eq!(p["name"], "*Door");
        assert_eq!(p["at"], json!([10.0, 20.0]));
        assert_eq!(p["scale"], json!(2.0));
        assert_eq!(p["rotation"], json!(45.0));
        assert_eq!(p["explode"], json!(true));
    }

    #[test]
    fn recent_persists_and_dedups() {
        let mut st = State::default();
        st.record_recent("A", None);
        st.record_recent("B", Some("lib.dwg"));
        st.record_recent("a", None); // dedup by name-case-insensitive
        assert_eq!(st.recent.len(), 2);
        let prefs = st.prefs();
        let mut st2 = State::default();
        st2.load_prefs(&prefs);
        assert_eq!(st2.recent, st.recent);
    }

    #[test]
    fn filter_is_case_insensitive() {
        let mut st = State::default();
        st.filter = "win".into();
        assert!(st.filter.to_ascii_lowercase().contains("win"));
    }
}
