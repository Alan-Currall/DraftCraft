//! STRETCH via a right-to-left press-and-drag crossing window at "Select objects" feeds the command.

use draftcraft_engine::Session;
use draftcraft_engine::doc::EntityKind;
use draftcraft_engine::geom::Vec2;
use draftcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(draftcraft_engine::doc::Drawing::new_metric(), "Stretch drag", None);
    session.execute("line", &json!({ "points": [[0, 0], [100, 0]] })).unwrap();
    session.execute("view.set", &json!({ "center": [50, 0], "height": 200 })).unwrap();
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn screen(h: &Harness<'_, CadApp>, p: Vec2) -> Pos2 {
    h.state().canvas.xf.unwrap().to_screen(p)
}

fn move_to(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
}

fn button(h: &mut Harness<'_, CadApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn click(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    move_to(h, pos);
    button(h, pos, true);
    button(h, pos, false);
}

fn enter(h: &mut Harness<'_, CadApp>) {
    h.event(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.event(Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(4);
}

fn prompt(h: &Harness<'_, CadApp>) -> String {
    h.state().session.current_prompt().map(|p| p.display()).unwrap_or_default()
}

fn drag(h: &mut Harness<'_, CadApp>, from: Vec2, to: Vec2) {
    let (a, b) = (screen(h, from), screen(h, to));
    move_to(h, a);
    button(h, a, true);
    for i in 1..=8 {
        move_to(h, a + (b - a) * (i as f32 / 8.0));
    }
    button(h, b, false);
}

#[test]
fn crossing_drag_then_enter_stretches() {
    let mut h = harness();
    // Start STRETCH like clicking the tool button.
    h.state_mut().start("stretch");
    h.run_steps(3);
    assert!(prompt(&h).starts_with("Select objects"), "{}", prompt(&h));
    // Right to left drag across the right end of the line.
    drag(&mut h, Vec2::new(120.0, 20.0), Vec2::new(80.0, -20.0));
    // The drag completes one window; Enter advances to base point.
    assert!(prompt(&h).starts_with("Select objects"), "after drag: {}", prompt(&h));
    enter(&mut h);
    assert!(h.state().session.running.is_some(), "STRETCH ended unexpectedly");
    assert!(prompt(&h).starts_with("Specify base point"), "{}", prompt(&h));
    // Base at (50,60), second at (70,60): displacement (20,0).
    click(&mut h, screen(&h, Vec2::new(50.0, 60.0)));
    assert!(prompt(&h).starts_with("Specify second point"), "{}", prompt(&h));
    click(&mut h, screen(&h, Vec2::new(70.0, 60.0)));
    assert!(h.state().session.running.is_none());
    // The line's right end moved to 120,0; left end stayed at 0,0.
    let line = h
        .state()
        .session
        .doc()
        .unwrap()
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Line(l) => Some(l.clone()),
            _ => None,
        })
        .unwrap();
    assert!(line.a.xy().dist(Vec2::new(0.0, 0.0)) < 1e-6, "{line:?}");
    assert!(line.b.xy().dist(Vec2::new(120.0, 0.0)) < 1e-6, "{line:?}");
}

