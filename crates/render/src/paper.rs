//! Paper sizes and the layout sheet (paper rectangle, printable area) for paper space.
//!
//! Paper-space units are millimetres in metric drawings (`MEASUREMENT` = 1) and on layouts whose
//! page setup is in millimetres, and inches otherwise. A layout read from a file may place the
//! paper off the paper-space origin: its lower-left corner sits at the layout limits (or, when
//! those do not match the paper, at the negative of the paper image origin less the margins).

use draftcraft_doc::{Drawing, PageSetup};
use draftcraft_geom::{Bounds2, Vec2};

/// Maximum model-space plot offset accepted by the PLOT command, in millimetres.
pub const MAX_PLOT_OFFSET_MM: f64 = 100_000.0;

/// A standard paper size (portrait: `width_mm <= height_mm`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaperSize {
    /// Canonical media name, as shown in page setup.
    pub name: &'static str,
    /// Short names accepted by commands (`A4`, `Letter`, `ANSI B`…).
    pub aliases: &'static [&'static str],
    pub width_mm: f64,
    pub height_mm: f64,
    /// Inch-based (ANSI/ARCH) rather than ISO.
    pub imperial: bool,
}

const fn iso(name: &'static str, aliases: &'static [&'static str], w: f64, h: f64) -> PaperSize {
    PaperSize { name, aliases, width_mm: w, height_mm: h, imperial: false }
}
const fn inch(name: &'static str, aliases: &'static [&'static str], w: f64, h: f64) -> PaperSize {
    PaperSize { name, aliases, width_mm: w * 25.4, height_mm: h * 25.4, imperial: true }
}

/// Common ISO (A0–A6, B0–B5) and ANSI/ARCH sheet sizes (ISO 216, ANSI/ASME Y14.1).
pub const PAPER_SIZES: &[PaperSize] = &[
    iso("ISO A0 (841.00 x 1189.00 MM)", &["A0", "ISO A0"], 841.0, 1189.0),
    iso("ISO A1 (594.00 x 841.00 MM)", &["A1", "ISO A1"], 594.0, 841.0),
    iso("ISO A2 (420.00 x 594.00 MM)", &["A2", "ISO A2"], 420.0, 594.0),
    iso("ISO A3 (297.00 x 420.00 MM)", &["A3", "ISO A3"], 297.0, 420.0),
    iso("ISO A4 (210.00 x 297.00 MM)", &["A4", "ISO A4"], 210.0, 297.0),
    iso("ISO A5 (148.00 x 210.00 MM)", &["A5", "ISO A5"], 148.0, 210.0),
    iso("ISO A6 (105.00 x 148.00 MM)", &["A6", "ISO A6"], 105.0, 148.0),
    iso("ISO B0 (1000.00 x 1414.00 MM)", &["B0", "ISO B0"], 1000.0, 1414.0),
    iso("ISO B1 (707.00 x 1000.00 MM)", &["B1", "ISO B1"], 707.0, 1000.0),
    iso("ISO B2 (500.00 x 707.00 MM)", &["B2", "ISO B2"], 500.0, 707.0),
    iso("ISO B3 (353.00 x 500.00 MM)", &["B3", "ISO B3"], 353.0, 500.0),
    iso("ISO B4 (250.00 x 353.00 MM)", &["B4", "ISO B4"], 250.0, 353.0),
    iso("ISO B5 (176.00 x 250.00 MM)", &["B5", "ISO B5"], 176.0, 250.0),
    inch("ANSI A (8.50 x 11.00 Inches)", &["ANSI A", "Letter", "A-size"], 8.5, 11.0),
    inch("Legal (8.50 x 14.00 Inches)", &["Legal"], 8.5, 14.0),
    inch("ANSI B (11.00 x 17.00 Inches)", &["ANSI B", "Tabloid", "Ledger", "B-size"], 11.0, 17.0),
    inch("ANSI C (17.00 x 22.00 Inches)", &["ANSI C", "C-size"], 17.0, 22.0),
    inch("ANSI D (22.00 x 34.00 Inches)", &["ANSI D", "D-size"], 22.0, 34.0),
    inch("ANSI E (34.00 x 44.00 Inches)", &["ANSI E", "E-size"], 34.0, 44.0),
    inch("ARCH A (9.00 x 12.00 Inches)", &["ARCH A"], 9.0, 12.0),
    inch("ARCH B (12.00 x 18.00 Inches)", &["ARCH B"], 12.0, 18.0),
    inch("ARCH C (18.00 x 24.00 Inches)", &["ARCH C"], 18.0, 24.0),
    inch("ARCH D (24.00 x 36.00 Inches)", &["ARCH D"], 24.0, 36.0),
    inch("ARCH E1 (30.00 x 42.00 Inches)", &["ARCH E1"], 30.0, 42.0),
    inch("ARCH E (36.00 x 48.00 Inches)", &["ARCH E"], 36.0, 48.0),
];

impl PaperSize {
    /// Margins (left, bottom, right, top; mm) of the printable area the built-in plot devices
    /// give this size: 5 mm on ISO sheets, a quarter inch on inch sheets.
    pub fn margins_mm(&self) -> [f64; 4] {
        if self.imperial { [6.35; 4] } else { [5.0; 4] }
    }
}

/// Look up a paper size by canonical name or alias (case- and space-insensitive).
pub fn paper_size(name: &str) -> Option<&'static PaperSize> {
    let norm = |s: &str| s.chars().filter(|c| !c.is_whitespace() && *c != '_' && *c != '-').collect::<String>().to_ascii_lowercase();
    let n = norm(name);
    if n.is_empty() {
        return None;
    }
    PAPER_SIZES.iter().find(|p| norm(p.name) == n || p.aliases.iter().any(|a| norm(a) == n))
}

/// Millimetres per paper-space unit for a drawing (1 for metric, 25.4 for imperial).
pub fn paper_unit_mm(d: &Drawing) -> f64 {
    if d.header.i64("MEASUREMENT", 0) == 1 { 1.0 } else { 25.4 }
}

/// Millimetres per paper-space unit on a layout with page setup `page`: millimetres when the
/// page says so (plot paper units mm), else as the drawing's MEASUREMENT says. Drawings from
/// AutoCAD often have MEASUREMENT 0 with layouts in millimetres (#1058).
pub fn page_unit_mm(d: &Drawing, page: &PageSetup) -> f64 {
    if page.paper_mm { 1.0 } else { paper_unit_mm(d) }
}

/// Millimetres per paper-space unit on the layout holding the paper-space entity `h` (the
/// drawing's when no layout holds it).
pub fn unit_mm_of(d: &Drawing, h: draftcraft_doc::Handle) -> f64 {
    d.layouts.iter().find(|l| l.entities.contains(h)).map_or_else(|| paper_unit_mm(d), |l| page_unit_mm(d, &l.page))
}

/// The paper sheet of a layout, in paper-space units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sheet {
    /// Sheet size (already rotated for landscape). The sheet spans `origin..origin + size`.
    pub size: Vec2,
    /// Printable area (inside the margins).
    pub printable: Bounds2,
    /// Millimetres per paper-space unit.
    pub unit_mm: f64,
    /// The paper's lower-left corner in paper space.
    pub origin: Vec2,
}

