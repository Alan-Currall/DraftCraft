//! Plot dialog "Apply to Layout" saves all plot settings in the layout's page setup,
//! and they persist through DXF and DWG round trips.
//!
//! This exercises persistence of the settings "Apply to Layout" writes via the `pagesetup` command:
//! - device/printer
//! - paper size and orientation
//! - scale/fit and centering
//! - plot options: lineweights and plot style table
//! Transparency is not a DXF plot flag and does not persist across DXF by design.

use draftcraft_doc::{Drawing, Space};

fn drawing_with_layout_settings() -> Drawing {
    // Metric drawing with one layout.
    let mut d = Drawing::new_metric();
    // A page setup as the Plot dialog would apply to the layout.
    {
        let p = &mut d.layouts[0].page;
        p.device = "DraftCraft PNG".into(); // a built-in device
        p.paper = "ANSI A (8.50 x 11.00 Inches)".into();
        p.landscape = true;
        p.scale_to_fit = false;
        p.scale = 2.0; // 2 paper units per drawing unit
        p.center = true;
        p.lineweights = false;
        p.plot_style_table = "monochrome.ctb".into();
        // Transparency is intentionally left alone: DXF has no flag for it.
    }
    d
}

#[test]
fn layout_plot_settings_round_trip_through_dxf() {
    let d = drawing_with_layout_settings();
    let text = draftcraft_io::write_dxf(&d);
    let back = draftcraft_io::read_dxf(text.as_bytes()).unwrap();
    let l = back.layout("Layout1").unwrap();
    let p = &l.page;
    assert_eq!(p.device, "DraftCraft PNG");
    assert_eq!(p.paper, "ANSI A (8.50 x 11.00 Inches)");
    assert!(p.landscape);
    assert!(!p.scale_to_fit);
    assert_eq!(p.scale, 2.0);
    assert!(p.center);
    assert!(!p.lineweights);
    assert_eq!(p.plot_style_table, "monochrome.ctb");
    // Layouts always plot the whole sheet; the area stays "layout".
    assert_eq!(p.plot_area, "layout");
    // Transparency is not saved in DXF; a fresh read defaults it to false.
    assert!(!p.plot_transparency);
    // A layout plots 1:1; the plotter applies the page setup for the PNG/PDF/SVG outputs.
    let bytes = draftcraft_io::plot(&back, &Space::Paper("Layout1".into()), &serde_json::json!({})).unwrap();
    assert!(!bytes.is_empty(), "the plot succeeded");
}

#[test]
fn layout_plot_settings_round_trip_through_dwg() {
    use draftcraft_io::{DwgReader, Version, read_dwg_with, write_dwg_own};
    let d = drawing_with_layout_settings();
    // Pick a modern DWG version that the writer supports fully.
    let (dwg, _unwritten) = write_dwg_own(&d, Version::R2018).unwrap();
    let back = read_dwg_with(&dwg, DwgReader::Direct).unwrap();
    let l = back.layout("Layout1").unwrap();
    let p = &l.page;
    assert_eq!(p.device, "DraftCraft PNG");
    assert_eq!(p.paper, "ANSI A (8.50 x 11.00 Inches)");
    assert!(p.landscape);
    assert!(!p.scale_to_fit);
    assert_eq!(p.scale, 2.0);
    assert!(p.center);
    assert!(!p.lineweights);
    assert_eq!(p.plot_style_table, "monochrome.ctb");
}

