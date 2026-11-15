use draftcraft_doc::{Common, Drawing, EntityKind, Space};
use draftcraft_dxf::Version;
use draftcraft_geom::{Mat4, Vec3};
use draftcraft_solid::{Body, box_solid, cylinder};

fn data(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/acad-solids").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The solids of model space: (volume, centroid) of each, in drawing order.
fn solids(d: &Drawing) -> Vec<(f64, Vec3)> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Solid3d(so) => {
                let body = super::world_body(so).expect("body");
                let m = body.mass().expect("mass");
                Some((m.volume, m.centroid))
            }
            _ => None,
        })
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

fn check(name: &str, got: &[(f64, Vec3)], want: &[(f64, [f64; 3])]) {
    assert_eq!(got.len(), want.len(), "{name}: {got:?}");
    for (i, ((v, c), (wv, wc))) in got.iter().zip(want).enumerate() {
        assert!(close(*v, *wv), "{name} #{i}: volume {v}, AutoCAD's MASSPROP {wv}");
        let d = (*c - Vec3::new(wc[0], wc[1], wc[2])).len();
        assert!(d <= 1e-3, "{name} #{i}: centroid {c:?}, AutoCAD's MASSPROP {wc:?}");
    }
}

/// AutoCAD's MASSPROP for the six primitives of `solids-basic` (the drawings' README).
const BASIC: [(f64, [f64; 3]); 6] = [
    (240000.0, [50.0, 30.0, 20.0]),
    (98174.7704, [200.0, 0.0, 25.0]),
    (56548.6678, [300.0, 0.0, 15.0]),
    (113097.3355, [400.0, 0.0, 30.0]),
    (78956.8352, [500.0, 0.0, 0.0]),
    (80000.0, [626.6667, 25.0, 13.3333]),
];

fn close4(got: &[(f64, Vec3)], want: &[(f64, [f64; 3])], name: &str) {
    // MASSPROP prints four decimals: compare to that.
    assert_eq!(got.len(), want.len(), "{name}: {got:?}");
    for (i, ((v, c), (wv, wc))) in got.iter().zip(want).enumerate() {
        assert!((v - wv).abs() <= 1e-4 + 1e-9 * wv.abs(), "{name} #{i}: volume {v}, AutoCAD's MASSPROP {wv}");
        assert!((*c - Vec3::new(wc[0], wc[1], wc[2])).len() <= 1e-3, "{name} #{i}: centroid {c:?}, AutoCAD's MASSPROP {wc:?}");
    }
}