impl Sheet {
    pub fn bounds(&self) -> Bounds2 {
        Bounds2::new(self.origin, self.origin + self.size)
    }
    /// The sheet for a page setup in a drawing whose paper units are `unit_mm` millimetres.
    pub fn from_page(page: &PageSetup, unit_mm: f64) -> Sheet {
        let sane = |v: f64, d: f64| if v.is_finite() && v > 0.0 { v.min(100_000.0) } else { d };
        let w = sane(page.width_mm, 215.9);
        let h = sane(page.height_mm, 279.4);
        let (w, h) = if page.landscape { (w.max(h), w.min(h)) } else { (w.min(h), w.max(h)) };
        let u = if unit_mm.is_finite() && unit_mm > 0.0 { unit_mm } else { 1.0 };
        let size = Vec2::new(w / u, h / u);
        // Margins: left, bottom, right, top (DXF group codes 40..43). Rotate for landscape so
        // they stay with the physical media after the width/height swap above.
        let raw = page.margins_mm.map(|v| if v.is_finite() && v >= 0.0 { v } else { 0.0 });
        let [ml, mb, mr, mt] = raw;
        let (l_mm, b_mm, r_mm, t_mm) = if page.landscape { (mt, ml, mb, mr) } else { (ml, mb, mr, mt) };
        let (l, b, r, t) = (l_mm / u, b_mm / u, r_mm / u, t_mm / u);
        let printable = if l + r < size.x && b + t < size.y {
            Bounds2::new(Vec2::new(l, b), Vec2::new(size.x - r, size.y - t))
        } else {
            Bounds2::new(Vec2::ZERO, size)
        };
        Sheet { size, printable, unit_mm: u, origin: Vec2::ZERO }
    }
}

/// The sheet of a named layout (`None` if there is no such layout).
pub fn sheet(d: &Drawing, layout: &str) -> Option<Sheet> {
    let l = d.layouts.iter().find(|l| l.name == layout).or_else(|| d.layout(layout))?;
    Some(sheet_for(d, &l.name, &l.page))
}

