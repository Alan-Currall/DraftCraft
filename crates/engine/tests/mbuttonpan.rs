//! MBUTTONPAN: whether the middle button pans (1, the default) or a middle click opens the
//! object snap menu (0). A user-profile setting, typed like any system variable.

use draftcraft_engine::{Input, Session, sysvars};
use serde_json::json;

#[test]
fn mbuttonpan_defaults_on_is_typed_and_kept_in_the_profile() {
    let mut s = Session::new();
    assert!(s.settings.mbuttonpan);
    assert_eq!(sysvars::get(&s, "MBUTTONPAN"), Some(json!(1)));

    // Typed at the command line: the name, then the new value.
    s.start("MBUTTONPAN").unwrap();
    s.input(Input::Text("0".into())).unwrap();
    assert!(!s.settings.mbuttonpan);
    assert_eq!(sysvars::get(&s, "mbuttonpan"), Some(json!(0)));

    // Only 0 and 1 are valid.
    assert!(s.execute("setvar", &json!({ "name": "MBUTTONPAN", "value": 2 })).is_err());
    assert!(!s.settings.mbuttonpan);

    // A profile variable: OPTIONS lists it and sets it, and a reset turns it back on.
    let profile = s.execute("options", &json!({})).unwrap();
    assert!(profile.to_string().contains("MBUTTONPAN"), "{profile}");
    s.execute("options", &json!({ "MBUTTONPAN": 1 })).unwrap();
    assert!(s.settings.mbuttonpan);
    s.execute("options", &json!({ "MBUTTONPAN": 0 })).unwrap();
    s.execute("options", &json!({ "reset": true })).unwrap();
    assert!(s.settings.mbuttonpan);
}

