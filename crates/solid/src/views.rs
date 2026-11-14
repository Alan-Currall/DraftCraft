//! Drawings from solids (FLATSHOT and SECTION): exact hidden lines through SolidCraft's `hlr`,
//! and cross-sections through a slice and the cut face's exact edges.
//!
//! The hidden-line code works on SolidCraft's portable B-rep record, which the kernel's public
//! API hands out only inside its native body format: [`portable`] reads that format's tagged
//! values (a public, documented layout: a tag byte, then fixed-size little-endian numbers or a
//! u32 length) back into the record. See docs/solids.md, "Open questions".

use draftcraft_geom::{Vec2, Vec3};
use serde_json::{Map, Number, Value};
use solidcraft_geom as sg;
use solidcraft_geom::hlr;

use crate::body::{Body, Frame, Result, Seg, SolidError, from_seg2, to_sg};

fn err(m: impl Into<String>) -> SolidError {
    SolidError(m.into())
}

/// How a piece of a projected line is seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Visible,
    Hidden,
    /// The hidden-line code could not decide (a freeform face in the way): reported, not guessed.
    Uncertain,
}

/// One piece of a projected line, in the view's 2D coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewPiece {
    pub seen: Seen,
    /// Lines and arcs where the projection is exact, short lines (chords) elsewhere.
    pub segs: Vec<Seg>,
    /// Whether this piece is a silhouette of a curved face rather than an edge.
    pub silhouette: bool,
    /// Whether the piece is exact (lines and arcs) rather than chords of a curve.
    pub exact: bool,
}

/// A projected view of solids: what FLATSHOT draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub pieces: Vec<ViewPiece>,
    /// Faces outside the subset the hidden-line code handles exactly: their silhouettes are
    /// missing and what they might hide is reported uncertain.
    pub unsupported_faces: usize,
}

/// Most bodies in one view.
pub const MAX_VIEW_BODIES: usize = 10_000;

/// The view of `bodies` looking along `direction`, with `up` pointing up on the paper. The view's
/// 2D coordinates are `(p · x, p · y)` with `x = direction × up` and `y = x × direction`.
pub fn project(bodies: &[&Body], direction: Vec3, up: Vec3) -> Result<View> {
    if bodies.len() > MAX_VIEW_BODIES {
        return Err(err("too many solids for one view"));
    }
    let view = hlr::View::new(to_sg(direction), to_sg(up)).map_err(|e| err(e.to_string()))?;
    let opts = hlr::Options { tangent_edges: false, ..hlr::Options::default() };
    let mut defs = Vec::with_capacity(bodies.len());
    for b in bodies {
        let brep = portable(b)?;
        defs.push(hlr::prepare(&brep, &opts).map_err(|e| err(format!("hidden lines: {e}")))?);
    }
    let id = [sg::Vec3::new(1.0, 0.0, 0.0), sg::Vec3::new(0.0, 1.0, 0.0), sg::Vec3::new(0.0, 0.0, 1.0)];
    let scene: Vec<hlr::Placement> = (0..defs.len()).map(|i| hlr::Placement { body: i, rotation: id, translation: sg::Vec3::ZERO }).collect();
    let drawing = hlr::hidden_lines(&defs, &scene, &view, &opts, &|| false).map_err(|e| err(format!("hidden lines: {e}")))?;
    let mut out = View { pieces: Vec::new(), unsupported_faces: drawing.unsupported.len() };
    for line in &drawing.lines {
        let silhouette = matches!(line.source, hlr::Source::Silhouette { .. });
        for p in line.pieces.iter().filter(|p| !p.covered) {
            let seen = match p.visibility {
                hlr::Visibility::Visible => Seen::Visible,
                hlr::Visibility::Hidden => Seen::Hidden,
                hlr::Visibility::Uncertain => Seen::Uncertain,
            };
            let mut exact = matches!(p.accuracy, sg::section::CurveAccuracy::Analytic);
            let mut segs = Vec::with_capacity(p.geometry.len());
            for g in &p.geometry {
                match from_seg2(*g) {
                    Some(s) => segs.push(s),
                    None => {
                        // A conic or spline piece: drawn as chords of it.
                        exact = false;
                        let pts = g.polyline(1e-3);
                        segs.extend(pts.windows(2).filter_map(|w| Some(Seg::Line { a: v2(*w.first()?), b: v2(*w.get(1)?) })));
                    }
                }
            }
            if !segs.is_empty() {
                out.pieces.push(ViewPiece { seen, segs, silhouette, exact });
            }
        }
    }
    Ok(out)
}

fn v2(p: sg::Vec2) -> Vec2 {
    Vec2::new(p.x, p.y)
}

