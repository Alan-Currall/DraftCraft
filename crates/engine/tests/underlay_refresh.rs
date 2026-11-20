//! PDF underlays re-render to stay sharp at zoom, and keep the higher resolution once loaded.

use draftcraft_engine::cmd::file::{base64_encode, set_pdf_hooks, PdfHooks};
use draftcraft_engine::cmd::underlay::UNDERLAY_MAX_DPI;
use draftcraft_engine::{Session, doc::Space};
use serde_json::json;

fn sample_pdf_bytes() -> Vec<u8> {
    use draftcraft_doc::{Common, Drawing, EntityKind, Line, Vec3};
    use draftcraft_io::{PdfOptions, pdf};
    // A tiny drawing with one segment; device-independent.
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(100.0, 0.0, 0.0) }))
        .unwrap();
    pdf(&d, &Space::Model, &PdfOptions::default()).unwrap()
}

#[test]
fn refresh_upsamples_on_demand_and_never_downsamples() {
    // Install real PDF hooks.
    set_pdf_hooks(PdfHooks { pages: draftcraft_io::pdf_read::pages, rasterize: draftcraft_io::pdf_read::rasterize, import: draftcraft_io::pdf_read::import });
    let mut s = Session::new();
    // Attach page 1 at default scale with bytes handed over.
    let data = base64_encode(&sample_pdf_bytes());
    let r = s.execute("pdfattach", &json!({ "path": "in-memory.pdf", "data": data })).unwrap();
    assert_eq!(r["page"], 1);
    // Raster key includes the page.
    let key = format!("{}#page={}", r["path"].as_str().unwrap(), r["page"].as_i64().unwrap());
    let (w_pt, _h_pt) = {
        let d = s.doc().unwrap();
        let img = d.model.iter().find_map(|e| if let draftcraft_engine::doc::EntityKind::Image(i) = &e.kind { Some(i.clone()) } else { None }).unwrap();
        (img.size.x, img.size.y)
    };
    // A high zoom: request near the cap; image must grow (or already be at least that big).
    let want_dpi = UNDERLAY_MAX_DPI;
    s.execute("pdfunderlay.refresh", &json!({ "pxPerUnit": want_dpi, "space": "Model" })).unwrap();
    let hi = s.doc().unwrap().rasters.get(&key).map(|r| (r.width(), r.height())).unwrap();
    // Rough lower bound: 80% of theoretical (reader clamps to limits).
    let min_w = ((w_pt / 72.0) * want_dpi * 0.8) as u32;
    assert!(hi.0 >= min_w, "width {} < {}", hi.0, min_w);
    // A lower zoom: no downsampling — keeps the higher-resolution pixels already loaded.
    s.execute("pdfunderlay.refresh", &json!({ "pxPerUnit": 96.0, "space": "Model" })).unwrap();
    let after = s.doc().unwrap().rasters.get(&key).map(|r| (r.width(), r.height())).unwrap();
    assert_eq!(after, hi);
}

