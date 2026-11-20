//! User plot style tables offered from a configurable folder.

use serde_json::json;

fn make_ctb_bytes(body: &str) -> Vec<u8> {
    let mut out = Vec::from(b"PIAFILEVERSION_2.0\n"[..].to_vec());
    let mut comp = miniz_oxide::deflate::compress_to_vec_zlib(body.as_bytes(), 6);
    out.append(&mut comp);
    out
}

#[test]
fn user_folder_tables_are_listed_and_persisted_by_name() {
    use draftcraft_engine::cmd::plotters::PlotterStore;
    // Use in-memory plotters to avoid filesystem dependencies in tests not related to plotters.
    draftcraft_engine::cmd::plotters::set_plotter_store(PlotterStore::memory());
    let mut s = draftcraft_engine::Session::new();
    // Point the user plot styles folder at a temp directory.
    let dir = tempfile::tempdir().unwrap();
    s.execute("options", &json!({"PLOTSTYLEPATH": dir.path().to_string_lossy()})).unwrap();
    // Write a minimal CTB mapping and a named STB into the folder.
    std::fs::write(dir.path().join("map.ctb"), make_ctb_bytes(r#"{"kind":"ctb","lineweights":{"1":0.70}}"#)).unwrap();
    let stb = serde_json::to_string(&draftcraft_doc::PlotStyleTable::named(
        "pens.stb",
        "ours",
        vec![draftcraft_doc::PlotStyle { name: "Thick".into(), lineweight: Some(70), ..Default::default() }],
    ))
    .unwrap();
    std::fs::write(dir.path().join("pens.stb"), make_ctb_bytes(&stb)).unwrap();
    // STYLESMANAGER lists them beside built-ins.
    let list = s.execute("stylesmanager", &json!({})).unwrap();
    let tables: Vec<String> = list["tables"].as_array().unwrap().iter().filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_string)).collect();
    assert!(tables.iter().any(|n| n.eq_ignore_ascii_case("map.ctb")));
    assert!(tables.iter().any(|n| n.eq_ignore_ascii_case("pens.stb")));
    // PAGESETUP without a layout reports the names too.
    let report = s.execute("pagesetup", &json!({})).unwrap();
    let names: Vec<String> = report["plotStyleTables"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    assert!(names.iter().any(|n| n.eq_ignore_ascii_case("map.ctb")));
    // Set the layout's table to the user CTB; saving keeps only the name (no XRECORD with tables).
    s.execute("pagesetup", &json!({"layout": "Layout1", "plotStyleTable": "map.ctb"})).unwrap();
    let dxf = draftcraft_io::write_dxf(&s.doc().unwrap());
    assert!(dxf.contains("map.ctb"));
    assert!(!dxf.contains("DRAFTCRAFT_PLOTSTYLES"));
    let back = draftcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    assert_eq!(back.layout("Layout1").unwrap().page.plot_style_table, "map.ctb");
}

