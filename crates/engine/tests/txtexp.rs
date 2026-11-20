//! TXTEXP (TXTEXPLODE): text becomes polylines tracing its characters exactly as drawn, keeping
//! the text's properties, in one undo step.

use draftcraft_color::Color;
use draftcraft_doc::{Entity, EntityKind, Handle, Lineweight};
use draftcraft_engine::{Input, Session, render};
use draftcraft_geom::{Bounds2, Vec2};
use serde_json::json;

fn last_of(s: &Session, kind: &str) -> Handle {
    s.doc().unwrap().model.iter().filter(|e| format!("{:?}", e.kind).starts_with(kind)).last().unwrap().handle
}

fn of_kind(s: &Session, kind: &str) -> Vec<Entity> {
    s.doc().unwrap().model.iter().filter(|e| format!("{:?}", e.kind).starts_with(kind)).map(|e| (**e).clone()).collect()
}

/// The drawn text's geometry bounds (strokes and glyph outlines), from the renderer.
fn drawn_bounds(s: &Session, h: Handle) -> Bounds2 {
    let d = s.doc().unwrap();
    let e = d.entity(h).unwrap();
    let pts: Vec<Vec2> = match &e.kind {
        EntityKind::Text(t) => render::place_text_entity(d, t, &t.value).0.outlines().into_iter().flatten().collect(),
        EntityKind::MText(m) => render::layout_mtext_entity(d, m).strokes.into_iter().flatten().collect(),
        _ => unreachable!(),
    };
    Bounds2::from_points(pts)
}

fn polyline_bounds(es: &[Entity]) -> Bounds2 {
    Bounds2::from_points(es.iter().flat_map(|e| match &e.kind {
        EntityKind::LwPolyline(p) => p.vertices.iter().map(|v| v.p).collect::<Vec<_>>(),
        _ => Vec::new(),
    }))
}

fn same(a: Bounds2, b: Bounds2) -> bool {
    a.min.near(b.min, 1e-9) && a.max.near(b.max, 1e-9)
}

#[test]
fn text_becomes_polylines_where_it_was_drawn_with_its_properties_in_one_undo_step() {
    let mut s = Session::new();
    s.execute("text", &json!({ "at": [10, 20], "text": "HOB 8", "height": 2.5, "rotation": 30, "justify": "MC" })).unwrap();
    let h = last_of(&s, "Text");
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            e.common.color = Color::Index(1);
            e.common.linetype = "Continuous".into();
            e.common.lineweight = Lineweight::Mm100(35);
            if let EntityKind::Text(t) = &mut e.kind {
                t.width_factor = 0.8;
                t.oblique = 15f64.to_radians();
            }
        })
        .unwrap();
    let want = drawn_bounds(&s, h);
    assert!(want.width() > 1.0);
    let undo = s.state().unwrap().undo.len();

    let r = s.execute("txtexp", &json!({ "handles": [h.hex()] })).unwrap();
    assert_eq!(r["texts"], json!(1));
    assert!(of_kind(&s, "Text").is_empty(), "the text is deleted");
    let pl = of_kind(&s, "LwPolyline");
    assert!(pl.len() >= 5, "a polyline per stroke or contour: {}", pl.len());
    assert_eq!(r["handles"].as_array().unwrap().len(), pl.len());
    assert!(same(polyline_bounds(&pl), want), "{:?} vs {want:?}", polyline_bounds(&pl));
    for e in &pl {
        assert_eq!(e.common.color, Color::Index(1));
        assert_eq!(e.common.linetype, "Continuous");
        assert_eq!(e.common.lineweight, Lineweight::Mm100(35));
        assert_eq!(e.common.layer, "0");
    }
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step");

    s.execute("undo", &json!({})).unwrap();
    assert!(of_kind(&s, "LwPolyline").is_empty());
    assert_eq!(of_kind(&s, "Text").len(), 1);
}

#[test]
fn mtext_explodes_with_its_wrapping_and_other_objects_are_kept() {
    let mut s = Session::new();
    s.execute("mtext", &json!({ "at": [0, 0], "text": "FIRST LINE OF A NOTE THAT WRAPS\\PSECOND", "height": 2.0, "width": 30 })).unwrap();
    let m = last_of(&s, "MText");
    s.execute("line", &json!({ "points": [[0, -50], [10, -50]] })).unwrap();
    let line = last_of(&s, "Line");
    let want = drawn_bounds(&s, m);
    // The wrapped MTEXT is taller than two lines of text.
    assert!(want.height() > 3.0 * 2.0, "{want:?}");

    // Interactive, typed by its long name: select the text and the line, then Enter.
    s.start("TXTEXPLODE").unwrap();
    s.input(Input::Pick(vec![m, line])).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(!s.prompting());
    assert!(of_kind(&s, "MText").is_empty());
    assert!(s.doc().unwrap().entity(line).is_some(), "not text: kept");
    let pl = of_kind(&s, "LwPolyline");
    assert!(!pl.is_empty());
    assert!(same(polyline_bounds(&pl), want), "{:?} vs {want:?}", polyline_bounds(&pl));
    assert!(s.log.iter().any(|l| l.contains("1 text object exploded")), "{:?}", s.log);
}
