use serde_json::{Value, json};

use super::*;
use crate::docking::Panel;
use crate::{CadApp, Services};
use draftcraft_engine::Session;

fn best(question: &str) -> &'static str {
    search(question, 5).first().map_or("(nothing)", |h| h.topic.command)
}

#[test]
fn the_index_loads_with_written_topics_and_every_command() {
    assert!(ENTRIES.len() >= 40, "only {} written topics", ENTRIES.len());
    let specs = draftcraft_engine::command_specs().iter().filter(|s| !s.id.contains('.') && !s.label.is_empty()).count();
    // Every command is searchable: the written topics plus one from each other command's spec.
    assert!(docs().len() >= specs, "{} topics for {specs} commands", docs().len());
    for s in draftcraft_engine::command_specs().iter().filter(|s| !s.id.contains('.') && !s.label.is_empty()) {
        assert!(topic(s.id).is_some(), "{} has no help topic", s.id);
    }
}

#[test]
fn every_written_topic_names_a_real_command_once() {
    let mut seen = std::collections::HashSet::new();
    for e in ENTRIES {
        assert!(draftcraft_engine::find_command(e.command).is_some() || crate::menus::is_ui_command(e.command), "{}: no such command", e.command);
        assert!(seen.insert(e.command), "{} has two topics", e.command);
        assert!(GROUPS.contains(&e.group), "{}: unknown group {}", e.command, e.group);
        assert!(!e.title.is_empty() && !e.summary.is_empty(), "{}: no title or summary", e.command);
        assert!(e.steps.len() >= 2, "{}: too few steps", e.command);
        assert!(e.asks.len() >= 3, "{}: give at least three ways of asking", e.command);
        for r in e.related {
            assert!(topic(r).is_some(), "{}: related topic {r} does not exist", e.command);
        }
    }
}

#[test]
fn every_written_topic_has_the_icon_its_tool_button_shows() {
    for e in ENTRIES {
        if let Some(tool) = tool_icon(e.command) {
            assert_eq!(tool, e.icon, "{}: Help shows a different icon from its tool button", e.command);
        }
    }
}

#[test]
fn every_written_topic_icon_draws_something() {
    let ctx = egui::Context::default();
    let count = |icon: Option<Icon>| {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(icon) = icon {
                let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("help-icon-test")));
                icons::paint(&p, egui::Rect::from_min_size(egui::pos2(10.0, 10.0), vec2(40.0, 40.0)), icon, false);
            }
        });
        out.textures_delta.clear();
        out.shapes.len()
    };
    let empty = count(None);
    for e in ENTRIES {
        assert!(count(Some(e.icon)) > empty, "{}: its icon {:?} draws nothing", e.command, e.icon);
    }
}

#[test]
fn how_do_i_draw_a_line_finds_line_first() {
    for q in ["how do i draw a line", "How do I draw a line?", "HOW DO I DRAW A LINE", "draw a line", "line"] {
        assert_eq!(best(q), "line", "{q:?}");
    }
    let hits = search("how do i draw a line", 5);
    let top = hits.first().map(|h| h.topic).and_then(|t| t.icon());
    assert_eq!(top, Some(Icon::Line), "the answer shows the Line tool's icon");
}

#[test]
fn everyday_questions_find_the_right_command() {
    for (q, want) in [
        ("how do I draw a circle", "circle"),
        ("How can I round a corner?", "fillet"),
        ("how do i delete something", "erase"),
        ("make a copy of an object", "copy"),
        ("how do I print to pdf", "plot"),
        ("add a dimension", "dimlinear"),
        ("how do I create a new layer", "layer"),
        ("fill an area with a pattern", "hatch"),
        ("how do I make a block", "block"),
        ("draw a parallel line", "offset"),
        ("cut off overhanging lines", "trim"),
        ("make a line longer", "extend"),
        ("how do I zoom to everything", "zoom"),
        ("snap to the centre of a circle", "osnap"),
        ("snap to the center of a circle", "osnap"),
        ("change the colour of an object", "properties"),
        ("draw a hexagon", "polygon"),
        ("how long is this line", "dist"),
        ("add an arrow with text", "mleader"),
        ("undo a mistake", "undo"),
    ] {
        assert_eq!(best(q), want, "{q:?}");
    }
}

