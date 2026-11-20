//! STRETCH behaviour aligned with AutoCAD LT for arcs, hatch boundaries, and leaders.

use draftcraft_engine::Session;
use draftcraft_engine::doc::{EntityKind, Handle};
use draftcraft_engine::geom::Vec2;
use serde_json::json;

fn h(v: &serde_json::Value) -> Handle {
    let hex = v["handle"].as_str().or_else(|| v["handles"].get(0).and_then(serde_json::Value::as_str)).unwrap();
    Handle::parse_hex(hex).unwrap()
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-6 * (1.0 + b.len())
}

#[test]
fn arc_endpoint_stretch_keeps_sagitta() {
    let mut s = Session::new();
    // Arc from (10,0) to (0,10), quarter circle centered at origin.
    let a = h(&s.execute("arc", &json!({"center":[0,0], "radius": 10.0, "start": 0.0, "end": 90.0})).unwrap());
    // Crossing window around start point (10,0); move +5 in X.
    let r = s.execute("stretch", &json!({ "window": [[12, 1],[8,-1]], "delta": [5, 0] })).unwrap();
    assert_eq!(r["stretched"], 1);
    let d = s.doc().unwrap();
    if let EntityKind::Arc(x) = &d.entity(a).unwrap().kind {
        let c = x.center.xy();
        let sp = c + Vec2::new(x.start.cos(), x.start.sin()) * x.radius;
        let ep = c + Vec2::new(x.end.cos(), x.end.sin()) * x.radius;
        assert!(near(sp, Vec2::new(15.0, 0.0)), "moved start: {sp:?}");
        assert!(near(ep, Vec2::new(0.0, 10.0)), "fixed end: {ep:?}");
        let sweep = (x.end - x.start).rem_euclid(std::f64::consts::TAU);
        let h_now = x.radius * (1.0 - (sweep / 2.0).cos());
        let h_was = 10.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
        assert!((h_now - h_was).abs() < 1e-6, "sagitta preserved: now={h_now} was={h_was}");
    } else {
        panic!("not an arc");
    }
}

#[test]
fn hatch_boundary_vertices_move_inside_window() {
    let mut s = Session::new();
    // Rectangle [0,0]-[10,10] and a hatch picked inside.
    s.execute("rectang", &json!({"p1":[0,0], "p2":[10,10]})).unwrap();
    s.execute("hatch", &json!({"points":[[5,5]], "pattern":"ANSI31"})).unwrap();
    let hdl = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).unwrap().handle;
    // Crossing window over the bottom-left corner; move +2 in X.
    s.execute("stretch", &json!({ "window": [[1,1],[-1,-1]], "delta": [2, 0] })).unwrap();
    let d = s.doc().unwrap();
    if let EntityKind::Hatch(h) = &d.entity(hdl).unwrap().kind {
        let mut min_x = f64::INFINITY;
        for lp in &h.loops {
            for v in &lp.vertices {
                min_x = min_x.min(v.p.x);
            }
        }
        assert!(min_x >= 2.0 - 1e-9, "left boundary moved: min_x={min_x}");
    } else {
        panic!("not a hatch");
    }
}

#[test]
fn leader_and_mleader_points_stretch() {
    let mut s = Session::new();
    // Leader from (0,0) to (10,0).
    s.execute("leader", &json!({"points": [[0,0],[10,0]]})).unwrap();
    let ld = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Leader(_))).unwrap().handle;
    s.execute("stretch", &json!({ "window": [[12,1],[8,-1]], "delta": [5, 0] })).unwrap();
    let d = s.doc().unwrap();
    if let EntityKind::Leader(l) = &d.entity(ld).unwrap().kind {
        let a = l.vertices.first().unwrap().xy();
        let b = l.vertices.last().unwrap().xy();
        assert!(near(a, Vec2::new(0.0, 0.0)) && near(b, Vec2::new(15.0, 0.0)), "{a:?} {b:?}");
    } else {
        panic!("not a leader");
    }

    // Multileader from (0,0) to (10,0), with text.
    s.execute("mleader", &json!({"points": [[0,0],[10,0]], "text": "X"})).unwrap();
    let ml = s.doc().unwrap().model.iter().rev().find(|e| matches!(e.kind, EntityKind::MLeader(_))).unwrap().handle;
    s.execute("stretch", &json!({ "window": [[12,1],[8,-1]], "delta": [5, 0] })).unwrap();
    let d = s.doc().unwrap();
    if let EntityKind::MLeader(m) = &d.entity(ml).unwrap().kind {
        assert!(near(m.landing.xy(), Vec2::new(15.0, 0.0)), "landing moved: {:?}", m.landing.xy());
        let last = m.leaders.first().and_then(|l| l.last()).unwrap().xy();
        assert!(near(last, Vec2::new(15.0, 0.0)), "last leader vertex moved: {last:?}");
    } else {
        panic!("not a multileader");
    }
}