/// The sheet of `page` as placed by the layout `layout`: the paper's lower-left corner where the
/// file placed it. A layout authored here (no image origin and no limits) stays at the origin.
pub fn sheet_for(d: &Drawing, layout: &str, page: &PageSetup) -> Sheet {
    let u = page_unit_mm(d, page);
    let mut s = Sheet::from_page(page, u);
    // Paper image origin, in page units; None means a page DraftCraft made (render at origin).
    let image_origin = page.image_origin;
    // Limits from the layout, if they look like the paper (within half a mm per side).
    let limits_match_sheet = |lo: Vec2, hi: Vec2| {
        let near = |a: f64, b: f64| (a - b).abs() <= 0.5 / u;
        near(hi.x - lo.x, s.size.x) && near(hi.y - lo.y, s.size.y)
    };
    // Prefer layout limits when they match the paper size.
    let origin_from_limits = d
        .layouts
        .iter()
        .find(|l| l.name.eq_ignore_ascii_case(layout))
        .and_then(|l| {
            l.limits.and_then(|(lo, hi)| {
                if limits_match_sheet(lo, hi) {
                    Some(lo)
                } else {
                    None
                }
            })
        });
    let origin = if let Some(o) = origin_from_limits {
        o
    } else if let Some(io) = image_origin {
        // Offset by the negative image origin less margins (left/bottom).
            let [l, b, _, _] = page.margins_mm;
            Vec2::ZERO - io / u - Vec2::new(l / u, b / u)
    } else {
        // A layout authored here: keep the paper at the origin.
        Vec2::ZERO
    };
    // Guard against hostile values: keep the default-at-origin sheet if origin is not sane.
    if !(origin.x.is_finite() && origin.y.is_finite()) || origin.x.abs() > 1.0e12 || origin.y.abs() > 1.0e12 {
        return s;
    }
    s.origin = origin;
    // Move the printable area by the same offset.
    s.printable = Bounds2::new(s.printable.min + origin, s.printable.max + origin);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_sheet() {
        assert_eq!(paper_size("a4").map(|p| p.width_mm), Some(210.0));
        assert_eq!(paper_size("ansi b").map(|p| p.height_mm), Some(17.0 * 25.4));
        assert_eq!(paper_size("Letter").map(|p| p.name), Some("ANSI A (8.50 x 11.00 Inches)"));
        assert!(paper_size("").is_none());
        assert!(paper_size("nope").is_none());
        let page = PageSetup { width_mm: 210.0, height_mm: 297.0, margins_mm: [5.0; 4], landscape: true, ..PageSetup::default() };
        let s = Sheet::from_page(&page, 1.0);
        assert_eq!(s.size, Vec2::new(297.0, 210.0));
        assert_eq!(s.printable, Bounds2::new(Vec2::new(5.0, 5.0), Vec2::new(292.0, 205.0)));
        let hostile = PageSetup { width_mm: f64::NAN, height_mm: -1.0, margins_mm: [1e9; 4], ..PageSetup::default() };
        let s = Sheet::from_page(&hostile, 0.0);
        assert!(s.size.x > 0.0 && s.printable == s.bounds());
    }
}

/// What a layout's saved page setup says about how to plot it, for a plot that was not given the
/// matching options (#2493): the plot window, whether to scale to fit, the saved scale, and
/// centring.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SavedPlot {
    /// The saved plot window, when the plot area is "window" and its corners are a real box.
    pub window: Option<Bounds2>,
    /// Scale to fit the window (or the extents) on the printable area.
    pub fit: bool,
    /// The saved plot scale, paper units per drawing unit, when it isn't scale to fit or 1:1.
    pub scale: Option<f64>,
    /// Centre the plot on the printable area.
    pub center: bool,
}

/// The saved plot settings of `page`. A layout plotted over its whole sheet ("layout" plot area)
/// has nothing to fit, so scale to fit only counts for a window or the extents.
pub fn saved_plot(page: &PageSetup) -> SavedPlot {
    let [x1, y1, x2, y2] = page.plot_window;
    let window = (page.plot_area.eq_ignore_ascii_case("window") && [x1, y1, x2, y2].iter().all(|v| v.is_finite()))
        .then(|| Bounds2::new(Vec2::new(x1, y1), Vec2::new(x2, y2)))
        .filter(|w| w.width() > 0.0 && w.height() > 0.0);
    let fit = page.scale_to_fit && (window.is_some() || page.plot_area.eq_ignore_ascii_case("extents"));
    let scale = Some(page.scale).filter(|s| !page.scale_to_fit && s.is_finite() && *s > 0.0 && (*s - 1.0).abs() > 1e-9);
    SavedPlot { window, fit, scale, center: page.center }
}
