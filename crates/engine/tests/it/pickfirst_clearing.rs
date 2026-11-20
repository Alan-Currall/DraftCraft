use draftcraft_engine::Session;
use draftcraft_geom::Vec2;
use serde_json::json;

#[test]
fn pickfirst_off_clears_selection_on_command_start() {
    let mut s = Session::new();
    // One object to select.
    let r = s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let h = r["handles"][0].as_str().unwrap().to_string();
    let hh = draftcraft_doc::Handle::parse_hex(&h).unwrap();
    s.set_selection(vec![hh]);
    assert_eq!(s.selection(), vec![hh]);
    // Turn PICKFIRST off.
    s.execute("options", &json!({"PICKFIRST": 0})).unwrap();
    assert!(!s.settings.pickfirst);
    // Start a selection-taking command: selection is cleared and the prompt asks for objects.
    s.start("erase").unwrap();
    assert!(s.selection().is_empty(), "pre-selection must be cleared when PICKFIRST is off");
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    // Pick the object to finish and verify normal behaviour continues.
    s.input(draftcraft_engine::Input::Point(Vec2::new(5.0, 0.0))).unwrap();
    s.input(draftcraft_engine::Input::Enter).unwrap();
    assert!(s.doc().unwrap().model.is_empty(), "object should be erased");
}

