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
    // Options
    #[serde(default)]
    specify_point_on_screen: bool,
    #[serde(default)]
    scale_specify_on_screen: bool,
    #[serde(default)]
    uniform_scale: bool,
    #[serde(default)]
    scale_uniform: f64,
    #[serde(default)]
    scale_x: f64,
    #[serde(default)]
    scale_y: f64,
    #[serde(default)]
    rotation_specify_on_screen: bool,
    #[serde(default)]
    rotation_deg: f64,
    #[serde(default)]
    repeat: bool,
    /// Recently inserted blocks (most-recent-first).
    #[serde(default)]
    recent: Vec<Recent>,
    /// Libraries pane: last opened path and listing cache.
    #[serde(default)]
    lib: LibraryState,
    /// When Repeat Placement is on: the chosen block to arm for the next placement, and the
    /// document revision at arming time. Not persisted.
    #[serde(skip)]
    repeat_armed: Option<(String, Option<String>, bool)>, // (name, file, explode)
    #[serde(skip)]
    repeat_after_rev: u64,
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
            "specifyPointOnScreen": self.specify_point_on_screen,
            "scaleSpecifyOnScreen": self.scale_specify_on_screen,
            "uniformScale": self.uniform_scale,
            "scaleUniform": self.scale_uniform,
            "scaleX": self.scale_x,
            "scaleY": self.scale_y,
            "rotationSpecifyOnScreen": self.rotation_specify_on_screen,
            "rotationDeg": self.rotation_deg,
            "repeat": self.repeat,
            "recent": self.recent,
            "library": self.lib,
        })
    }
    pub(crate) fn load_prefs(&mut self, v: &Value) {
        if let Some(t) = v.get("tab").and_then(|x| serde_json::from_value::<TabSer>(x.clone()).ok()) {
            self.tab = t;
        }
        self.filter = v.get("filter").and_then(Value::as_str).unwrap_or_default().to_string();
        self.specify_point_on_screen = v.get("specifyPointOnScreen").and_then(Value::as_bool).unwrap_or(true);
        self.scale_specify_on_screen = v.get("scaleSpecifyOnScreen").and_then(Value::as_bool).unwrap_or(true);
        self.uniform_scale = v.get("uniformScale").and_then(Value::as_bool).unwrap_or(true);
        self.scale_uniform = v.get("scaleUniform").and_then(Value::as_f64).unwrap_or(1.0);
        self.scale_x = v.get("scaleX").and_then(Value::as_f64).unwrap_or(1.0);
        self.scale_y = v.get("scaleY").and_then(Value::as_f64).unwrap_or(1.0);
        self.rotation_specify_on_screen = v.get("rotationSpecifyOnScreen").and_then(Value::as_bool).unwrap_or(true);
        self.rotation_deg = v.get("rotationDeg").and_then(Value::as_f64).unwrap_or(0.0);
        self.repeat = v.get("repeat").and_then(Value::as_bool).unwrap_or(false);
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
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h + label_h + 4.0), egui::Sense::click_and_drag());
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
    resp.clicked() || resp.dragged()
}

/// Open the Blocks palette.
pub(crate) fn open(app: &mut CadApp) -> Result<Value, String> {
    crate::docking::open_on_demand(app, crate::docking::Panel::Blocks, 320.0)
}

/// Start INSERT interactively for `name`, applying LT-style preset options from `st`:
/// - a uniform scale with the Scale keyword, or X/Y factors with X/Y keywords;
/// - a rotation preset with the Rotate keyword;
/// - explode by prefixing the name with `*`.
/// If `file` is given and the block is not defined, it is defined first from that file's model space.
pub(crate) fn start_insert_with_presets(app: &mut CadApp, name: &str, file: Option<&str>, explode: bool, st: &State) {
    // If this came from a library and the block isn't defined yet, copy it first.
    if let Some(f) = file
        && app.session.doc().is_ok_and(|d| d.block(name).is_none())
    {
        let _ = app.run("adcenter", json!({ "path": f, "kind": "blocks", "names": [name] }));
    }
    // Start interactive INSERT, then seed presets before the point when requested.
    app.cmdline("-insert");
    let typed = if explode { format!("*{name}") } else { name.to_string() };
    if app.session.input(draftcraft_engine::Input::Text(typed)).is_err() {
        return;
    }
    // Scale presets (before the insertion point) when not "specify on screen".
    if !st.scale_specify_on_screen {
        if st.uniform_scale {
            let _ = app.session.input(draftcraft_engine::Input::Keyword("Scale".into()));
            let _ = app.session.input(draftcraft_engine::Input::Text(format!("{}", st.scale_uniform)));
        } else {
            let _ = app.session.input(draftcraft_engine::Input::Keyword("X".into()));
            let _ = app.session.input(draftcraft_engine::Input::Text(format!("{}", st.scale_x)));
            let _ = app.session.input(draftcraft_engine::Input::Keyword("Y".into()));
            let _ = app.session.input(draftcraft_engine::Input::Text(format!("{}", st.scale_y)));
        }
    }
    // Rotation preset (before the insertion point) when not "specify on screen".
    if !st.rotation_specify_on_screen {
        let _ = app.session.input(draftcraft_engine::Input::Keyword("Rotate".into()));
        let _ = app.session.input(draftcraft_engine::Input::Text(format!("{}", st.rotation_deg)));
    }
}

