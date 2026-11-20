//! CETRANSPARENCY (the Properties palette's Transparency with nothing selected) applies to new
//! objects and is saved with them.
#![allow(clippy::unwrap_used)]

use draftcraft_doc::Transparency;
use draftcraft_engine::Session;
use serde_json::json;

fn last_transparency(s: &Session) -> Transparency {
    s.doc().unwrap().model.last().unwrap().common.transparency
}

#[test]
fn new_objects_get_the_current_transparency() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 10]] })).unwrap();
    assert_eq!(last_transparency(&s), Transparency::ByLayer, "default");

    s.execute("setvar", &json!({ "name": "CETRANSPARENCY", "value": "50" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    assert_eq!(last_transparency(&s), Transparency::Percent(50));

    s.execute("setvar", &json!({ "name": "CETRANSPARENCY", "value": "byblock" })).unwrap();
    s.execute("circle", &json!({ "center": [0, 0], "radius": 3 })).unwrap();
    assert_eq!(last_transparency(&s), Transparency::ByBlock);

    assert!(s.execute("setvar", &json!({ "name": "CETRANSPARENCY", "value": "95" })).is_err());
    assert!(s.execute("setvar", &json!({ "name": "CETRANSPARENCY", "value": "glass" })).is_err());

    // Saved with the object (both DXF and DWG).
    s.execute("setvar", &json!({ "name": "CETRANSPARENCY", "value": 30 })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [0, 10]] })).unwrap();
    let h = s.doc().unwrap().model.last().unwrap().handle;
    for name in ["t.dxf", "t.dwg"] {
        let bytes = draftcraft_io::write(s.doc().unwrap(), name).unwrap();
        let back = draftcraft_io::read(&bytes, name).unwrap();
        assert_eq!(back.entity(h).unwrap().common.transparency, Transparency::Percent(30), "{name}");
    }
}
