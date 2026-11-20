use egui_kittest::Harness;
use serde_json::json;

use crate::CadApp;
use draftcraft_engine::Session;

fn harness() -> Harness<'static, CadApp> {
    Harness::builder().with_size(egui::vec2(900.0, 700.0)).with_context_setup(|ctx| crate::theme::install_fonts(ctx, "en")).build_ui_state(
        CadApp::new(Session::default(), crate::Services::default()),
        |app, _| app,
        |app, ctx| {
            app.logic(ctx);
            app.ui(ctx);
        },
    )
}

#[test]
fn opens_for_selected_annotative_objects() {
    let mut h = harness();
    // One annotative text selected.
    let r = h.state_mut().run("text", json!({ "at": [0, 0], "text": "A", "height": 2.5 })).unwrap();
    let hx = r["handle"].as_str().unwrap().to_string();
    h.state_mut().run("properties.set", json!({ "handles": [hx.clone()], "annotative": true })).unwrap();
    h.state_mut().run("select", json!({ "handles": [hx] })).unwrap();
    // Open the dialog.
    h.state_mut().run("ui.objectscale.dialog", json!({})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Annotation Object Scale List").is_some(), "dialog not open");
}
