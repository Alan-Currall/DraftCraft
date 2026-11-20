use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;
use draftcraft_engine::Session;
use draftcraft_ui_egui::{CadApp, Services, theme};

#[test]
fn opens_for_selected_annotative_objects() {
    // App with fonts installed and UI loop wired.
    let mut app = CadApp::new(Session::default(), Services::default());
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_context_setup(|ctx| theme::install_fonts(ctx, "en"))
        .build_ui_state(|ui, app: &mut CadApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            draftcraft_ui_egui::dialogs::show(app, &ctx);
        }, app);
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
