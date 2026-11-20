//! DraftCraft Help in the real app shell: a question typed at the command line (spaces and all)
//! opens the Help panel on the answer, F1 opens it, and its Start button runs the command.

use draftcraft_engine::Session;
use draftcraft_ui_egui::{CadApp, Services, docking::Panel, help};
use egui::{Event, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.in_window_menu = false;
    let mut harness = Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness
}

fn release_focus(h: &mut Harness<'_, CadApp>) {
    h.ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
}

/// Type `text` at the command line one key at a time, as a person does.
fn type_keys(h: &mut Harness<'_, CadApp>, text: &str) {
    release_focus(h);
    for c in text.chars() {
        h.event(Event::Text(c.to_string()));
        h.run_steps(1);
    }
}

#[test]
fn a_question_typed_at_the_command_line_opens_help_on_the_answer() {
    let mut h = harness();
    type_keys(&mut h, "how do I draw a line");
    assert_eq!(h.state().cmd.buffer, "how do I draw a line", "a space inside a question is typed, not Enter");
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert!(h.state().ui.docking.layout.contains(&Panel::Help));
    assert_eq!(h.state().ui.help.query, "how do I draw a line");
    assert!(h.state().session.running.is_none());
    // The answer shows the Line topic with its Start button; clicking it starts LINE.
    h.get_by_label("Start LINE").click();
    h.run_steps(3);
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("line"));
}

#[test]
fn an_ordinary_command_still_ends_at_the_space() {
    let mut h = harness();
    type_keys(&mut h, "circle ");
    h.run_steps(2);
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("circle"));
    assert!(!h.state().ui.docking.layout.contains(&Panel::Help));
}

#[test]
fn f1_opens_the_help_panel() {
    let mut h = harness();
    release_focus(&mut h);
    h.key_press(egui::Key::F1);
    h.run_steps(4);
    assert!(h.state().ui.docking.layout.contains(&Panel::Help));
}

// Ranking is unit-tested in help::tests; the integration tests above check UX wiring.
