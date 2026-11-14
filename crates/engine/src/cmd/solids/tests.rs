use draftcraft_doc::{EntityKind, Solid3d};
use draftcraft_geom::Vec3;
use serde_json::{Value, json};

use crate::Session;

fn solid(s: &Session, h: &str) -> Solid3d {
    let d = s.doc().unwrap();
    let e = d.model.iter().find(|e| e.handle.hex() == h).unwrap_or_else(|| panic!("no entity {h}"));
    match &e.kind {
        EntityKind::Solid3d(so) => so.clone(),
        other => panic!("not a solid: {other:?}"),
    }
}

fn h(r: &Value) -> String {
    r["handle"].as_str().unwrap().to_string()
}

fn count(s: &Session, name: &str) -> usize {
    s.doc().unwrap().model.iter().filter(|e| e.kind.type_name() == name).count()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

#[test]
fn primitives_by_json() {
    let mut s = Session::new();
    let b = s.execute("box", &json!({"corner1": [10, 10], "corner2": [12, 13], "height": 4})).unwrap();
    assert!(near(solid(&s, &h(&b)).volume, 24.0, 1e-9));
    let c = s.execute("cylinder", &json!({"center": [0, 0], "radius": 1, "height": 2})).unwrap();
    assert!(near(solid(&s, &h(&c)).volume, std::f64::consts::TAU, 1e-3));
    let sp = s.execute("sphere", &json!({"center": [5, 5, 5], "radius": 1})).unwrap();
    assert!(near(solid(&s, &h(&sp)).volume, 4.0 / 3.0 * std::f64::consts::PI, 1e-3));
    let w = s.execute("wedge", &json!({"corner1": [0, 0], "corner2": [2, 3], "height": 4})).unwrap();
    assert!(near(solid(&s, &h(&w)).volume, 12.0, 1e-9));
    let t = s.execute("torus", &json!({"center": [0, 0], "radius": 3, "tubeRadius": 1})).unwrap();
    assert!(near(solid(&s, &h(&t)).volume, 2.0 * std::f64::consts::PI.powi(2) * 3.0, 1e-3));
    let k = s.execute("cone", &json!({"center": [0, 0], "radius": 1, "height": 3})).unwrap();
    assert!(near(solid(&s, &h(&k)).volume, std::f64::consts::PI, 1e-3));
    // The box sits where it was drawn: its plan edges span 10..12 x 10..13.
    let so = solid(&s, &h(&b));
    let xs: Vec<f64> = so.plan_segments().iter().flat_map(|(a, b)| [a.x, b.x]).collect();
    assert!(xs.iter().all(|x| (9.999..=12.001).contains(x)), "{xs:?}");
}

#[test]
fn bad_primitive_input_is_an_error_not_a_crash() {
    let mut s = Session::new();
    for (id, p) in [
        ("box", json!({"corner1": [0, 0], "corner2": [0, 0], "height": 1})),
        ("box", json!({"corner1": [0, 0], "corner2": [1, 1], "height": f64::MAX})),
        ("cylinder", json!({"center": [0, 0], "radius": -1, "height": 2})),
        ("sphere", json!({"center": [0, 0], "radius": 0})),
        ("torus", json!({"center": [0, 0], "radius": 1, "tubeRadius": 2})),
        ("sphere", json!({"center": ["a", 0], "radius": 1})),
    ] {
        assert!(s.execute(id, &p).is_err(), "{id} {p}");
    }
    assert_eq!(count(&s, "3D Solid"), 0);
}

#[test]
fn box_at_the_command_line() {
    let mut s = Session::new();
    s.cmdline("box 0,0 2,3 4").unwrap();
    let d = s.doc().unwrap();
    let so = match &d.model.last().unwrap().kind {
        EntityKind::Solid3d(so) => so.clone(),
        other => panic!("{other:?}"),
    };
    assert!(near(so.volume, 24.0, 1e-9));
}

#[test]
fn extrude_and_revolve_profiles() {
    let mut s = Session::new();
    let pl = s.execute("rectang", &json!({"p1": [0, 0], "p2": [4, 2]})).unwrap();
    let r = s.execute("extrude", &json!({"handles": [h(&pl)], "height": 3})).unwrap();
    let hs = r["handles"].as_array().unwrap();
    assert_eq!(hs.len(), 1);
    assert!(near(solid(&s, hs[0].as_str().unwrap()).volume, 24.0, 1e-9));
    // The profile is gone (DELOBJ).
    assert_eq!(count(&s, "Polyline"), 0);

    let c = s.execute("circle", &json!({"center": [10, 0], "radius": 1})).unwrap();
    let r = s.execute("extrude", &json!({"handles": [h(&c)], "height": 2, "keepProfiles": true})).unwrap();
    let so = solid(&s, r["handles"][0].as_str().unwrap());
    assert!(near(so.volume, std::f64::consts::TAU, 1e-3));
    assert_eq!(count(&s, "Circle"), 1);

    // A unit square 2 from the Y axis, turned about it: a ring of volume 2π · 2.5 · 1.
    let sq = s.execute("rectang", &json!({"p1": [2, 0], "p2": [3, 1]})).unwrap();
    let r = s.execute("revolve", &json!({"handles": [h(&sq)], "axisStart": [0, 0], "axisEnd": [0, 1]})).unwrap();
    let so = solid(&s, r["handles"][0].as_str().unwrap());
    assert!(near(so.volume, std::f64::consts::TAU * 2.5, 1e-3), "{}", so.volume);

    // Open curves aren't profiles.
    s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap();
    let l = s.doc().unwrap().model.last().unwrap().handle.hex();
    assert!(s.execute("extrude", &json!({"handles": [l], "height": 1})).is_err());
}

#[test]
fn booleans_through_the_region_commands() {
    let mut s = Session::new();
    let a = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [10, 10], "height": 10})).unwrap());
    let b = h(&s.execute("box", &json!({"corner1": [5, 5], "corner2": [15, 15], "height": 10})).unwrap());
    let r = s.execute("union", &json!({"handles": [a, b]})).unwrap();
    assert_eq!(h(&r), a);
    assert!(near(solid(&s, &a).volume, 1750.0, 1e-9));
    assert_eq!(count(&s, "3D Solid"), 1);

    let c = h(&s.execute("cylinder", &json!({"center": [2, 2], "radius": 1, "height": 10})).unwrap());
    s.execute("subtract", &json!({"from": [a], "remove": [c]})).unwrap();
    assert!(near(solid(&s, &a).volume, 1750.0 - 10.0 * std::f64::consts::PI, 1e-3));

    let d = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [1, 1], "height": 1})).unwrap());
    let e = h(&s.execute("box", &json!({"corner1": [5, 5], "corner2": [6, 6], "height": 1})).unwrap());
    let r = s.execute("intersect", &json!({"handles": [d, e]})).unwrap();
    assert!(r["handle"].is_null());
    assert!(s.doc().unwrap().model.iter().all(|x| x.handle.hex() != d && x.handle.hex() != e));
}

