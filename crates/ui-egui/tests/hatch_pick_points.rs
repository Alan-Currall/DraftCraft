//! HATCHPICK: internal points succeed on large drawings by searching near the point first.

use draftcraft_engine::Session;
use draftcraft_engine::doc::geom::{PolyVertex, Vec2};
use draftcraft_engine::doc::{Common, EntityKind, LwPolyline, Space};
use draftcraft_ui_egui::{CadApp, Services};
use serde_json::json;

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

fn add_far_large_polyline(app: &mut CadApp, origin: (f64, f64), segments: usize) {
    let mut vertices: Vec<PolyVertex> = Vec::with_capacity(segments + 1);
    let (ox, oy) = origin;
    for i in 0..=segments {
        vertices.push(PolyVertex::new(Vec2::new(ox + i as f64, oy)));
    }
    let poly = LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false };
    app.session.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::LwPolyline(poly)).unwrap();
}

#[test]
fn hatchpick_internal_point_succeeds_with_far_heavy_geometry() {
    let mut a = app();
    // Nearby closed boundary.
    a.session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    // Far-away many segments (beyond region::MAX_SEGMENTS if taken all at once).
    add_far_large_polyline(&mut a, (200_000.0, 200_000.0), draftcraft_geom::region::MAX_SEGMENTS + 500);
    // HATCHPICK validates that a closed boundary exists around the given point.
    let r = a.run("hatchpick", json!({ "points": [[5, 5]] })).unwrap();
    assert_eq!(r["points"], json!([[5.0, 5.0]]));
    assert!(r.get("handles").is_some());
}
