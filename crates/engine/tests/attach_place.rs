use draftcraft_engine::{Session, doc::EntityKind};
use serde_json::json;

fn last_kind(s: &Session) -> EntityKind {
    s.doc().unwrap().model.last().unwrap().kind.clone()
}

#[test]
fn imageattach_interactive_accepts_programmed_values() {
    let mut s = Session::new();
    // Start interactive IMAGEATTACH and feed path, pathType, at, width, rotation
    s.start("imageattach").unwrap();
    // Use a small PNG from the repository
    let path = "/agent/repos/DraftCraft/crates/engine/maps/site.png";
    s.cmdline(path).unwrap();
    s.cmdline("None").unwrap(); // store only file name
    s.cmdline("0,0").unwrap(); // at
    s.cmdline("4").unwrap(); // width
    s.cmdline("15").unwrap(); // rotation
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "imageattach finished");
    match last_kind(&s) {
        EntityKind::Image(i) => {
            assert!(i.path.ends_with("site.png"), "path stored");
            let ang = i.u.y.atan2(i.u.x);
            assert!((ang - 15f64.to_radians()).abs() < 1e-6, "rotation set");
        }
        k => panic!("not an image: {k:?}"),
    }
}

#[test]
fn imageattach_interactive_picks_scale_and_rotation() {
    let mut s = Session::new();
    s.start("imageattach").unwrap();
    let path = "/agent/repos/DraftCraft/crates/engine/maps/site.png";
    s.cmdline(path).unwrap();
    s.cmdline("Full").unwrap();
    s.cmdline("1,1").unwrap(); // insertion
    s.cmdline("4,1").unwrap(); // width by point: 3 units
    s.cmdline("1,2").unwrap(); // rotation by point: 90 degrees from +x
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "imageattach finished");
    match last_kind(&s) {
        EntityKind::Image(i) => {
            assert!((i.insert.x - 1.0).abs() < 1e-9 && (i.insert.y - 1.0).abs() < 1e-9);
            let ang = i.u.y.atan2(i.u.x);
            assert!((ang - 90f64.to_radians()).abs() < 1e-6, "rotation from point");
        }
        k => panic!("not an image"),
    }
}

#[test]
fn pdfattach_interactive_programmed_values() {
    // Create a tiny one-page PDF via engine PREVIEW (model to PDF) and attach it.
    let mut s = Session::new();
    s.execute("line", &json!({"points":[[0,0],[10,0]]})).unwrap();
    let pdf = draftcraft_io::pdf(&s.doc().unwrap(), &draftcraft_engine::doc::Space::Model, &draftcraft_io::PdfOptions::default()).unwrap();
    let path = std::env::temp_dir().join("attach-place.pdf");
    std::fs::write(&path, &pdf).unwrap();

    s.start("pdfattach").unwrap();
    s.cmdline(&path.to_string_lossy()).unwrap();
    s.cmdline("Relative").unwrap();
    s.cmdline("1").unwrap(); // page
    s.cmdline("0,0").unwrap(); // at
    s.cmdline("25.4").unwrap(); // scale
    s.cmdline("0").unwrap(); // rotation
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "pdfattach finished");
    match last_kind(&s) {
        EntityKind::Image(i) => {
            assert!(i.underlay.is_some(), "underlay");
        }
        k => panic!("not an underlay"),
    }
}

