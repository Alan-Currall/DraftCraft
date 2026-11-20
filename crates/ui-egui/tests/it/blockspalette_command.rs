use draftcraft_ui_egui::{CadApp, Session, Services};
use serde_json::json;

#[test]
fn typing_blockspalette_opens_the_palette() {
    let mut app = CadApp::new(Session::new(), Services::default());
    // Ensure palettes are visible and have a right dock to host Blocks.
    app.run("ui.toggle.palettes", json!({"on": true})).unwrap();
    assert!(app.cmdline("BLOCKSPALETTE").is_none());
    assert!(app.ui.docking.layout.contains(&draftcraft_ui_egui::docking::Panel::Blocks));
    // The ui.panel.blocks id also works.
    app.run("ui.dock", json!({"operation":"close","panel":"blocks"})).unwrap();
    app.run("ui.panel.blocks", json!({})).unwrap();
    assert!(app.ui.docking.layout.contains(&draftcraft_ui_egui::docking::Panel::Blocks));
}

