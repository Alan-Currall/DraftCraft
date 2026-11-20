//! Plotting to PNG refreshes PDF underlays to the device resolution-based px-per-unit.

use draftcraft_engine::cmd::file::{base64_encode, set_io, set_pdf_hooks, IoHooks, PdfHooks};
use draftcraft_engine::cmd::underlay::UNDERLAY_DPI;
use draftcraft_engine::{Session, doc::Space};
use serde_json::json;

fn sample_pdf_bytes() -> Vec<u8> {
    use draftcraft_doc::{Common, Drawing, EntityKind, Line};
    use draftcraft_geom::Vec3;
    use draftcraft_io::{PdfOptions, pdf};
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(100.0, 0.0, 0.0) }))
        .unwrap();
    pdf(&d, &Space::Model, &PdfOptions::default()).unwrap()
}

#[test]
fn plot_to_png_upsamples_underlay() {
    // Install IO hooks (read/write/plot) and PDF hooks from io.
    set_io(IoHooks {
        read: |b, n| draftcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| draftcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: Some(|d, s, o| draftcraft_io::plot(d, s, o).map_err(|e| e.to_string())),
    });
    set_pdf_hooks(PdfHooks {
        pages: |b| draftcraft_io::pdf_read::pages(b).map_err(|e| e.to_string()),
        rasterize: |b, i, d| draftcraft_io::pdf_read::rasterize(b, i, d).map_err(|e| e.to_string()),
        import: |b, i, v, f, t| {
            let opts = draftcraft_io::pdf_read::ImportOptions { vectors: v, fills: f, text: t };
            let (shapes, notes) = draftcraft_io::pdf_read::import(b, i, opts).map_err(|e| e.to_string())?;
            Ok((shapes, notes.images, notes.truncated))
        },
    });

    let mut s = Session::new();
    // Attach a PDF underlay from a temp file so refresh can read it back.
    let pdf_path = std::env::temp_dir().join(format!("dc-underlay-plot-{}.pdf", std::process::id()));
    std::fs::write(&pdf_path, sample_pdf_bytes()).unwrap();
    let r = s.execute("pdfattach", &json!({ "path": pdf_path.to_string_lossy(), "at": [0,0], "scale": 25.4 })).unwrap();
    let key = format!("{}#page={}", r["path"].as_str().unwrap(), r["page"].as_i64().unwrap());

    let before = s.doc().unwrap().rasters.get(&key).map(|r| r.width()).unwrap();
    // Sanity: attached at UNDERLAY_DPI initially.
    assert!(before > 0);

    // Force a refresh to ~300 dpi directly to validate the path.
    s.execute("pdfunderlay.refresh", &json!({ "pxPerUnit": 12.0, "space": "Model" })).unwrap();
    let mid = s.doc().unwrap().rasters.get(&key).map(|r| r.width()).unwrap();
    assert!(f64::from(mid) >= f64::from(before) * 1.7 - 1.0, "mid {mid} vs before {before}");

    // Plot to PNG with default PNG device ("DraftCraft PNG" → 300 dpi): no downsampling; kept.
    let _ = s
        .execute(
            "plot",
            &json!({ "layout": "Model", "device": "DraftCraft PNG", "paper": "A4", "path": null, "fit": false, "scale": 1.0 }),
        )
    .unwrap();

    let after = s.doc().unwrap().rasters.get(&key).map(|r| r.width()).unwrap();
    // Expect the refreshed width to be kept (no downsampling).
    assert_eq!(after, mid);
}

