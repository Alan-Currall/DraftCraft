use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;
use draftcraft_engine::Session;
use draftcraft_ui_egui::{CadApp, Services, theme};

#[test]
fn properties_shows_paper_and_model_text_height_for_annotative_text() {
    let mut app = CadApp::new(Session::default(), Services::default());
    // Show palettes like other tests do.
    app.ui.show_toolsets = true;
    app.ui.show_palettes = true;
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_context_setup(|ctx| theme::install_fonts(ctx, "en"))
        .build_ui_state(|ui, app: &mut CadApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            draftcraft_ui_egui::dialogs::show(app, &ctx);
            draftcraft_ui_egui::palettes::right_palettes(app, ui);
        }, app);
    // Annotative style and text selected.
    h.state_mut().run("style", json!({ "name": "Anno", "annotative": true, "current": true })).unwrap();
    let r = h.state_mut().run("text", json!({ "at": [0, 0], "text": "A", "height": 0.25 })).unwrap();
    let hx = r["handle"].as_str().unwrap().to_string();
    h.state_mut().run("properties.set", json!({ "handles": [hx.clone()], "annotative": true })).unwrap();
    h.state_mut().run("select", json!({ "handles": [hx] })).unwrap();
    h.run_steps(4);
    // The rows are present.
    assert!(h.query_by_label("Paper text height").is_some());
    assert!(h.query_by_label("Model text height").is_some());
}

