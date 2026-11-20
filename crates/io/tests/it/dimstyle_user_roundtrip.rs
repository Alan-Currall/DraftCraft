//! A user-created dimension style round-trips through DXF and DWG saves and reopens,
//! and dimensions using it keep rendering accordingly.
use draftcraft_doc::{Common, DimKind, DimStyle, Dimension, Drawing, Entity, EntityKind, Space, Vec3};
use draftcraft_io::{SaveOptions, Version, read_dxf, write_dxf, write_with};

fn sample_style() -> DimStyle {
    DimStyle {
        name: "Custom".into(),
        // Non-defaults across tabs to exercise IO
        scale: 2.0,
        arrow_size: 0.25,
        tick_size: 0.0,
        arrow_block: "archtick".into(),
        arrow_block1: String::new(),
        arrow_block2: "open".into(),
        ext_offset: 0.05,
        ext_extend: 0.2,
        text_height: 0.22,
        text_gap: 0.08,
        decimals: 2,
        linear_unit: 2,
        text_style: "Standard".into(),
        post: "<> mm".into(),
        zero_suppression: 8,
        baseline_spacing: 0.4,
        dim_line_extend: 0.0,
        fit: 3,
        annotative: false,
        alt: true,
        alt_factor: 25.4,
        alt_decimals: 1,
        alt_post: " (<> in)".into(),
        ..DimStyle::default()
    }
}

fn drawing_with_custom() -> Drawing {
    let mut d = Drawing::new_metric();
    let st = sample_style();
    d.dim_styles.push(st);
    d.header.set_str("DIMSTYLE", "Custom");
    // A linear dimension that uses it
    let dm = Dimension {
        style: "Custom".into(),
        text: "<>".into(),
        kind: DimKind::Linear { rotation: 0.0 },
        p13: Vec3::new(0.0, 0.0, 0.0),
        p14: Vec3::new(18.5, 0.0, 0.0),
        defpt: Vec3::new(9.25, 2.0, 0.0),
        p15: Vec3::ZERO,
        assoc: Vec::new(),
        overrides: serde_json::Map::new(),
        text_mid: Vec3::new(9.25, 2.0, 0.0),
        user_text_pos: false,
        text_rotation: 0.0,
        block: None,
    };
    d.model.push(Entity { handle: d.alloc_handle(), common: Common::default(), kind: EntityKind::Dimension(dm) });
    d
}

#[test]
fn user_dimstyle_roundtrips_dxf() {
    let d = drawing_with_custom();
    let text = write_dxf(&d);
    let back = read_dxf(text.as_bytes()).unwrap();
    // The style is present and equal
    let want = sample_style();
    let got = back.dim_style("Custom").cloned().expect("Custom style present");
    assert_eq!(got.name, "Custom");
    // Compare selective fields (writer may normalize some colors/weights)
    assert_eq!((got.arrow_block, got.arrow_block2, got.decimals, got.linear_unit), ("archtick".into(), "open".into(), 2, 2));
    // The dimension uses it
    let dm = back.model.iter().find_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some(d.clone()) } else { None }).unwrap();
    assert_eq!(dm.style, "Custom");
    // It renders
    let st = back.dim_style(&dm.style).cloned().unwrap();
    let g = draftcraft_render::dimension_geometry(&dm, &st, st.effective_scale(back.header.f64("DIMSCALE", 1.0)));
    assert!(!g.lines.is_empty(), "geometry lines drawn");
}

#[test]
fn user_dimstyle_roundtrips_dwg() {
    let d = drawing_with_custom();
    let bytes = write_with(&d, "x.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    let back = draftcraft_io::read(bytes.as_slice(), "x.dwg").unwrap();
    assert!(back.dim_style("Custom").is_some(), "Custom style present");
    let dm = back.model.iter().find_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some(d.clone()) } else { None }).unwrap();
    assert_eq!(dm.style, "Custom");
}