#[test]
fn slips_aliases_and_unwritten_commands_are_found() {
    assert_eq!(best("how do i draw a cirle"), "circle");
    assert_eq!(best("rectangel"), "rectang");
    assert_eq!(best("L"), "line");
    assert_eq!(best("TR"), "trim");
    // DONUT has no written topic yet; Help still finds it from its command spec.
    assert!(entry("donut").is_none());
    assert_eq!(best("how do I draw a donut"), "donut");
    assert!(search("zzzz qqqq", 5).is_empty());
    assert!(search("", 5).is_empty());
}

#[test]
fn where_to_find_lists_typing_the_menu_and_tool_sets() {
    let line = places("line");
    assert!(line.contains(&"Type LINE (or L) and press Enter".to_string()), "{line:?}");
    assert!(line.contains(&"Menu: Draw > Line".to_string()), "{line:?}");
    assert!(line.iter().any(|p| p.starts_with("Tool Sets: ")), "{line:?}");
    let undo = places("undo");
    assert!(undo.iter().any(|p| p == "Menu: Edit > Undo") && undo.iter().any(|p| p == "Keyboard: Cmd+Z"), "{undo:?}");
    let plot = places("plot");
    assert!(plot.iter().any(|p| p == "Keyboard: Cmd+P"), "{plot:?}");
    // Every written topic says where to find it beyond typing.
    for e in ENTRIES {
        assert!(places(e.command).len() >= 2, "{}: {:?}", e.command, places(e.command));
    }
}

#[test]
fn questions_at_the_command_line_are_told_from_commands() {
    for q in ["how do I draw a line", "How do I draw a line?", "what is a block", "trim?", "how"] {
        assert!(is_question(q), "{q:?}");
    }
    for c in ["line", "LINE 0,0 10,10", "l", "(+ 1 2)", "?", "help", "circle 0,0 5"] {
        assert!(!is_question(c), "{c:?}");
    }
    for w in QUESTION_WORDS {
        assert!(!known_name(w), "{w} is a command name, so it can't start a question");
    }
    assert!(starts_question("how"));
    assert!(!starts_question("line"));
}

#[test]
fn a_question_typed_at_the_command_line_opens_help_with_it() {
    let mut app = CadApp::new(Session::empty(), Services::default());
    assert!(!app.ui.docking.layout.contains(&Panel::Help));
    assert!(app.cmdline("how do I draw a line").is_none());
    assert!(app.ui.docking.layout.contains(&Panel::Help));
    assert_eq!(app.ui.help.query, "how do I draw a line");
    assert!(app.session.running.is_none(), "no command started");
}

#[test]
fn help_f1_and_the_menu_open_the_panel() {
    let mut app = CadApp::new(Session::empty(), Services::default());
    assert!(app.cmdline("HELP").is_none());
    assert!(app.ui.docking.layout.contains(&Panel::Help));
    let r = app.run("ui.panel.help", json!({ "query": "draw a circle" })).unwrap_or(Value::Null);
    assert_eq!(r["results"][0]["command"], "circle", "{r}");
    assert_eq!(crate::menus::UI_COMMANDS.iter().find(|c| c.0 == "ui.panel.help").and_then(|c| c.3), Some("F1"));
}

#[test]
fn the_help_panel_draws_a_frame_without_trouble() {
    let mut app = CadApp::new(Session::empty(), Services::default());
    let ctx = egui::Context::default();
    for q in ["", "how do I draw a line", "zzzz", "donut"] {
        app.run("ui.panel.help", json!({ "query": q })).unwrap_or(Value::Null);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }
    app.ui.help.query.clear();
    app.ui.help.selected = Some("hatch");
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
}
