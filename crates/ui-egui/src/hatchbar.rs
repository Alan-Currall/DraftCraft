//! Hatch Creation bar: a compact strip over the drawing while HATCH or GRADIENT picks
//! boundaries (like AutoCAD LT's Hatch Creation tab).
//! It updates the drawing's HP* variables (pattern/scale/angle, associative, separate,
//! island detection, origin mode) and current colour for new objects (CECOLOR),
//! so the interactive hatch preview reflects choices and the created hatch uses them.
//! The bar disappears when the command ends.

use draftcraft_engine::Input;
use egui::{Color32, Sense, Stroke, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// Basic colour choices offered for hatch colour (set through CECOLOR).
pub const COLORS: [(&str, &str); 10] = [
    (".", "Use Current"),
    ("ByLayer", "ByLayer"),
    ("ByBlock", "ByBlock"),
    ("1", "Red"),
    ("2", "Yellow"),
    ("3", "Green"),
    ("4", "Cyan"),
    ("5", "Blue"),
    ("6", "Magenta"),
    ("7", "White"),
];

/// Island detection styles (HPISLANDDETECTION).
pub const ISLANDS: [&str; 3] = ["Normal", "Outer", "Ignore"];
/// Origin modes in HPORIGINMODE order (0=current, 1..5 as listed).
pub const ORIGINS: [&str; 6] = ["Use Current", "Bottom Left", "Bottom Right", "Top Right", "Top Left", "Center"];

/// Whether the bar should be visible now: HATCH or GRADIENT is running at its boundary prompt.
pub fn shown(app: &CadApp) -> bool {
    // Only while the interactive command is asking to pick points or select objects.
    is_hatch_like_running(app) && boundary_prompt(app).is_some()
}

fn is_hatch_like_running(app: &CadApp) -> bool {
    app.session.running.as_ref().is_some_and(|r| matches!(r.id.as_str(), "hatch" | "gradient"))
}

/// Boundary prompt mode: Some(false)=Pick internal point, Some(true)=Select objects, None otherwise.
pub fn boundary_prompt(app: &CadApp) -> Option<bool> {
    let p = app.session.current_prompt()?;
    match p.message.as_str() {
        "Pick internal point" => Some(false),
        // HATCH's object mode exposes a keyword to switch back to point-picking
        "Select objects" if p.keywords.iter().any(|k| k == "picK internal point") => Some(true),
        _ => None,
    }
}

/// The values shown by the bar, pulled from the drawing header.
#[derive(Clone, Debug, PartialEq)]
pub struct Values {
    pub pattern: String,
    pub color: String,
    pub angle_deg: f64,
    pub scale: f64,
    pub associative: bool,
    pub separate: bool,
    pub island: i64,
    pub origin: i64,
}

pub fn values(app: &CadApp) -> Values {
    let Ok(d) = app.session.doc() else {
        return Values {
            pattern: "ANSI31".into(),
            color: ".".into(),
            angle_deg: 0.0,
            scale: 1.0,
            associative: true,
            separate: false,
            island: 0,
            origin: 0,
        };
    };
    let h = &d.header;
    // Present CECOLOR as a friendly token when possible
    let color = match h.get("CECOLOR") {
        Some(draftcraft_engine::doc::HVal::Int(256)) => "ByLayer".to_string(),
        Some(draftcraft_engine::doc::HVal::Int(0)) => "ByBlock".to_string(),
        Some(draftcraft_engine::doc::HVal::Int(i)) => i.to_string(),
        // True colour kept in CECOLOR_RGB is shown as "Use Current" here
        _ => ".".into(),
    };
    Values {
        pattern: h.str("HPNAME", "ANSI31"),
        color,
        angle_deg: h.f64("HPANG", 0.0).to_degrees(),
        scale: h.f64("HPSCALE", 1.0),
        associative: h.i64("HPASSOC", 1) != 0,
        separate: h.i64("HPSEPARATE", 0) != 0,
        island: h.i64("HPISLANDDETECTION", 0),
        origin: h.i64("HPORIGINMODE", 0),
    }
}

/// A label for a colour value.
pub fn color_label(v: &str) -> String {
    COLORS.iter().find(|(k, _)| k.eq_ignore_ascii_case(v)).map_or_else(|| v.to_string(), |(_, n)| (*n).to_string())
}

/// Apply one change. Errors are echoed on the command line.
pub fn set(app: &mut CadApp, name: &str, value: Value) {
    if let Err(e) = app.session.execute("setvar", &json!({ "name": name, "value": value })) {
        app.session.echo(e.to_string());
    }
}

/// What the bar asked for this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Set one header/system variable (e.g. HPNAME, HPSCALE, HPANG, HPASSOC, …).
    Set(&'static str, Value),
    /// Switch boundary mode: true=Select objects, false=Pick internal point.
    Mode(bool),
    /// Finish (Enter).
    Done,
    /// Cancel (Esc).
    Cancel,
    /// Open the full Hatch/Gradient dialog; the running command is cancelled first.
    Dialog,
}

pub fn apply(app: &mut CadApp, a: Action) {
    match a {
        Action::Set("HPCOLOR", v) => {
            // Map HPCOLOR to CECOLOR in this implementation.
            set(app, "CECOLOR", v);
        }
        Action::Set(n, v) => set(app, n, v),
        Action::Mode(selecting) => {
            if boundary_prompt(app) != Some(selecting) {
                let k = if selecting { "Select objects" } else { "picK internal point" };
                if let Err(e) = app.session.input(Input::Keyword(k.into())) {
                    app.session.echo(e.to_string());
                }
            }
        }
        Action::Done => {
            app.cmd.buffer.clear();
            if let Err(e) = app.session.input(Input::Enter) {
                app.session.echo(e.to_string());
            }
        }
        Action::Cancel => app.session.cancel(),
        Action::Dialog => {
            let g = app.session.running.as_ref().is_some_and(|r| r.id == "gradient");
            app.session.cancel();
            crate::hatchdlg::open_create(app, g);
        }
    }
}

/// A numeric text field committing on Enter or focus loss; returns the parsed value when changed.
fn num_field(ui: &mut egui::Ui, id: &str, value: f64, width: f32) -> Option<f64> {
    let key = egui::Id::unique(("hatchbar_field", id));
    let mut text: String = ui.data_mut(|d| d.get_temp(key)).unwrap_or_else(|| fmt(value));
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(width).id(key.with("edit")));
    let mut out = None;
    if r.has_focus() || r.lost_focus() {
        ui.data_mut(|d| d.insert_temp(key, text.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(key));
        out = text.trim().parse::<f64>().ok().filter(|v| v.is_finite() && (v - value).abs() > 1e-12);
    } else if !r.has_focus() {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    out
}

pub fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

fn swatch(ui: &mut egui::Ui, c: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    ui.painter().rect_filled(r, 2.0, c);
    ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside);
}

fn color32(app: &CadApp, v: &str) -> Color32 {
    use draftcraft_color::Color;
    let Ok(d) = app.session.doc() else { return Color32::GRAY };
    let layer = d.header.str("CLAYER", "0");
    let chosen = match v {
        "." => Color::from_aci(d.header.i64("CECOLOR", 256) as i16),
        _ => Color::parse(v).or_else(|| v.parse::<i16>().ok().map(Color::from_aci)).unwrap_or(Color::ByLayer),
    };
    let layer_color = d.layer(&layer).map(|l| l.color).unwrap_or(Color::Index(7));
    let rgb = chosen.resolve(layer_color, Color::Index(7));
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

/// Draw the overlay anchored to the top-center of `area`. A no-op when it shouldn't show now.
pub fn show(app: &mut CadApp, ctx: &egui::Context, area: egui::Rect) {
    if !shown(app) {
        return;
    }
    let t = Tokens::get();
    let v = values(app);
    let selecting = boundary_prompt(app) == Some(true);
    let grad = app.session.running.as_ref().is_some_and(|r| r.id == "gradient");
    let mut actions = Vec::new();
    egui::Area::new(egui::Id::unique("hatch_bar"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(egui::pos2(area.center().x, area.min.y + 8.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(t.panel)
                .stroke(Stroke::new(1.0, t.border))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .shadow(egui::epaint::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(60) })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        // Boundary mode
                        if ui
                            .selectable_label(!selecting, crate::tl!("Pick Points"))
                            .on_hover_text(crate::tl!("Pick an internal point of each area"))
                            .clicked()
                        {
                            actions.push(Action::Mode(false));
                        }
                        if ui
                            .selectable_label(selecting, crate::tl!("Select"))
                            .on_hover_text(crate::tl!("Select boundary objects"))
                            .clicked()
                        {
                            actions.push(Action::Mode(true));
                        }
                        ui.separator();
                        if !grad {
                            // Pattern
                            let resp = ui.button(format!("▦ {} ▾", v.pattern)).on_hover_text(crate::tl!("Hatch pattern"));
                            egui::Popup::from_toggle_button_response(&resp)
                                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                .show(|ui| {
                                    let fid = egui::Id::unique("hatch_bar_filter");
                                    let mut filter: String = ui.data_mut(|d| d.get_temp(fid)).unwrap_or_default();
                                    ui.add(egui::TextEdit::singleline(&mut filter).hint_text(crate::tl!("Search for pattern")).desired_width(6.0 * 62.0));
                                    ui.data_mut(|d| d.insert_temp(fid, filter.clone()));
                                    ui.set_width(6.0 * 62.0);
                                    let chosen =
                                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| crate::palettes::patterns::grid(ui, &v.pattern, &filter)).inner;
                                    if let Some(n) = chosen {
                                        actions.push(Action::Set("HPNAME", json!(n)));
                                        ui.close();
                                    }
                                });
                            // Colour (maps to CECOLOR)
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                swatch(ui, color32(app, &v.color));
                                ui.menu_button(format!("{} ▾", crate::i18n::t(&color_label(&v.color))), |ui| {
                                    for (k, n) in COLORS {
                                        ui.horizontal(|ui| {
                                            swatch(ui, color32(app, k));
                                            if ui.selectable_label(v.color.eq_ignore_ascii_case(k), crate::i18n::t(n)).clicked() {
                                                actions.push(Action::Set("HPCOLOR", json!(k)));
                                                ui.close();
                                            }
                                        });
                                    }
                                })
                                .response
                                .on_hover_text(crate::tl!("Hatch colour"));
                            });
                            // Angle / Scale
                            ui.label(egui::RichText::new(crate::tl!("Angle")).color(t.text_dim));
                            if let Some(a) = num_field(ui, "angle", v.angle_deg, 44.0) {
                                actions.push(Action::Set("HPANG", json!(a)));
                            }
                            ui.label(egui::RichText::new(crate::tl!("Scale")).color(t.text_dim));
                            if let Some(s) = num_field(ui, "scale", v.scale, 52.0).filter(|s| *s > 0.0) {
                                actions.push(Action::Set("HPSCALE", json!(s)));
                            }
                            ui.separator();
                        }
                        // Advanced options popup
                        let resp = ui.button("⚙").on_hover_text(crate::tl!("Hatch options"));
                        egui::Popup::from_toggle_button_response(&resp)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_min_width(200.0);
                                let mut assoc = v.associative;
                                if ui.checkbox(&mut assoc, crate::tl!("Associative")).changed() {
                                    actions.push(Action::Set("HPASSOC", json!(i64::from(assoc))));
                                }
                                let mut sep = v.separate;
                                if ui.checkbox(&mut sep, crate::tl!("Create separate hatches")).changed() {
                                    actions.push(Action::Set("HPSEPARATE", json!(i64::from(sep))));
                                }
                                ui.separator();
                                ui.label(egui::RichText::new(crate::tl!("Island detection")).color(t.text_dim));
                                for (i, n) in ISLANDS.iter().enumerate() {
                                    if ui.radio(v.island == i as i64, crate::i18n::t(n)).clicked() {
                                        actions.push(Action::Set("HPISLANDDETECTION", json!(i)));
                                    }
                                }
                                ui.separator();
                                ui.label(egui::RichText::new(crate::tl!("Origin")).color(t.text_dim));
                                for (i, n) in ORIGINS.iter().enumerate() {
                                    if ui.radio(v.origin == i as i64, crate::i18n::t(n)).clicked() {
                                        actions.push(Action::Set("HPORIGINMODE", json!(i)));
                                    }
                                }
                                ui.separator();
                                if ui.button(crate::tl!("Hatch and Gradient dialog…")).clicked() {
                                    actions.push(Action::Dialog);
                                    ui.close();
                                }
                            });
                        ui.separator();
                        let done =
                            ui.add(egui::Button::new(egui::RichText::new("✓").strong().color(Color32::WHITE)).fill(t.accent)).on_hover_text(crate::tl!(
                                "Close Hatch Creation (Enter)"
                            ));
                        if done.clicked() {
                            actions.push(Action::Done);
                        }
                        if ui.button("✕").on_hover_text(crate::tl!("Cancel (Esc)")).clicked() {
                            actions.push(Action::Cancel);
                        }
                    });
                });
        });
    for a in actions {
        apply(app, a);
    }
}

