//! OBJECTSCALE: add/delete scales for annotative objects and persist through DXF/DWG saves.

use draftcraft_engine::Session;
use serde_json::json;

fn scales_of(s: &Session, h: &str) -> Vec<String> {
    let d = s.doc().unwrap();
    let h = draftcraft_doc::Handle::parse_hex(h).unwrap();
    d.entity(h).unwrap().common.anno_scales.iter().map(|s| s.name.clone()).collect()
}

#[test]
fn add_and_remove_scales_and_persist() {
    let mut s = Session::default();
    // Add a scale to the drawing's list and set the current to 1:1.
    s.execute("scalelistedit", &json!({ "add": { "name": "1:1250", "ratio": "1:1250" } })).unwrap();
    s.execute("setvar", &json!({ "name": "CANNOSCALE", "value": "1:1" })).unwrap();
    // One text made annotative explicitly.
    let r = s.execute("text", &json!({ "at": [0, 0], "text": "A", "height": 2.5 })).unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    s.execute("properties.set", &json!({ "handles": [h.clone()], "annotative": true })).unwrap();
    // Add 1:1250 to it.
    s.execute("objectscale", &json!({ "handles": [h.clone()], "add": "1:1250" })).unwrap();
    let list = scales_of(&s, &h);
    assert!(list.iter().any(|n| n.eq_ignore_ascii_case("1:1")));
    assert!(list.iter().any(|n| n.eq_ignore_ascii_case("1:1250")));
    // Remove 1:1 (the object still keeps 1:1250).
    s.execute("objectscale", &json!({ "handles": [h.clone()], "delete": "1:1" })).unwrap();
    let list = scales_of(&s, &h);
    assert!(!list.iter().any(|n| n.eq_ignore_ascii_case("1:1")));
    assert!(list.iter().any(|n| n.eq_ignore_ascii_case("1:1250")));
    // DXF/DWG round-trips keep the object's scale list.
    for (name, format, binary) in [("anno.dxf", "dxf", false), ("anno.dwg", "dwg", true)] {
        let options = draftcraft_io::SaveOptions { version: None, binary };
        let bytes = draftcraft_io::write_with(s.doc().unwrap(), format, &options).unwrap();
        let reopened = if binary { draftcraft_io::read(&bytes, name).unwrap() } else { draftcraft_io::read_dxf(bytes.as_bytes()).unwrap() };
        let e = reopened.entity(draftcraft_doc::Handle::parse_hex(&h).unwrap()).unwrap();
        let names: Vec<String> = e.common.anno_scales.iter().map(|s| s.name.clone()).collect();
        assert!(!names.iter().any(|n| n.eq_ignore_ascii_case("1:1")));
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("1:1250")));
    }
}

