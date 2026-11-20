use draftcraft_engine::{Input, Session};
use draftcraft_engine::geom::Vec2;
use draftcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.canvas.zoom_pending = false;
    // One entity to make the scene non-empty.
    app.session.execute("line", &json!({"points":[[0,0],[10,0]]})).unwrap();
    app.session.execute("view.set", &json!({"center":[5,0],"height":30})).unwrap();
    // Coarse grid snap and ortho on: selection corners must ignore these.
    app.session.execute("setvar", &json!({"name":"SNAPUNIT","value":[5.0,5.0]})).unwrap();
    app.session.execute("setvar", &json!({"name":"SNAPMODE","value":1})).unwrap();
    app.session.execute("setvar", &json!({"name":"ORTHOMODE","value":1})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn click(h: &mut Harness<'_, CadApp>, at: Pos2) {
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn selection_window_corners_ignore_snaps_and_ortho_at_prompts() {
    let mut h = harness();
    // Start a pure selection prompt.
    h.state_mut().session.start("select").unwrap();
    h.run_steps(2);
    let xf = h.state().canvas.xf.unwrap();
    // A raw point off the grid.
    let a_world = Vec2::new(2.3, 1.7);
    let a = xf.to_screen(a_world);
    click(&mut h, a);
    // First corner recorded as the raw world point (not snapped to the grid, not orthogonalised).
    let pw = h.state().session.pending_window.expect("pending selection window");
    assert!(pw.corner.near(a_world, 1e-6), "first corner must follow raw cursor, got {:?}", pw.corner);
    // The opposite corner (second click) also uses raw coordinates.
    let b_world = Vec2::new(8.9, 3.4);
    let b = xf.to_screen(b_world);
    click(&mut h, b);
    assert!(h.state().session.pending_window.is_none(), "window must close on second click");
    // Feed Enter to finish the selection (no-op in this test).
    h.state_mut().session.input(Input::Enter).unwrap();
}

