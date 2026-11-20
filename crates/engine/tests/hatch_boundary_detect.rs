//! HATCH pick tolerance: search near the picked point first to avoid the global segment cap on
//! large drawings. A far-away mass of segments must not prevent picking a nearby simple region.

use draftcraft_engine::Session;
use draftcraft_engine::doc::geom::{PolyVertex, Vec2, Vec3};
use draftcraft_engine::doc::{Common, EntityKind, LwPolyline, Space};
use serde_json::json;

fn add_far_large_polyline(s: &mut Session, origin: (f64, f64), segments: usize) {
    // One very long polyline far from the origin, with `segments` segments (segments = vertices - 1).
    // This would trip region::MAX_SEGMENTS if the whole drawing were considered at once.
    let mut vertices: Vec<PolyVertex> = Vec::with_capacity(segments + 1);
    let (ox, oy) = origin;
    for i in 0..=segments {
        vertices.push(PolyVertex::new(Vec2::new(ox + i as f64, oy)));
    }
    let poly = LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false };
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::LwPolyline(poly)).unwrap();
}

#[test]
fn hatch_pick_ignores_far_mass_of_segments() {
    let mut s = Session::new();
    // A small rectangle near the origin: the region to hatch.
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    // Far away, a single polyline with more than MAX_SEGMENTS segments.
    // Region tracing over the whole drawing would refuse this input; the near-first search avoids it.
    add_far_large_polyline(&mut s, (100_000.0, 100_000.0), draftcraft_geom::region::MAX_SEGMENTS + 1000);
    // Pick inside the nearby rectangle.
    let r = s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap();
    // A hatch was created; the far-away geometry did not block the pick.
    let handle = r["handle"].as_str().expect("a created hatch handle");
    let d = s.doc().unwrap();
    assert!(d.model.iter().any(|e| e.handle.hex() == handle && matches!(e.kind, EntityKind::Hatch(_))));
}

#[test]
fn hatch_pick_still_works_on_small_drawings() {
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [20, 12] })).unwrap();
    // No far-away geometry: the ordinary small case still works.
    let r = s.execute("hatch", &json!({ "points": [[10, 6]], "pattern": "ANSI31" })).unwrap();
    let handle = r["handle"].as_str().expect("a created hatch handle");
    let d = s.doc().unwrap();
    assert!(d.model.iter().any(|e| e.handle.hex() == handle && matches!(e.kind, EntityKind::Hatch(_))));
}