/// The cross-section of `body` by the plane `frame`: closed loops in the frame's coordinates,
/// outer boundaries and holes alike (empty when the plane misses the body).
pub fn section(body: &Body, frame: &Frame) -> Result<Vec<Vec<Seg>>> {
    let (part, _) = crate::body::slice(body, frame.origin, frame.normal())?;
    let Some(part) = part else { return Ok(Vec::new()) };
    let size = part.size().max(1e-9);
    let tol = size * 1e-6;
    let normal = frame.normal();
    let on_plane: Vec<usize> = part
        .edge_polylines(size / 2000.0)?
        .into_iter()
        .filter(|(_, pts)| !pts.is_empty() && pts.iter().all(|p| (*p - frame.origin).dot(normal).abs() <= tol * 10.0))
        .map(|(i, _)| i)
        .collect();
    let mut segs = Vec::new();
    for (i, e) in part.planar_edges(frame, &on_plane)?.into_iter().enumerate() {
        let edge = on_plane.get(i).copied().unwrap_or(0);
        segs.extend(e.ok_or_else(|| err(format!("section: edge {edge} of the cut is not a line or an arc")))?);
    }
    chain(segs, tol * 100.0)
}

/// Most segments chained into loops.
const MAX_CHAIN: usize = 200_000;

/// Join segments end to start into closed loops.
fn chain(mut segs: Vec<Seg>, tol: f64) -> Result<Vec<Vec<Seg>>> {
    if segs.len() > MAX_CHAIN {
        return Err(err("section: too many edges"));
    }
    let mut loops = Vec::new();
    while let Some(first) = segs.pop() {
        let start = first.start();
        let mut lp = vec![first];
        let mut end = first.end();
        while end.dist(start) > tol {
            let next = segs.iter().position(|s| s.start().dist(end) <= tol || s.end().dist(end) <= tol);
            let Some(i) = next else { return Err(err("section: the cut's edges don't close")) };
            let s = segs.swap_remove(i);
            let s = if s.start().dist(end) <= tol { s } else { s.reversed() };
            end = s.end();
            lp.push(s);
        }
        loops.push(lp);
    }
    Ok(loops)
}

/// The portable B-rep record of a body, read back from its native encoding.
fn portable(b: &Body) -> Result<sg::brep::Brep> {
    let bytes = b.encode()?;
    let body = bytes.get(8..).ok_or_else(|| err("native body: header"))?;
    let mut input = body;
    let mut budget = 20_000_000usize;
    let value = read(&mut input, 0, &mut budget)?;
    let Value::Object(mut map) = value else { return Err(err("native body: not a record")) };
    let solid = map.remove("solid").ok_or_else(|| err("native body: no solid"))?;
    serde_json::from_value(solid).map_err(|e| err(format!("native body: {e}")))
}

fn take<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    let head = input.get(..n).ok_or_else(|| err("native body: truncated"))?;
    *input = input.get(n..).ok_or_else(|| err("native body: truncated"))?;
    Ok(head)
}

fn u32_len(input: &mut &[u8]) -> Result<usize> {
    let raw: [u8; 4] = take(input, 4)?.try_into().map_err(|_| err("native body: length"))?;
    usize::try_from(u32::from_le_bytes(raw)).map_err(|_| err("native body: length"))
}

fn text(input: &mut &[u8]) -> Result<String> {
    let n = u32_len(input)?;
    if n > 1 << 20 {
        return Err(err("native body: string too long"));
    }
    std::str::from_utf8(take(input, n)?).map(str::to_owned).map_err(|_| err("native body: text"))
}

/// One tagged value: 0 null, 1/2 false/true, 3 i64, 4 u64, 5 f64, 6 string, 7 array, 8 map.
fn read(input: &mut &[u8], depth: usize, budget: &mut usize) -> Result<Value> {
    if depth > 48 || *budget == 0 {
        return Err(err("native body: too deep"));
    }
    *budget -= 1;
    let tag = *take(input, 1)?.first().ok_or_else(|| err("native body: tag"))?;
    Ok(match tag {
        0 => Value::Null,
        1 => Value::Bool(false),
        2 => Value::Bool(true),
        3..=5 => {
            let raw: [u8; 8] = take(input, 8)?.try_into().map_err(|_| err("native body: number"))?;
            Value::Number(match tag {
                3 => Number::from(i64::from_le_bytes(raw)),
                4 => Number::from(u64::from_le_bytes(raw)),
                _ => Number::from_f64(f64::from_le_bytes(raw)).ok_or_else(|| err("native body: non-finite number"))?,
            })
        }
        6 => Value::String(text(input)?),
        7 | 8 => {
            let n = u32_len(input)?;
            if n > input.len() || n > *budget {
                return Err(err("native body: collection too large"));
            }
            if tag == 7 {
                let mut a = Vec::with_capacity(n);
                for _ in 0..n {
                    a.push(read(input, depth + 1, budget)?);
                }
                Value::Array(a)
            } else {
                let mut m = Map::new();
                for _ in 0..n {
                    let k = text(input)?;
                    let v = read(input, depth + 1, budget)?;
                    m.insert(k, v);
                }
                Value::Object(m)
            }
        }
        _ => return Err(err("native body: unknown tag")),
    })
}