#[test]
fn autocad_primitives_open_as_solids_in_every_format() {
    for name in [
        "solids-basic-2000.dxf",
        "solids-basic-2010.dxf",
        "solids-basic-2018.dxf",
        "solids-basic-2000.dwg",
        "solids-basic-2013.dwg",
        "solids-basic-2018.dwg",
    ] {
        let d = crate::read(&data(name), name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(d.read_notes.iter().all(|n| !n.contains("3D solid")), "{name}: {:?}", d.read_notes);
        close4(&solids(&d), &BASIC, name);
        // The data store records became solids: none is left to write twice.
        assert!(d.foreign.data_store_entities().is_empty(), "{name}");
    }
}

#[test]
fn autocad_edited_solids_open_as_solids() {
    // SUBTRACT, FILLETEDGE, CHAMFEREDGE (the README's MASSPROP).
    let want = [(437168.1469, [50.0, 50.0, 25.0]), (495707.9633, [250.0, 50.0, 25.0]), (495166.6667, [450.0, 50.0, 24.7724])];
    for name in ["solids-edit-2000.dxf", "solids-edit-2018.dwg"] {
        let d = crate::read(&data(name), name).unwrap_or_else(|e| panic!("{name}: {e}"));
        close4(&solids(&d), &want, name);
    }
}

#[test]
fn autocad_free_form_solids_never_crash_and_say_what_is_left() {
    // LOFT and SWEEP make spline surfaces: a solid read, or a note saying why it is kept as saved.
    let name = "solids-free-2018.dwg";
    let d = crate::read(&data(name), name).unwrap_or_else(|e| panic!("{name}: {e}"));
    let read = solids(&d).len();
    let kept = d.model.iter().filter(|e| matches!(&e.kind, EntityKind::Unknown(u) if u.dxf_type == "3DSOLID")).count();
    assert_eq!(read + kept, 2, "{name}");
    if kept > 0 {
        assert!(d.read_notes.iter().any(|n| n.contains("3D solid")), "{name}: {:?}", d.read_notes);
    }
}

/// A drawing with a box placed away from the origin and a box with a hole.
fn drawing() -> (Drawing, Vec<(f64, Vec3)>) {
    let mut d = Drawing::new_metric();
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 60.0, 40.0)).expect("box");
    let mut so = super::solid_of(&b).expect("solid");
    let mut m = Mat4::IDENTITY;
    m.m[0][3] = 1000.0;
    m.m[1][3] = -50.0;
    so.placement = m;
    d.add(&Space::Model, Common::default(), EntityKind::Solid3d(so)).expect("add");
    let block = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 50.0)).expect("box");
    let hole = cylinder(Vec3::new(50.0, 50.0, -10.0), Vec3::new(0.0, 0.0, 1.0), 20.0, 70.0).expect("cylinder");
    let holed: Body = draftcraft_solid::boolean(&block, &hole, draftcraft_solid::BoolOp::Subtract).expect("subtract").expect("a solid");
    d.add(&Space::Model, Common::default(), EntityKind::Solid3d(super::solid_of(&holed).expect("solid"))).expect("add");
    let want = solids(&d);
    (d, want)
}

/// Each 3DSOLID record of DXF text: its groups as (code, value) pairs.
fn solid_records(text: &str) -> Vec<Vec<(String, String)>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out: Vec<Vec<(String, String)>> = Vec::new();
    let mut current: Option<Vec<(String, String)>> = None;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        if *code == "0" {
            out.extend(current.take());
            current = (*value == "3DSOLID").then(Vec::new);
        } else if let Some(c) = current.as_mut() {
            c.push((code.to_string(), value.to_string()));
        }
    }
    out.extend(current);
    out
}

#[test]
fn solids_save_as_autocad_3dsolids_in_every_version() {
    let (d, want) = drawing();
    let want: Vec<(f64, [f64; 3])> = want.iter().map(|(v, c)| (*v, [c.x, c.y, c.z])).collect();
    for v in Version::ALL {
        let dxf = crate::write_with(&d, "s.dxf", &crate::SaveOptions { version: Some(v), binary: false }).expect("dxf");
        let text = String::from_utf8_lossy(&dxf).replace("\r\n", "\n");
        assert_eq!(text.matches("\n3DSOLID\n").count(), 2, "{v:?}");
        // No interim meshes: AutoCAD's own entity.
        assert!(!text.contains("DRAFTCRAFT_SOLID") && !text.contains("\nPOLYLINE\n"), "{v:?}");
        assert_eq!(text.contains("\nACDSDATA\n"), v >= Version::R2013, "{v:?}");
        // AutoCAD discards a DXF file whose 3DSOLID lacks the AcDbEntity marker before its layer.
        for groups in solid_records(&text) {
            let at = |g: (&str, &str)| groups.iter().position(|(c, x)| c == g.0 && (g.1.is_empty() || x == g.1));
            let (marker, layer, modeler) = (at(("100", "AcDbEntity")), at(("8", "")), at(("100", "AcDbModelerGeometry")));
            assert!(marker.is_some() && marker < layer && layer < modeler, "{v:?}: {groups:?}");
            // SAT is ACIS 400, as AutoCAD writes it in a 2000 file: AutoCAD 2027 discards a 700
            // solid from a DWG 2000 to 2010 file (error 75003). The DWG writer takes these groups.
            if v < Version::R2013 {
                let first = groups.iter().skip_while(|(c, _)| c != "70").find(|(c, _)| c == "1");
                let first = first.map(|(_, x)| draftcraft_solid::acis::scramble(&x.replace("^ ", "^")));
                assert!(first.as_deref().is_some_and(|l| l.starts_with("400 ")), "{v:?}: {first:?}");
            }
        }
        check(&format!("DXF {v:?}"), &solids(&crate::read(&dxf, "s.dxf").expect("read dxf")), &want);
        let (dwg, unwritten) = crate::write_dwg_own(&d, v).expect("dwg");
        assert!(unwritten.is_empty(), "{v:?}: {unwritten:?}");
        check(&format!("DWG {v:?}"), &solids(&crate::read(&dwg, "s.dwg").expect("read dwg")), &want);
    }
}

