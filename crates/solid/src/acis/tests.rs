use draftcraft_geom::Vec3;

use super::*;
use crate::body::{BoolOp, boolean, box_solid, cone, cylinder, sphere, torus};

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-9)
}

/// Written as SAT and as SAB, each read back: the same volume and centroid.
fn round_trip(name: &str, body: &Body) {
    let mass = body.mass().expect("mass");
    let sat = write_sat(body).unwrap_or_else(|e| panic!("{name}: write SAT: {e}"));
    for (form, data) in [("SAT", sat.clone().into_bytes()), ("scrambled SAT", scramble(&sat).into_bytes()), ("SAB", write_sab(body).expect("SAB"))] {
        let back = read(&data).unwrap_or_else(|e| panic!("{name}: read {form}: {e}\n{sat}"));
        assert_eq!(back.len(), 1, "{name} {form}");
        let m = back[0].mass().expect("mass back");
        assert!(close(m.volume, mass.volume, 1e-5), "{name} {form}: volume {} → {}\n{sat}", mass.volume, m.volume);
        assert!((m.centroid - mass.centroid).len() <= 1e-5 * body.size().max(1.0), "{name} {form}: centroid {:?} → {:?}", mass.centroid, m.centroid);
        assert_eq!(back[0].face_count(), body.face_count(), "{name} {form}: faces\n{sat}");
    }
}

#[test]
fn primitives_round_trip_through_acis() {
    let z = Vec3::new(0.0, 0.0, 1.0);
    round_trip("box", &box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 60.0, 40.0)).expect("box"));
    round_trip("cylinder", &cylinder(Vec3::new(200.0, 0.0, 0.0), z, 25.0, 50.0).expect("cylinder"));
    round_trip("cone", &cone(Vec3::new(300.0, 0.0, 0.0), z, 30.0, 0.0, 60.0).expect("cone"));
    round_trip("frustum", &cone(Vec3::new(300.0, 0.0, 0.0), z, 30.0, 10.0, 60.0).expect("frustum"));
    round_trip("sphere", &sphere(Vec3::new(400.0, 0.0, 30.0), 30.0).expect("sphere"));
    round_trip("torus", &torus(Vec3::new(500.0, 0.0, 0.0), 40.0, 10.0).expect("torus"));
}

#[test]
fn a_box_with_a_hole_round_trips_through_acis() {
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 50.0)).expect("box");
    let c = cylinder(Vec3::new(50.0, 50.0, -10.0), Vec3::new(0.0, 0.0, 1.0), 20.0, 70.0).expect("cylinder");
    let holed = boolean(&b, &c, BoolOp::Subtract).expect("subtract").expect("a solid");
    round_trip("box with a hole", &holed);
}

#[test]
fn sat_text_looks_like_acis_700() {
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 3.0)).expect("box");
    let sat = write_sat(&b).expect("sat");
    let lines: Vec<&str> = sat.lines().collect();
    assert_eq!(lines.first().copied(), Some("700 0 1 0 "));
    assert!(lines.iter().any(|l| l.starts_with("body $-1 -1 $-1 $1 $-1 $-1 #")), "{sat}");
    assert_eq!(lines.iter().filter(|l| l.starts_with("face ")).count(), 6);
    assert_eq!(lines.iter().filter(|l| l.starts_with("plane-surface ")).count(), 6);
    assert_eq!(lines.iter().filter(|l| l.starts_with("edge ")).count(), 12);
    assert_eq!(lines.iter().filter(|l| l.starts_with("vertex ")).count(), 8);
    assert_eq!(lines.last().copied(), Some("End-of-ACIS-data "));
    assert_eq!(scramble(&scramble(&sat)), sat);
}

#[test]
fn hostile_acis_is_an_error_not_a_panic() {
    for data in
        [&b""[..], b"700 0 1 0", b"ACIS BinaryFile", b"garbage\n#\n", b"700 0 1 0 \n4 x 4 y 4 z \n1 1e-06 1e-10 \nbody $5 -1 $-1 $9 $-1 $-1 #\n"]
    {
        let _ = read(data);
    }
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 3.0)).expect("box");
    let sab = write_sab(&b).expect("sab");
    for cut in [1, 10, 40, sab.len() / 2, sab.len() - 1] {
        let _ = read(&sab[..cut]);
    }
    let sat = write_sat(&b).expect("sat");
    for cut in [5, 30, sat.len() / 3, sat.len() / 2] {
        let _ = read(&sat.as_bytes()[..cut]);
    }
}

/// A cone's apex is a loop of its own, as AutoCAD writes it: one coedge on an edge with no curve
/// that starts and ends at the apex vertex. Without it AutoCAD reports the cone as an invalid
/// solid of volume 0 (seen in AutoCAD 2027: SOLIDEDIT Check, MASSPROP).
#[test]
fn a_cone_apex_is_a_point_edge_loop() {
    let z = Vec3::new(0.0, 0.0, 1.0);
    let sat = write_sat(&cone(Vec3::new(300.0, 0.0, 0.0), z, 30.0, 0.0, 60.0).expect("cone")).expect("sat");
    let lines: Vec<&str> = sat.lines().collect();
    assert_eq!(lines.iter().filter(|l| l.starts_with("loop ")).count(), 3, "base face, cone side and apex\n{sat}");
    let point_edges: Vec<&&str> = lines.iter().filter(|l| l.starts_with("edge ") && l.contains(" 1 $") && l.contains(" 0 $")).collect();
    assert_eq!(point_edges.len(), 1, "{sat}");
    let f: Vec<&str> = point_edges[0].split_whitespace().collect();
    // edge attrib -1 attrib start 1 end 0 coedge curve sense @7 unknown #
    assert_eq!(f.get(4), f.get(6), "starts and ends at the apex\n{sat}");
    assert_eq!(f.get(9).copied(), Some("$-1"), "no curve\n{sat}");
    assert_eq!(lines.iter().filter(|l| l.starts_with("ellipse-curve ")).count(), 1, "{sat}");
    assert_eq!(lines.iter().filter(|l| l.starts_with("vertex ")).count(), 2, "{sat}");
    // The frustum and the sphere gain no point edge.
    for body in [cone(Vec3::new(0.0, 0.0, 0.0), z, 30.0, 10.0, 60.0).expect("frustum"), sphere(Vec3::new(0.0, 0.0, 0.0), 30.0).expect("sphere")] {
        let sat = write_sat(&body).expect("sat");
        assert!(sat.lines().filter(|l| l.starts_with("edge ")).all(|l| !l.contains("$-1 forward @7")), "{sat}");
    }
}
