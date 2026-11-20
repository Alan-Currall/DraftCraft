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
    // Keep the command line (no dialog) so HATCH runs interactively.
    app.run("setvar", json!({ "name": "HPDLGMODE", "value": 0 })).unwrap();
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
    app.run("setvar", json!({ "name": "HPDLGMODE", "value": 0 })).unwrap();
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

