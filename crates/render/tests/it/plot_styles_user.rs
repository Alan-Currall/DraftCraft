use draftcraft_color::Color;
use draftcraft_doc::{Common, Drawing, EntityKind, Line, PlotStyleTable, Space};
use draftcraft_geom::Vec3;

fn line_at(y: f64) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(100.0, y, 0.0) })
}

fn make_ctb_bytes(body: &str) -> Vec<u8> {
    let mut out = Vec::from(b"PIAFILEVERSION_2.0\n"[..].to_vec());
    let mut comp = miniz_oxide::deflate::compress_to_vec_zlib(body.as_bytes(), 6);
    out.append(&mut comp);
    out
}

#[test]
fn color_dependent_user_table_applies_lineweight() {
    // Map Color_1 to 0.50 mm, Color_2 to 0.13 mm; leave others default.
    let body = r#"{"kind":"ctb","lineweights":{"1":0.50,"2":0.13}}"#;
    let bytes = make_ctb_bytes(body);
    let table = draftcraft_doc::parse_ctb_or_stb("user.ctb", &bytes).unwrap();
    assert!(table.is_color_dependent());
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common { color: Color::Index(1), ..Default::default() }, line_at(0.0)).unwrap();
    d.add(&Space::Model, Common { color: Color::Index(2), ..Default::default() }, line_at(10.0)).unwrap();
    // Build a plot with lineweights on and the user table.
    let opts = draftcraft_render::Options {
        transparency: false,
        tolerance: 0.001,
        min_dash: 0.0,
        text: false,
        linetypes: false,
        fill: false,
        lineweights: true,
        view_height: 0.0,
        plot_style_table: Some(std::sync::Arc::new(table)),
        view: None,
        skip_viewport: None,
        cancel: None,
    };
    let list = draftcraft_render::build_plot(&d, &Space::Model, &opts);
    // The two polylines' lineweights in mm follow the mapping (approximate equality).
    let lws: Vec<f32> = list.prims.iter().filter(|p| matches!(p.kind, draftcraft_render::Kind::Polyline)).map(|p| p.lw).collect();
    assert_eq!(lws.len(), 2, "{lws:?}");
    // The display list leaves widths in mm until pen::apply.
    assert!((lws[0] - 0.50).abs() < 1e-6, "{:?}", lws);
    assert!((lws[1] - 0.13).abs() < 1e-6, "{:?}", lws);
}