#[test]
fn moved_solids_still_combine() {
    let mut s = Session::new();
    let a = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [2, 2], "height": 2})).unwrap());
    let b = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [2, 2], "height": 2})).unwrap());
    s.execute("move", &json!({"handles": [b], "from": [0, 0], "to": [1, 1]})).unwrap();
    s.execute("rotate", &json!({"handles": [a], "base": [1, 1], "angle": 90})).unwrap();
    s.execute("intersect", &json!({"handles": [a, b]})).unwrap();
    assert!(near(solid(&s, &a).volume, 2.0, 1e-9), "{}", solid(&s, &a).volume);
}

#[test]
fn massprop_of_a_solid() {
    let mut s = Session::new();
    let a = h(&s.execute("box", &json!({"corner1": [1, 2], "corner2": [3, 6], "height": 1})).unwrap());
    let r = s.execute("massprop", &json!({"handles": [a]})).unwrap();
    let m = &r["solids"][0];
    assert!(near(m["volume"].as_f64().unwrap(), 8.0, 1e-9));
    assert!(near(m["centroid"][0].as_f64().unwrap(), 2.0, 1e-9));
    assert!(near(m["centroid"][1].as_f64().unwrap(), 4.0, 1e-9));
    assert!(near(m["centroid"][2].as_f64().unwrap(), 0.5, 1e-9));
    assert!(r["message"].as_str().unwrap().contains("SOLIDS"));
}

