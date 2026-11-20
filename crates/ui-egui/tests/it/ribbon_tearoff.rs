use draftcraft_ui_egui::{CadApp, Services};
use serde_json::json;

#[test]
fn tearoff_ribbon_panel_floats_and_persists() {
    let mut app = CadApp::new(draftcraft_engine::Session::new(), Services::default());
    // Open and float the Home/Draw panel.
    let id = "ribbon:Home:Draw";
    draftcraft_ui_egui::docking::command(&mut app, false, &json!({"operation":"open","panel": id})).unwrap();
    draftcraft_ui_egui::docking::command(&mut app, false, &json!({"operation":"float","panel": id, "rect":[200.0,120.0,280.0,180.0]})).unwrap();
    // Saved and reloaded workspace keeps it.
    let saved = serde_json::to_string(&app.ui).unwrap();
    let restored: draftcraft_ui_egui::UiState = serde_json::from_str(&saved).unwrap();
    assert!(restored
        .docking
        .layout
        .floating
        .iter()
        .any(|w| w
            .panels()
            .any(|p| matches!(p, draftcraft_ui_egui::docking::Panel::Ribbon { tab: t, panel: pn } if (*t == "Home") && (*pn == "Draw")))));
}

#[test]
fn return_ribbon_panels_closes_all_tearoffs() {
    let mut app = CadApp::new(draftcraft_engine::Session::new(), Services::default());
    for (tab, panel) in [("Home", "Draw"), ("Home", "Modify")] {
        let id = format!("ribbon:{tab}:{panel}");
        draftcraft_ui_egui::docking::command(&mut app, false, &json!({"operation":"open","panel": id})).unwrap();
        draftcraft_ui_egui::docking::command(&mut app, false, &json!({"operation":"float","panel": id, "rect":[160.0,140.0,260.0,160.0]})).unwrap();
    }
    // Close them through the UI command.
    draftcraft_ui_egui::menus::activate(&mut app, "ui.ribbon.returnpanels");
    assert!(!app
        .ui
        .docking
        .layout
        .panels()
        .iter()
        .any(|p| matches!(p, draftcraft_ui_egui::docking::Panel::Ribbon { .. })));
    assert!(app.ui.docking.hidden.iter().all(|(p, _)| !matches!(p, draftcraft_ui_egui::docking::Panel::Ribbon { .. })));
}

#[test]
fn per_panel_return_closes_only_that_panel() {
    let mut app = CadApp::new(draftcraft_engine::Session::new(), Services::default());
    for (tab, panel) in [("Home", "Draw"), ("Home", "Modify")] {
        let id = format!("ribbon:{tab}:{panel}");
        draftcraft_ui_egui::docking::command(&mut app, false, &json!({"operation":"open","panel": id})).unwrap();
        draftcraft_ui_egui::docking::command(&mut app, false, &json!({"operation":"float","panel": id, "rect":[280.0,180.0,260.0,160.0]})).unwrap();
    }
    // Close only Draw.
    draftcraft_ui_egui::docking::command(&mut app, false, &json!({"operation":"close","panel":"ribbon:Home:Draw"})).unwrap();
    // Modify remains.
    assert!(app.ui.docking.layout.panels().iter().any(|p| matches!(p, draftcraft_ui_egui::docking::Panel::Ribbon { tab, panel } if *tab == "Home" && *panel == "Modify")));
    assert!(!app.ui.docking.layout.panels().iter().any(|p| matches!(p, draftcraft_ui_egui::docking::Panel::Ribbon { tab, panel } if *tab == "Home" && *panel == "Draw")));
}

#[test]
fn unknown_ribbon_id_opens_nothing_special_and_is_dropped() {
    let mut app = CadApp::new(draftcraft_engine::Session::new(), Services::default());
    // Operation with an unknown ribbon id resolves to a safe built-in (Properties) rather than leaking or erroring.
    let _ = draftcraft_ui_egui::docking::command(&mut app, false, &json!({"operation":"open","panel":"ribbon:Unknown:Foo"})).unwrap();
    assert!(app.ui.docking.layout.contains(&draftcraft_ui_egui::docking::Panel::Properties));
}