#[test]
fn an_autocad_solid_saved_again_stays_a_solid() {
    for (name, out) in [
        ("solids-basic-2018.dwg", "a.dwg"),
        ("solids-basic-2018.dwg", "a.dxf"),
        ("solids-basic-2000.dxf", "a.dxf"),
        ("solids-basic-2000.dxf", "a.dwg"),
    ] {
        let d = crate::read(&data(name), name).expect("read");
        for v in [Version::R2000, Version::R2010, Version::R2018] {
            let bytes = crate::write_with(&d, out, &crate::SaveOptions { version: Some(v), binary: false }).expect("write");
            let back = crate::read(&bytes, out).expect("read back");
            close4(&solids(&back), &BASIC, &format!("{name} → {out} {v:?}"));
            assert_eq!(back.foreign.data_store_entities().len(), 0, "{name} → {out} {v:?}");
        }
    }
}

#[test]
fn hostile_3dsolid_groups_are_kept_and_never_crash() {
    let (d, _) = drawing();
    let text = String::from_utf8(crate::write_with(&d, "s.dxf", &crate::SaveOptions { version: Some(Version::R2000), binary: false }).expect("dxf"))
        .expect("text")
        .replace("\r\n", "\n");
    // The first solid's SAT text with lines dropped, numbers garbled or cut short: each is a solid
    // that can't be read, kept as saved with a note, or a solid; never a crash.
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let first = lines.iter().position(|l| l == "AcDbModelerGeometry").expect("solid");
    // Groups are line pairs: a code, then its value.
    let sat: Vec<usize> =
        (first + 1..lines.len() - 1).step_by(2).take_while(|i| lines[*i].trim() != "0").filter(|i| lines[*i].trim() == "1").map(|i| i + 1).collect();
    assert!(sat.len() > 20);
    for (k, &at) in sat.iter().enumerate().skip(3).step_by(5) {
        let mut l = lines.clone();
        match k % 3 {
            0 => l[at] = String::new(),
            1 => {
                let garbled: String = l[at].chars().map(|c| if c.is_ascii_alphanumeric() { 'n' } else { c }).collect();
                l[at] = garbled;
            }
            _ => {
                let half = l[at].len() / 2;
                l[at].truncate(half);
            }
        }
        let back = crate::read(l.join("\n").as_bytes(), "bad.dxf").expect("the drawing still opens");
        let solids = back.model.iter().filter(|e| matches!(e.kind, EntityKind::Solid3d(_))).count();
        let kept = back.model.iter().filter(|e| matches!(&e.kind, EntityKind::Unknown(u) if u.dxf_type == "3DSOLID")).count();
        assert_eq!(solids + kept, 2, "line {at}");
        if kept > 0 {
            assert!(back.read_notes.iter().any(|n| n.contains("3D solid")), "line {at}: {:?}", back.read_notes);
        }
    }
}