#[test]
fn slice_keeps_both_or_one_side() {
    let mut s = Session::new();
    let a = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [10, 2], "height": 2})).unwrap());
    let r = s.execute("slice", &json!({"handles": [a], "origin": [4, 0, 0], "normal": [1, 0, 0]})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 2);
    let total: f64 = r["handles"].as_array().unwrap().iter().map(|x| solid(&s, x.as_str().unwrap()).volume).sum();
    assert!(near(total, 40.0, 1e-9));
    let b = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [10, 2], "height": 2})).unwrap());
    s.execute("slice", &json!({"handles": [b], "points": [[4, -1], [4, 5]], "keep": [9, 1, 1]})).unwrap();
    assert!(near(solid(&s, &b).volume, 24.0, 1e-9));
}

#[test]
fn fillet_chamfer_and_shell() {
    let mut s = Session::new();
    let a = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [10, 10], "height": 10})).unwrap());
    s.execute("filletedge", &json!({"handle": a, "edges": [[10, 5, 10]], "radius": 1})).unwrap();
    let v = solid(&s, &a).volume;
    assert!(near(v, 1000.0 - (1.0 - std::f64::consts::PI / 4.0) * 10.0, 1e-3), "{v}");
    let b = h(&s.execute("box", &json!({"corner1": [20, 0], "corner2": [30, 10], "height": 10})).unwrap());
    // An edge picked in plan: the nearest edge seen from above.
    s.execute("chamferedge", &json!({"handle": b, "edges": [[30, 5, 10]], "distance": 1})).unwrap();
    assert!(near(solid(&s, &b).volume, 995.0, 1e-6));
    let c = h(&s.execute("box", &json!({"corner1": [40, 0], "corner2": [50, 10], "height": 10})).unwrap());
    s.execute("solidedit", &json!({"handle": c, "operation": "shell", "thickness": 1, "remove": [[45, 5]]})).unwrap();
    // Open at the top: 1000 − 8·8·9.
    assert!(near(solid(&s, &c).volume, 1000.0 - 576.0, 1e-6), "{}", solid(&s, &c).volume);
}

#[test]
fn interfere_reports_and_creates() {
    let mut s = Session::new();
    let a = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [2, 2], "height": 2})).unwrap());
    let b = h(&s.execute("box", &json!({"corner1": [1, 1], "corner2": [3, 3], "height": 2})).unwrap());
    let r = s.execute("interfere", &json!({"first": [a, b]})).unwrap();
    assert_eq!(r["pairs"].as_array().unwrap().len(), 1);
    assert!(near(r["pairs"][0]["volume"].as_f64().unwrap(), 2.0, 1e-9));
    assert_eq!(count(&s, "3D Solid"), 2);
    s.execute("interfere", &json!({"first": [a], "second": [b], "create": true})).unwrap();
    assert_eq!(count(&s, "3D Solid"), 3);
}

