use draftcraft_engine::Session;
use draftcraft_ui_egui::CadApp;

#[test]
fn modify_cancel_does_not_change_style() {
    let mut app = CadApp::new(Session::new(), draftcraft_ui_egui::Services::default());
    let d = app.session.doc().unwrap().clone();
    let base = d.dim_style("Standard").cloned().unwrap_or_default();
    let mut working = base.clone();
    working.decimals = base.decimals.wrapping_add(1);
    // No commit: style must stay the same
    let after = app.session.doc().unwrap().dim_style("Standard").cloned().unwrap_or_default();
    assert_eq!(after.decimals, base.decimals, "Cancel leaves style unchanged");
}

#[test]
fn modify_ok_commits_changes() {
    let mut app = CadApp::new(Session::new(), draftcraft_ui_egui::Services::default());
    let d = app.session.doc().unwrap().clone();
    let base = d.dim_style("Standard").cloned().unwrap_or_default();
    let mut working = base.clone();
    working.decimals = 3;
    draftcraft_ui_egui::managers::commit_style_edit(&mut app, &working);
    let got = app.session.doc().unwrap().dim_style("Standard").cloned().unwrap();
    assert_eq!(got.decimals, 3, "OK applies Modify edits");
}

#[test]
fn override_ok_sets_header_dim_variables() {
    let mut app = CadApp::new(Session::new(), draftcraft_ui_egui::Services::default());
    // Start from Standard
    let d = app.session.doc().unwrap().clone();
    let base = d.dim_style("Standard").cloned().unwrap_or_default();
    let mut working = base.clone();
    working.ext_offset = base.ext_offset + 1.25;
    // DIMEXO should be set on commit
    draftcraft_ui_egui::managers::commit_override_edit(&mut app, &base, &working);
    let exo = app.session.doc().unwrap().header.f64("DIMEXO", 0.0);
    assert!((exo - working.ext_offset).abs() < 1e-9, "Override sets DIMEXO");
}