/// Writes the drawings AutoCAD is checked against into `DRAFTCRAFT_ACIS_OUT` (run by hand:
/// `DRAFTCRAFT_ACIS_OUT=dir cargo test -p draftcraft-io --features solids write_files_for_autocad -- --ignored`).
#[test]
#[ignore = "writes files for a check in AutoCAD"]
fn write_files_for_autocad() {
    let Ok(dir) = std::env::var("DRAFTCRAFT_ACIS_OUT") else { return };
    let z = Vec3::new(0.0, 0.0, 1.0);
    let mut d = Drawing::new_metric();
    let mut add = |body: Body, placement: Mat4| {
        let mut so = super::solid_of(&body).expect("solid");
        so.placement = placement;
        d.add(&Space::Model, Common::default(), EntityKind::Solid3d(so)).expect("add");
    };
    add(box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 60.0, 40.0)).expect("box"), Mat4::IDENTITY);
    add(cylinder(Vec3::new(200.0, 0.0, 0.0), z, 25.0, 50.0).expect("cylinder"), Mat4::IDENTITY);
    add(draftcraft_solid::cone(Vec3::new(300.0, 0.0, 0.0), z, 30.0, 0.0, 60.0).expect("cone"), Mat4::IDENTITY);
    add(draftcraft_solid::sphere(Vec3::new(400.0, 0.0, 30.0), 30.0).expect("sphere"), Mat4::IDENTITY);
    add(draftcraft_solid::torus(Vec3::new(500.0, 0.0, 0.0), 40.0, 10.0).expect("torus"), Mat4::IDENTITY);
    add(draftcraft_solid::cone(Vec3::new(600.0, 0.0, 0.0), z, 30.0, 10.0, 60.0).expect("frustum"), Mat4::IDENTITY);
    let block = box_solid(Vec3::new(0.0, 200.0, 0.0), Vec3::new(100.0, 300.0, 50.0)).expect("box");
    let hole = cylinder(Vec3::new(50.0, 250.0, -10.0), z, 20.0, 70.0).expect("cylinder");
    add(draftcraft_solid::boolean(&block, &hole, draftcraft_solid::BoolOp::Subtract).expect("subtract").expect("solid"), Mat4::IDENTITY);
    // A 100 × 60 × 40 box turned 30° about Z and moved to (300, 250, 0).
    let (s, c) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
    let turned = Mat4 { m: [[c, -s, 0.0, 300.0], [s, c, 0.0, 250.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] };
    add(box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 60.0, 40.0)).expect("box"), turned);
    for (v, y) in [(Version::R2000, "2000"), (Version::R2010, "2010"), (Version::R2013, "2013"), (Version::R2018, "2018")] {
        let dxf = crate::write_with(&d, "s.dxf", &crate::SaveOptions { version: Some(v), binary: false }).expect("dxf");
        std::fs::write(format!("{dir}/dc-solids-{y}.dxf"), dxf).expect("write");
        let (dwg, _) = crate::write_dwg_own(&d, v).expect("dwg");
        std::fs::write(format!("{dir}/dc-solids-{y}.dwg"), dwg).expect("write");
    }
    // AutoCAD's own drawing, opened and saved again by DraftCraft.
    let acad = crate::read(&data("solids-basic-2018.dwg"), "a.dwg").expect("read");
    std::fs::write(format!("{dir}/acad-resaved-2018.dwg"), crate::write_dwg_own(&acad, Version::R2018).expect("dwg").0).expect("write");
    std::fs::write(
        format!("{dir}/acad-resaved-2000.dxf"),
        crate::write_with(&acad, "a.dxf", &crate::SaveOptions { version: Some(Version::R2000), binary: false }).expect("dxf"),
    )
    .expect("write");
    for (i, (v, c)) in solids(&d).iter().enumerate() {
        println!("#{} volume {v:.4} centroid {:.4}, {:.4}, {:.4}", i + 1, c.x, c.y, c.z);
    }
}
