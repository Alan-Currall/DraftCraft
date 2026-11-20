//! Paper vs model text height rows: editing paper height updates the entity and survives DXF/DWG.

use draftcraft_engine::Session;
use draftcraft_engine::doc::{Handle};
use serde_json::json;

#[test]
fn text_paper_height_edits_and_persists() {
    let mut s = Session::new();
    s.execute("style", &json!({ "name": "Anno", "annotative": true, "current": true })).unwrap();
    let r = s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 0.25 })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    // Change paper height through properties.set -> "height".
    s.execute("properties.set", &json!({ "handles": [h.hex()], "height": 0.5 })).unwrap();
    {
        let e = s.doc().unwrap().entity(h).unwrap();
        if let draftcraft_engine::doc::EntityKind::Text(t) = &e.kind {
            assert!((t.height - 0.5).abs() < 1e-9);
        } else {
            panic!("not text");
        }
    }
    // DXF and DWG save and reopen keep the paper height.
    for (name, format, binary) in [("h.dxf", "dxf", false), ("h.dwg", "dwg", true)] {
        let options = draftcraft_io::SaveOptions { version: None, binary };
        let bytes = draftcraft_io::write_with(s.doc().unwrap(), format, &options).unwrap();
        let reopened = if binary { draftcraft_io::read(&bytes, name).unwrap() } else { draftcraft_io::read_dxf(bytes.as_bytes()).unwrap() };
        let e = reopened.entity(h).unwrap();
        match &e.kind {
            draftcraft_engine::doc::EntityKind::Text(t) => assert!((t.height - 0.5).abs() < 1e-9),
            _ => panic!("not text"),
        }
    }
}

#[test]
fn dimension_paper_height_edits_and_persists() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "Anno", "annotative": true, "current": true })).unwrap();
    let r = s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [100, 0], "at": [50, 20] })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap()).unwrap();
    // Set paper text height as a per-dimension override.
    s.execute("properties.set", &json!({ "handles": [h.hex()], "textHeight": 3.0 })).unwrap();
    {
        let e = s.doc().unwrap().entity(h).unwrap();
        if let draftcraft_engine::doc::EntityKind::Dimension(dm) = &e.kind {
            assert_eq!(dm.overrides.get("textHeight").and_then(|v| v.as_f64()), Some(3.0));
        } else {
            panic!("not dimension");
        }
    }
    // Round-trip.
    let bytes = draftcraft_io::write_dxf(s.doc().unwrap());
    let reopened = draftcraft_io::read_dxf(bytes.as_bytes()).unwrap();
    let e = reopened.entity(h).unwrap();
    if let draftcraft_engine::doc::EntityKind::Dimension(dm) = &e.kind {
        assert_eq!(dm.overrides.get("textHeight").and_then(|v| v.as_f64()), Some(3.0));
    }
}

#[test]
fn mleader_paper_height_edits_and_persists() {
    let mut s = Session::new();
    s.execute("mleaderstyle", &json!({ "name": "Anno", "annotative": true, "current": true })).unwrap();
    let r = s.execute("mleader", &json!({ "points": [[0, 0], [10, 10]], "text": "X" })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    s.execute("properties.set", &json!({ "handles": [h.hex()], "height": 2.5 })).unwrap();
    {
        let e = s.doc().unwrap().entity(h).unwrap();
        if let draftcraft_engine::doc::EntityKind::MLeader(m) = &e.kind {
            assert_eq!(m.text.as_ref().map(|t| t.height), Some(2.5));
        } else {
            panic!("not mleader");
        }
    }
    let bytes = draftcraft_io::write_dxf(s.doc().unwrap());
    let reopened = draftcraft_io::read_dxf(bytes.as_bytes()).unwrap();
    let e = reopened.entity(h).unwrap();
    if let draftcraft_engine::doc::EntityKind::MLeader(m) = &e.kind {
        assert_eq!(m.text.as_ref().map(|t| t.height), Some(2.5));
    }
}

