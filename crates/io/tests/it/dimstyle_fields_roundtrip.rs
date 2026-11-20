use draftcraft_color::Color;
use draftcraft_doc::{Common, DimKind, DimStyle, Dimension, Drawing, Entity, EntityKind, Space, Vec3};
use draftcraft_io::{SaveOptions, Version, read_dxf, write_dxf, write_with};

fn styled() -> DimStyle {
    DimStyle {
        name: "Fields".into(),
        // Colours
        dim_line_color: Color::Index(3),
        ext_line_color: Color::ByLayer,
        text_color: Color::ByBlock,
        // Linetypes
        dim_linetype: "DASHED".into(),
        ext_linetype1: "ByLayer".into(),
        ext_linetype2: String::new(), // ByBlock
        // Leader arrow
        leader_arrow_block: "dot".into(),
        // Text fill
        text_fill: 2,
        text_fill_color: Color::Index(5),
        // Suppress halves
        suppress_dim1: true,
        suppress_dim2: false,
        // Zero suppression & fraction
        zero_suppression: 1 | 2 | 4, // leading + trailing + zero feet
        fraction_format: 1,          // diagonal
        // Alternate units zero suppression too
        alt: true,
        alt_zero_suppression: 2 | 8, // trailing + zero inches
        ..DimStyle::default()
    }
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    // Ensure linetypes referenced by the style exist in the table
    let lib = draftcraft_doc::library::standard_linetypes();
    for lt in lib {
        if (lt.name.eq_ignore_ascii_case("DASHED") || lt.name.eq_ignore_ascii_case("ByLayer")) && d.linetype(&lt.name).is_none() {
            d.linetypes.push(lt);
        }
    }
    let st = styled();
    d.dim_styles.push(st.clone());
    d.header.set_str("DIMSTYLE", &st.name);
    // Dimension using the style so writer keeps related table data paths warm
    let dm = Dimension {
        style: st.name.clone(),
        text: "<>".into(),
        kind: DimKind::Linear { rotation: 0.0 },
        p13: Vec3::new(0.0, 0.0, 0.0),
        p14: Vec3::new(10.0, 0.0, 0.0),
        defpt: Vec3::new(5.0, 2.0, 0.0),
        p15: Vec3::ZERO,
        assoc: Vec::new(),
        overrides: serde_json::Map::new(),
        text_mid: Vec3::new(5.0, 2.0, 0.0),
        user_text_pos: false,
        text_rotation: 0.0,
        block: None,
    };
    d.model.push(Entity { handle: d.alloc_handle(), common: Common::default(), kind: EntityKind::Dimension(dm) });
    d
}

#[test]
fn dimstyle_fields_roundtrip_dxf() {
    let d = drawing();
    let text = write_dxf(&d);
    let back = read_dxf(text.as_bytes()).unwrap();
    let got = back.dim_style("Fields").cloned().unwrap();
    // Colours
    assert_eq!(got.dim_line_color, Color::Index(3));
    assert!(matches!(got.ext_line_color, Color::ByLayer));
    assert!(matches!(got.text_color, Color::ByBlock));
    // Linetypes and leader arrow
    assert_eq!(got.dim_linetype, "DASHED");
    assert!(got.ext_linetype1.eq_ignore_ascii_case("ByLayer"));
    assert_eq!(got.ext_linetype2, "");
    assert_eq!(got.leader_arrow_block.to_ascii_lowercase(), "dot");
    // Text fill
    assert_eq!((got.text_fill, got.text_fill_color), (2, Color::Index(5)));
    // Suppress halves
    assert!(got.suppress_dim1 && !got.suppress_dim2);
    // Zero suppression and fraction format
    assert_eq!(got.zero_suppression & (1 | 2 | 4), 1 | 2 | 4);
    assert_eq!(got.fraction_format, 1);
    assert!(got.alt && got.alt_zero_suppression & (2 | 8) == 2 | 8);
}

#[test]
fn dimstyle_fields_roundtrip_dwg() {
    let d = drawing();
    let bytes = write_with(&d, "x.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    let back = draftcraft_io::read(bytes.as_slice(), "x.dwg").unwrap();
    let got = back.dim_style("Fields").cloned().unwrap();
    // Core fields should persist in DWG too
    assert_eq!(got.dim_linetype, "DASHED");
    assert_eq!(got.leader_arrow_block.to_ascii_lowercase(), "dot");
    assert_eq!((got.text_fill, got.text_fill_color), (2, Color::Index(5)));
}

