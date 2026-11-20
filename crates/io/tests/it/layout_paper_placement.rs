//! A layout read from a file plots its paper where the screen shows it: the drawing sits at the
//! same place on the sheet whether the plot uses the layout's own paper or a plot device's
//! nominal paper (210 x 297 for a file's 209.90 x 297.04, or the long side first) or a device's
//! own printable area: the paper's lower-left corner stays at the layout limits, where the screen
//! draws it.

use draftcraft_doc::geom::Vec2;
use draftcraft_doc::{Common, Drawing, EntityKind, Layout, Line, Space};
use draftcraft_geom::Vec3;
use serde_json::{Value, json};

/// The paper image origin of an AutoCAD-authored file these layouts are modelled after (mm).
const IMAGE_ORIGIN: Vec2 = Vec2 { x: 10.155462934424804, y: -6.351772438031048 };

struct Case {
    name: &'static str,
    /// Paper as the layout stores it (width, height in mm, portrait order) and landscape.
    paper: (f64, f64, bool),
    margins: [f64; 4],
    limits: (Vec2, Vec2),
    /// A plot device's nominal paper, as the Plot dialog sends it (width, height).
    nominal: (f64, f64),
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "A4",
            paper: (209.90277099609375, 297.03887939453125, false),
            margins: [3.175, 3.175, 3.175, 3.175],
            limits: (Vec2::new(-13.330462886741088, 3.176772485714764), Vec2::new(196.57230810935266, 300.215651880246)),
            nominal: (210.0, 297.0),
        },
        Case {
            name: "A3",
            paper: (297.03887939453125, 420.1583251953125, true),
            margins: [3.175, 3.175, 3.175, 3.175],
            limits: (Vec2::new(-13.33045072739355, 3.1767724857147632), Vec2::new(406.82787446791895, 300.215651880246)),
            nominal: (420.0, 297.0),
        },
        Case {
            name: "A1",
            paper: (594.0, 841.0, true),
            margins: [6.35, 14.463889122009277, 6.35, 6.35],
            limits: (Vec2::new(-16.505438520362304, 0.0017725333984799363), Vec2::new(824.4945614796377, 594.0017725333985)),
            nominal: (841.0, 594.0),
        },
    ]
}

/// The layout written to DXF and read back, so it carries the file's LAYOUT groups.
fn drawing(c: &Case) -> Drawing {
    let mut d = Drawing::new_metric();
    let mut l = Layout::new("Sheet", 1);
    l.page.width_mm = c.paper.0;
    l.page.height_mm = c.paper.1;
    l.page.landscape = c.paper.2;
    l.page.margins_mm = c.margins;
    l.page.paper_mm = true;
    l.page.image_origin = Some(IMAGE_ORIGIN);
    l.limits = Some(c.limits);
    d.layouts = vec![l];
    let line = EntityKind::Line(Line { a: Vec3::new(0.0, 20.0, 0.0), b: Vec3::new(50.0, 20.0, 0.0) });
    d.add(&Space::Paper("Sheet".into()), Common::default(), line).unwrap();
    draftcraft_io::read_dxf(draftcraft_io::write_dxf(&d).as_bytes()).unwrap()
}

/// Where the line's start lands on the plotted page, in mm from the paper's lower-left corner.
fn plotted(d: &Drawing, opts: Value) -> Vec2 {
    let mut o = json!({"compress": false, "fit": false});
    if let (Some(dst), Some(src)) = (o.as_object_mut(), opts.as_object()) {
        dst.extend(src.clone());
    }
    let bytes = draftcraft_io::plot(d, &Space::Paper("Sheet".into()), &o).unwrap();
    let doc = crate::pdf_read::read(&bytes);
    let m = crate::pdf_read::drawn(doc.content()).into_iter().find(|o| o.op == "m").expect("the line move-to");
    Vec2::new(m.nums[0], m.nums[1]) * (25.4 / 72.0)
}

#[test]
fn a_layout_plots_its_paper_where_the_screen_shows_it_on_any_paper() {
    for c in cases() {
        let d = drawing(&c);
        let screen = draftcraft_render::paper::sheet(&d, "Sheet").expect("the sheet");
        // The screen draws the paper at the layout limits.
        assert!((screen.origin - c.limits.0).len() < 1e-6, "{}: screen paper at {:?}", c.name, screen.origin);
        // The printable area's corner is the paper image origin's negative.
        assert!((screen.printable.min + IMAGE_ORIGIN).len() < 1e-3, "{}: printable corner {:?}", c.name, screen.printable.min);
        let want = Vec2::new(0.0, 20.0) - screen.origin;
        let own = plotted(&d, json!({}));
        assert!((own - want).len() < 0.01, "{}: own paper {own:?}, want {want:?}", c.name);
        let (w, h) = c.nominal;
        for (w, h) in [(w, h), (h, w)] {
            let nominal = plotted(&d, json!({"width": w, "height": h, "landscape": c.paper.2}));
            assert!((nominal - want).len() < 0.01, "{}: {w} x {h} paper {nominal:?}, want {want:?}", c.name);
        }
        // A plot device's own printable area (5 mm all round) keeps the paper where it was.
        let layout = d.layouts.iter().find(|l| l.name == "Sheet").expect("the layout");
        let device = draftcraft_doc::PageSetup { width_mm: w.min(h), height_mm: w.max(h), margins_mm: [5.0; 4], ..layout.page.clone() };
        let placed = draftcraft_render::paper::sheet_for(&d, "Sheet", &device);
        assert!((placed.origin - screen.origin).len() < 1e-6, "{}: device paper at {:?}", c.name, placed.origin);
        assert!((placed.printable.min - (screen.origin + Vec2::new(5.0, 5.0))).len() < 1e-6, "{}: {:?}", c.name, placed.printable);
    }
}