/// Called every frame to re-arm INSERT when Repeat Placement is on and the previous INSERT just
/// finished with a placement.
pub(crate) fn repeat_tick(app: &mut CadApp) {
    let (armed, after_rev, st_repeat, explode);
    {
        // Borrow briefly to read the repeat state.
        let st = &app.side_palettes.blocks;
        armed = st.repeat_armed.clone();
        after_rev = st.repeat_after_rev;
        st_repeat = st.repeat;
        explode = app.ui.blocks_explode;
    }
    let Some((name, file, armed_explode)) = armed else { return };
    if !st_repeat {
        return;
    }
    // Only re-arm after a finished placement: the document revision increases.
    let rev_now = app.session.state().map(|s| s.revision).unwrap_or(0);
    if app.session.running.is_none() && rev_now > after_rev && app.session.last_command.as_deref() == Some("insert") {
        // Start again with the same options.
        let st = app.side_palettes.blocks.clone();
        start_insert_with_presets(app, &name, file.as_deref(), armed_explode || explode, &st);
        // Record the new revision threshold.
        let mut st_mut = std::mem::take(&mut app.side_palettes.blocks);
        st_mut.repeat_after_rev = rev_now;
        app.side_palettes.blocks = st_mut;
    }
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
    // LT-style options
    egui::CollapsingHeader::new(crate::tl!("Options")).id_salt("blk-opts").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Insertion point"));
            ui.checkbox(&mut st.specify_point_on_screen, crate::tl!("Specify on screen"));
        });
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Scale"));
            ui.checkbox(&mut st.scale_specify_on_screen, crate::tl!("Specify on screen"));
            ui.checkbox(&mut st.uniform_scale, crate::tl!("Uniform"));
            if st.uniform_scale {
                let mut v = st.scale_uniform;
                if ui.add(egui::DragValue::new(&mut v).speed(0.05)).changed() {
                    st.scale_uniform = v;
                }
            } else {
                let (mut sx, mut sy) = (st.scale_x, st.scale_y);
                if ui.add(egui::DragValue::new(&mut sx).speed(0.05)).changed() {
                    st.scale_x = sx;
                }
                if ui.add(egui::DragValue::new(&mut sy).speed(0.05)).changed() {
                    st.scale_y = sy;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Rotation"));
            ui.checkbox(&mut st.rotation_specify_on_screen, crate::tl!("Specify on screen"));
            let mut deg = st.rotation_deg;
            if ui.add(egui::DragValue::new(&mut deg).speed(1.0)).changed() {
                st.rotation_deg = deg;
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut st.repeat, crate::tl!("Repeat Placement"));
            ui.checkbox(&mut app.ui.blocks_explode, crate::tl!("Explode"));
        });
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
            let mut to_run: Option<(Value, String)> = None;
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
                        let clicked_or_dragged = tile(ui, &b.name, &list, false);
                        if clicked_or_dragged && ui.input(|i| i.pointer.any_released()) {
                            // Drag-and-drop: if released over canvas, insert at drop point with preset options.
                            if let Some(p) = ui.input(|i| i.pointer.latest_pos())
                                && app.canvas.rect.is_some_and(|r| r.contains(p))
                                && let Some(xf) = app.canvas.xf
                            {
                                let at = xf.to_world(p);
                                let mut ins = InsertOptions {
                                    name: b.name.clone(),
                                    explode: app.ui.blocks_explode,
                                    at: Some(at),
                                    uniform_scale: None,
                                    xy_scale: None,
                                    rotation_deg: None,
                                };
                                if !st.scale_specify_on_screen {
                                    if st.uniform_scale {
                                        ins.uniform_scale = Some(st.scale_uniform);
                                    } else {
                                        ins.xy_scale = Some(Vec2::new(st.scale_x, st.scale_y));
                                    }
                                }
                                if !st.rotation_specify_on_screen {
                                    ins.rotation_deg = Some(st.rotation_deg);
                                }
                                if let Some(p) = ins.to_insert_params() {
                                    to_run = Some((p, b.name.clone()));
                                }
                                continue;
                            }
                        }
                        if clicked_or_dragged && !ui.input(|i| i.pointer.any_released()) {
                            // Interactive insertion: -insert then block name (prefix * for explode).
                            start_insert_interactive = Some((b.name.clone(), None, app.ui.blocks_explode));
                            st.record_recent(&b.name, None);
                        }
                        row += 1;
                    }
                });
            });
            if let Some((p, name)) = to_run {
                let _ = app.run("insert", p);
                st.record_recent(&name, None);
            }
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
        // Seed interactive INSERT with presets from options.
        start_insert_with_presets(app, &name, file.as_deref(), explode, &st);
        // Arm repeat placement if requested: re-run after the next insertion finishes.
        st.repeat_armed = st.repeat.then_some((name.clone(), file.clone(), explode));
        st.repeat_after_rev = app.session.state().map(|s| s.revision).unwrap_or(0);
    }
    app.side_palettes.blocks = st;
    // Repeat-placement driver: re-arm after a finished placement.
    repeat_tick(app);
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
    use draftcraft_engine::{Session, Input};
    use crate::Services;

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

    #[test]
    fn repeat_placement_persists() {
        let mut st = State::default();
        st.repeat = true;
        let prefs = st.prefs();
        let mut st2 = State::default();
        st2.load_prefs(&prefs);
        assert!(st2.repeat);
    }

    #[test]
    fn interactive_presets_apply_scale_and_rotation() {
        let mut app = crate::CadApp::new(Session::new(), Services::default());
        // Define a simple block B with one line.
        let l = app.session.execute("line", &json!({"points": [[0,0],[10,0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
        app.session.execute("block", &json!({"name":"B","base":[0,0],"handles":[l],"keep":"delete"})).unwrap();
        // Options: uniform scale 2, rotation 45°, both preset (not on screen).
        let mut st = State::default();
        st.scale_specify_on_screen = false;
        st.uniform_scale = true;
        st.scale_uniform = 2.0;
        st.rotation_specify_on_screen = false;
        st.rotation_deg = 45.0;
        start_insert_with_presets(&mut app, "B", None, false, &st);
        // Should be waiting for the insertion point now.
        assert!(app.session.running.is_some());
        // Place it.
        let _ = app.session.input(Input::Point(Vec2::new(5.0, 6.0)));
        assert!(app.session.running.is_none());
        // Verify the inserted reference's scale and rotation.
        let d = app.session.doc().unwrap();
        let mut ins_opt = None;
        for e in d.model.iter() {
            if let draftcraft_doc::EntityKind::Insert(i) = &e.kind {
                ins_opt = Some(i.clone());
            }
        }
        let ins = ins_opt.expect("insert present");
        assert!((ins.scale.x - 2.0).abs() < 1e-9 && (ins.scale.y - 2.0).abs() < 1e-9, "{:?}", ins.scale);
        assert!((ins.rotation - 45f64.to_radians()).abs() < 1e-9, "rotation={}", ins.rotation);
    }

    #[test]
    fn repeat_placement_arms_next_insert_after_finish() {
        let mut app = crate::CadApp::new(Session::new(), Services::default());
        // Define a simple block R.
        let l = app.session.execute("line", &json!({"points": [[0,0],[1,0]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
        app.session.execute("block", &json!({"name":"R","base":[0,0],"handles":[l],"keep":"delete"})).unwrap();
        // Arm repeat.
        let mut st = State::default();
        st.repeat = true;
        st.scale_specify_on_screen = false;
        st.uniform_scale = true;
        st.scale_uniform = 1.0;
        st.rotation_specify_on_screen = false;
        st.rotation_deg = 0.0;
        // Record into app state so repeat_tick can see it.
        app.side_palettes.blocks = st.clone();
        start_insert_with_presets(&mut app, "R", None, false, &st);
        let mut st2 = app.side_palettes.blocks.clone();
        st2.repeat_armed = Some(("R".into(), None, false));
        st2.repeat_after_rev = app.session.state().map(|s| s.revision).unwrap_or(0);
        app.side_palettes.blocks = st2;
        // Place once.
        let _ = app.session.input(Input::Point(Vec2::new(1.0, 2.0)));
        assert!(app.session.running.is_none());
        // The tick should start INSERT again.
        repeat_tick(&mut app);
        assert!(app.session.running.is_some(), "repeat did not re-arm INSERT");
    }
}