#[test]
fn flatshot_draws_lines_and_hidden_lines() {
    let mut s = Session::new();
    s.execute("box", &json!({"corner1": [0, 0], "corner2": [4, 2], "height": 3})).unwrap();
    let r = s.execute("flatshot", &json!({"view": "front", "base": [100, 0]})).unwrap();
    let n = r["handles"].as_array().unwrap().len();
    assert_eq!(n, 4, "a box seen from the front is a rectangle");
    let r = s.execute("flatshot", &json!({"view": "top"})).unwrap();
    assert_eq!(r["handles"].as_array().unwrap().len(), 4);
    let hole = s.execute("cylinder", &json!({"center": [2, 1], "radius": 0.5, "height": 3})).unwrap();
    let r = s.execute("flatshot", &json!({"handles": [h(&hole)], "view": "front", "showHidden": true})).unwrap();
    assert!(!r["handles"].as_array().unwrap().is_empty());
}

#[test]
fn section_makes_regions() {
    let mut s = Session::new();
    let a = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [10, 4], "height": 6})).unwrap());
    let c = h(&s.execute("cylinder", &json!({"center": [5, 2], "radius": 1, "height": 6})).unwrap());
    s.execute("subtract", &json!({"from": [a], "remove": [c]})).unwrap();
    let r = s.execute("section", &json!({"handles": [a], "plane": "xy", "origin": [0, 0, 3]})).unwrap();
    let hs = r["handles"].as_array().unwrap();
    assert_eq!(hs.len(), 1);
    let m = s.execute("massprop", &json!({"handles": [hs[0]]})).unwrap();
    let area = m["regions"][0]["area"].as_f64().unwrap();
    assert!(near(area, 40.0 - std::f64::consts::PI, 1e-2), "{area}");
    // A vertical cut through the middle of the hole: two 4 x 6 strips... less the hole: 10x6 − 2x6.
    let r = s.execute("section", &json!({"handles": [a], "points": [[0, 2], [10, 2]], "base": [50, 0]})).unwrap();
    let total: f64 = r["handles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| s.execute("massprop", &json!({"handles": [x]})).unwrap()["regions"][0]["area"].as_f64().unwrap())
        .sum();
    assert!(near(total, 48.0, 1e-2), "{total}");
    // A plane that misses: an error.
    assert!(s.execute("section", &json!({"handles": [a], "plane": "xy", "origin": [0, 0, 30]})).is_err());
}

#[test]
fn move_rotate_mirror_scale_keep_the_body() {
    let mut s = Session::new();
    let a = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [1, 2], "height": 3})).unwrap());
    let before = solid(&s, &a).body.clone();
    s.execute("move", &json!({"handles": [a], "from": [0, 0], "to": [100, 50]})).unwrap();
    s.execute("mirror", &json!({"handles": [a], "p1": [0, 0], "p2": [0, 1], "erase": true})).unwrap();
    let after = solid(&s, &a);
    assert_eq!(after.body, before);
    assert!(near(after.volume, 6.0, 1e-9));
    let w = after.world(Vec3::new(0.5, 1.0, 0.0));
    assert!(near(w.x, -100.5, 1e-9) && near(w.y, 51.0, 1e-9), "{w:?}");
}

#[test]
fn an_opened_solid_gets_the_look_of_its_body() {
    let mut s = Session::new();
    let a = h(&s.execute("box", &json!({"corner1": [0, 0], "corner2": [1, 1], "height": 1})).unwrap());
    let mut d = s.doc().unwrap().clone();
    let good = solid(&s, &a).look.clone();
    let hd = d.model.iter().find(|e| e.handle.hex() == a).unwrap().handle;
    d.model.modify(hd, |e| {
        if let EntityKind::Solid3d(so) = &mut e.kind {
            so.look = std::sync::Arc::new(draftcraft_doc::SolidLook::default());
        }
    });
    super::refresh_looks(&mut d);
    let EntityKind::Solid3d(so) = &d.model.get(hd).unwrap().kind else { panic!() };
    assert_eq!(so.look, good);
    // A body that can't be read keeps what it has.
    d.model.modify(hd, |e| {
        if let EntityKind::Solid3d(so) = &mut e.kind {
            so.body = std::sync::Arc::new(vec![1, 2, 3]);
        }
    });
    super::refresh_looks(&mut d);
}
