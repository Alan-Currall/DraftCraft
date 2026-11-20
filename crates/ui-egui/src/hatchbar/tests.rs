use draftcraft_engine::{Input, Session};
use draftcraft_engine::doc::{Drawing, EntityKind};
use draftcraft_engine::geom::Vec2;
use serde_json::json;

use crate::{CadApp, Services};

fn app_with_rect() -> CadApp {
    let mut s = Session::new();
    s.open_drawing(Drawing::new_metric(), "Hatch Bar", None);
    // A simple rectangle to hatch
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [100, 50] })).unwrap();
    CadApp::new(s, Services::default())
}

#[test]
fn bar_is_shown_during_interactive_hatch_and_goes_away() {
    let mut app = app_with_rect();
    // Start HATCH and assert the bar shows.
    app.start("hatch");
    assert!(crate::hatchbar::shown(&app));
    // Pick a point, then finish from the bar.
    app.session.input(Input::Point(Vec2::new(50.0, 25.0))).unwrap();
    crate::hatchbar::apply(&mut app, crate::hatchbar::Action::Done);
    assert!(app.session.running.is_none(), "command finished");
    // One hatch was created.
    let d = app.session.doc().unwrap();
    assert!(d.model.iter().any(|e| matches!(e.kind, EntityKind::Hatch(_))));
    // After the command ends, the bar is gone.
    assert!(!crate::hatchbar::shown(&app));
}

#[test]
fn bar_changes_flow_into_created_hatch() {
    let mut app = app_with_rect();
    app.start("hatch");
    // Set pattern, scale and angle from the bar (driven through HP* vars).
    crate::hatchbar::apply(&mut app, crate::hatchbar::Action::Set("HPNAME", json!("AR-CONC")));
    crate::hatchbar::apply(&mut app, crate::hatchbar::Action::Set("HPSCALE", json!(0.5)));
    crate::hatchbar::apply(&mut app, crate::hatchbar::Action::Set("HPANG", json!(30.0)));
    // Pick and finish.
    app.session.input(Input::Point(Vec2::new(50.0, 25.0))).unwrap();
    crate::hatchbar::apply(&mut app, crate::hatchbar::Action::Done);
    let d = app.session.doc().unwrap();
    let h = d.model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).unwrap();
    assert_eq!(h.pattern, "AR-CONC");
    assert!((h.scale - 0.5).abs() < 1e-9);
    assert!((h.angle - 30f64.to_radians()).abs() < 1e-9);
}

#[test]
fn transparency_annotative_and_background_flow_into_created_hatch() {
    let mut app = app_with_rect();
    app.start("hatch");
    // Transparency 40%, annotative on, background Blue (5)
    cadcraft_ui_egui::hatchbar::apply(app.state_mut(), cadcraft_ui_egui::hatchbar::Action::Set("CETRANSPARENCY", json!(40)));
    cadcraft_ui_egui::hatchbar::apply(app.state_mut(), cadcraft_ui_egui::hatchbar::Action::Set("HPANNOTATIVE", json!(1)));
    cadcraft_ui_egui::hatchbar::apply(app.state_mut(), cadcraft_ui_egui::hatchbar::Action::Set("HPBACKGROUNDCOLOR", json!(5)));
    // Pick and finish.
    app.session.input(Input::Point(Vec2::new(50.0, 25.0))).unwrap();
    cadcraft_ui_egui::hatchbar::apply(app.state_mut(), cadcraft_ui_egui::hatchbar::Action::Done);
    let d = app.session.doc().unwrap();
    let e = d.model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).unwrap();
    // Transparency in common
    assert_eq!(e.common.transparency, draftcraft_doc::Transparency::Percent(40));
    // Annotative: new hatch supports current scale
    assert!(!e.common.anno_scales.is_empty(), "annotative hatch supports a scale");
    // Background colour index 5
    if let EntityKind::Hatch(h) = &e.kind {
        assert_eq!(h.background, Some(draftcraft_color::Color::Index(5)));
    } else {
        panic!("expected hatch");
    }
}

