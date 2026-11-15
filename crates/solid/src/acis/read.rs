//! ACIS data as kernel bodies.
//!
//! `cadmpeg-asm` (Apache-2.0) frames the records, whatever the dialect (Spatial's ACIS save
//! formats, Autodesk's ShapeManager ones; text or binary), and decodes them into a neutral
//! B-rep: bodies to points, analytic surfaces and curves, NURBS ones from their caches, every
//! length in millimetres and every edge running along its curve. That B-rep is written as STEP
//! topology (`solidcore_step::write`'s entity writer) and read back by solidcore's STEP reader,
//! which works out what ACIS leaves to the modeller: pcurves, the seams of faces that go all the
//! way round, and the tolerances. Each of the data's solids becomes one body.
//!
//! The data comes from a file: every step is checked, and a solid that can't be read is an
//! error with the reason, never a panic.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use serde_json::Value;
use solidcore_ops as ops;
use solidcore_ops::pcurve::{EdgeUse, TrimFace, Trims};
use solidcore_record::brep::Curve2;
use solidcore_step::geom::{BSplineCurve, BSplineSurface, Curve, CurveForm, KnotSpec, Logical, P3, Placement, Surface, SurfaceForm};
use solidcore_step::p21::Param;
use solidcore_step::write::{GeomWriter, Writer};

use super::model::{SAB_MAGIC, scramble};
use crate::body::{Body, Result, SolidError};

fn err(m: impl Into<String>) -> SolidError {
    SolidError(m.into())
}

/// Largest ACIS data accepted for one entity.
pub const MAX_DATA: usize = 256 << 20;
/// Most faces read from one entity's data.
const MAX_FACES: usize = 200_000;

/// The kernel bodies of one entity's ACIS data: SAT text (as the entity's groups hold it,
/// scrambled or not) or SAB.
pub fn read(data: &[u8]) -> Result<Vec<Body>> {
    if data.is_empty() || data.len() > MAX_DATA {
        return Err(err("the solid's ACIS data is empty or too large"));
    }
    let brep = if data.starts_with(SAB_MAGIC) || data.starts_with(b"ASM BinaryFile") { decode_binary(data)? } else { decode_text(data)? };
    let (value, scale) = brep;
    to_bodies(&value, scale)
}

/// The text as SAT: unscrambled when it doesn't start with the save format's number.
fn plain_text(data: &[u8]) -> Vec<u8> {
    let text: String = data.iter().map(|b| char::from(*b)).collect();
    let starts_plain = text.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit());
    let text = if starts_plain { text } else { scramble(&text) };
    text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect()
}

fn decode_error(e: impl std::fmt::Display) -> SolidError {
    err(format!("the solid's ACIS data can't be read ({e})"))
}

fn format_id() -> Result<cadmpeg_asm::ids::IdFormat> {
    cadmpeg_asm::ids::IdFormat::from_literal("dwg").ok_or_else(|| err("the ACIS reader is not set up"))
}

/// Where a binary stream's end marker (`End-of-ACIS-data` or `End-of-ASM-data`, its name parts
/// with no record terminator after them) starts, at or after `start`.
fn end_marker(data: &[u8], start: usize) -> Option<usize> {
    const HEAD: &[u8] = b"\x0e\x03End\x0e\x02of\x0e";
    let rest = data.get(start..)?;
    rest.windows(HEAD.len()).enumerate().find_map(|(i, w)| {
        if w != HEAD {
            return None;
        }
        let tail = rest.get(i + HEAD.len()..)?;
        let ends = tail.starts_with(b"\x04ACIS\x0d\x04data") || tail.starts_with(b"\x03ASM\x0d\x04data");
        ends.then_some(start + i)
    })
}

/// What the decoder multiplies a binary stream's lengths by.
const BINARY_MM_PER_UNIT: f64 = 10.0;

/// Decoded text: the neutral B-rep (as JSON) and the data's millimetres per unit.
fn decode_text(data: &[u8]) -> Result<(Value, f64)> {
    let bytes = plain_text(data);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::desktop()).map_err(decode_error)?;
    let parsed = cadmpeg_asm::sat::parse(&ctx, &bytes).map_err(|e| err(format!("the solid's ACIS text can't be read ({e:?})")))?;
    let header = parsed.header.as_kernel_header();
    // The decoder turns the text's units into millimetres with the header's millimetres per
    // unit; a drawing takes the coordinates as they are written, so that factor comes back off.
    let scale = Some(parsed.header.scale().get()).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
    let brep = cadmpeg_asm::brep::decode_with_header(
        &ctx,
        &parsed.records,
        &bytes,
        Some(&header),
        "acis",
        format_id()?,
        cadmpeg_asm::brep::DecodePurpose::Model,
    )
    .map_err(decode_error)?;
    Ok((serde_json::to_value(&brep).map_err(decode_error)?, scale))
}

fn decode_binary(data: &[u8]) -> Result<(Value, f64)> {
    use cadmpeg_asm::{acis_header, asm_header};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::desktop()).map_err(decode_error)?;
    let (header, start, limit) = if let Some(h) = asm_header::parse(data) {
        let start = asm_header::record_stream_start_with_header(data, &h).ok_or_else(|| err("the solid's ACIS data has no records"))?;
        let limit = asm_header::solved_record_limit_with_header(data, &h).unwrap_or(data.len());
        (h, start, limit)
    } else if let Some(h) = acis_header::parse(data) {
        let start = acis_header::record_stream_start_with_header(data, &h).ok_or_else(|| err("the solid's ACIS data has no records"))?;
        let limit = acis_header::solved_record_limit(data).unwrap_or(data.len());
        (h, start, limit)
    } else {
        return Err(err("the solid's ACIS data has an unknown header"));
    };
    let limit = end_marker(data, start).map_or(limit, |end| end.min(limit));
    let records = cadmpeg_asm::sab::frame(&ctx, data, start, limit, header.width, None)
        .map_err(|e| err(format!("the solid's ACIS data can't be read ({e:?})")))?;
    // The decoder reads binary lengths as centimetres and returns millimetres; a drawing takes
    // the coordinates as they are written, so that factor comes back off.
    let scale = BINARY_MM_PER_UNIT;
    let brep = cadmpeg_asm::brep::decode_with_header(
        &ctx,
        &records,
        data,
        Some(&header.metadata),
        "acis",
        format_id()?,
        cadmpeg_asm::brep::DecodePurpose::Model,
    )
    .map_err(decode_error)?;
    Ok((serde_json::to_value(&brep).map_err(decode_error)?, scale))
}

/// The items of one list of the decoded B-rep, by id.
fn by_id<'a>(v: &'a Value, list: &str) -> HashMap<&'a str, &'a Value> {
    v.get(list).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| Some((x.get("id")?.as_str()?, x))).collect()).unwrap_or_default()
}

fn ids(v: Option<&Value>) -> Vec<&str> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|x| x.is_finite())
}

fn xyz(v: &Value, key: &str) -> Option<P3> {
    let p = v.get(key)?;
    Some([num(p, "x")?, num(p, "y")?, num(p, "z")?])
}

/// The decoded B-rep as STEP geometry, lengths in the data's own units.
struct Geometry {
    /// Millimetres per unit.
    mm: f64,
    /// The body's transform (ACIS's `transform` record): its linear part, its shift (in units)
    /// and its uniform scale.
    linear: [[f64; 3]; 3],
    shift: P3,
    scale: f64,
}

impl Geometry {
    fn new(mm: f64) -> Geometry {
        Geometry { mm, linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], shift: [0.0; 3], scale: 1.0 }
    }

    /// The body's transform, from the decoded body's rows (row-major, applied to column points;
    /// the shift in the decoder's millimetres of the binary centimetre convention). A mirroring
    /// or flattening transform is refused.
    fn transformed(&self, rows: Option<&Value>) -> Result<Geometry> {
        let Some(rows) = rows.and_then(Value::as_array) else { return Ok(Geometry::new(self.mm)) };
        let mut m = [[0.0; 4]; 3];
        for (i, row) in rows.iter().take(3).enumerate() {
            for (j, v) in row.as_array().into_iter().flatten().take(4).enumerate() {
                let v = v.as_f64().filter(|v| v.is_finite()).ok_or_else(|| err("a body transform that is not a number"))?;
                if let Some(slot) = m.get_mut(i).and_then(|r| r.get_mut(j)) {
                    *slot = v;
                }
            }
        }
        let linear = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
        let det = linear[0][0] * (linear[1][1] * linear[2][2] - linear[1][2] * linear[2][1])
            - linear[0][1] * (linear[1][0] * linear[2][2] - linear[1][2] * linear[2][0])
            + linear[0][2] * (linear[1][0] * linear[2][1] - linear[1][1] * linear[2][0]);
        if !(det.is_finite() && det > 1e-300) {
            return Err(err("a mirrored or flattened body (not read yet)"));
        }
        let shift = [m[0][3] / BINARY_MM_PER_UNIT, m[1][3] / BINARY_MM_PER_UNIT, m[2][3] / BINARY_MM_PER_UNIT];
        Ok(Geometry { mm: self.mm, linear, shift, scale: det.cbrt() })
    }

    fn len(&self, v: f64) -> f64 {
        v / self.mm * self.scale
    }
    fn apply(&self, p: P3) -> P3 {
        let l = &self.linear;
        [
            l[0][0] * p[0] + l[0][1] * p[1] + l[0][2] * p[2],
            l[1][0] * p[0] + l[1][1] * p[1] + l[1][2] * p[2],
            l[2][0] * p[0] + l[2][1] * p[1] + l[2][2] * p[2],
        ]
    }
    fn point(&self, v: &Value, key: &str) -> Option<P3> {
        let p = xyz(v, key)?;
        let q = self.apply([p[0] / self.mm, p[1] / self.mm, p[2] / self.mm]);
        Some(add(q, self.shift, 1.0))
    }
    fn direction(&self, v: &Value, key: &str) -> Option<P3> {
        unit(self.apply(xyz(v, key)?))
    }
    fn placement(&self, g: &Value, origin: &str, axis: &str, reference: &str) -> Option<Placement> {
        Some(Placement { origin: self.point(g, origin)?, axis: self.direction(g, axis)?, ref_direction: self.direction(g, reference)? })
    }

    fn surface(&self, s: &Value) -> Result<Surface> {
        let g = solved(s.get("geometry"));
        let g = g.ok_or_else(|| err("a face without a surface"))?;
        let kind = g.get("kind").and_then(Value::as_str).unwrap_or("");
        let bad = || err(format!("a {kind} surface the reader can't take"));
        match kind {
            "plane" => Ok(Surface::Plane { position: self.placement(g, "origin", "normal", "u_axis").ok_or_else(bad)? }),
            "cylinder" => Ok(Surface::Cylinder {
                position: self.placement(g, "origin", "axis", "ref_direction").ok_or_else(bad)?,
                radius: self.len(num(g, "radius").ok_or_else(bad)?),
            }),
            "cone" => {
                let position = self.placement(g, "origin", "axis", "ref_direction").ok_or_else(bad)?;
                let radius = self.len(num(g, "radius").ok_or_else(bad)?);
                let ratio = num(g, "ratio").unwrap_or(1.0);
                let half = num(g, "half_angle").ok_or_else(bad)?;
                if (ratio - 1.0).abs() > 1e-9 {
                    return Err(err("an elliptical cone (not read yet)"));
                }
                if half.abs() < 1e-12 { Ok(Surface::Cylinder { position, radius }) } else { Ok(Surface::Cone { position, radius, semi_angle: half }) }
            }
            "sphere" => Ok(Surface::Sphere {
                position: self.placement(g, "center", "axis", "ref_direction").ok_or_else(bad)?,
                radius: self.len(num(g, "radius").ok_or_else(bad)?),
            }),
            "torus" => Ok(Surface::Torus {
                position: self.placement(g, "center", "axis", "ref_direction").ok_or_else(bad)?,
                major_radius: self.len(num(g, "major_radius").ok_or_else(bad)?),
                minor_radius: self.len(num(g, "minor_radius").ok_or_else(bad)?),
            }),
            "nurbs" => self.nurbs_surface(g).ok_or_else(bad),
            _ => Err(bad()),
        }
    }

    fn curve(&self, c: &Value) -> Result<Option<Curve>> {
        let g = solved(c.get("geometry")).ok_or_else(|| err("an edge without a curve"))?;
        let kind = g.get("kind").and_then(Value::as_str).unwrap_or("");
        let bad = || err(format!("a {kind} curve the reader can't take"));
        Ok(Some(match kind {
            "line" => Curve::Line {
                origin: self.point(g, "origin").ok_or_else(bad)?,
                direction: self.direction(g, "direction").ok_or_else(bad)?,
                magnitude: 1.0,
            },
            "circle" => Curve::Circle {
                position: self.placement(g, "center", "axis", "ref_direction").ok_or_else(bad)?,
                radius: self.len(num(g, "radius").ok_or_else(bad)?),
            },
            "ellipse" => Curve::Ellipse {
                position: self.placement(g, "center", "axis", "ref_direction").ok_or_else(bad)?,
                semi_axis_1: self.len(num(g, "major_radius").ok_or_else(bad)?),
                semi_axis_2: self.len(num(g, "minor_radius").ok_or_else(bad)?),
            },
            "degenerate" => return Ok(None),
            "nurbs" => self.nurbs_curve(g).ok_or_else(bad)?,
            _ => return Err(bad()),
        }))
    }

    /// A pole: a point (`polynomial`) or a point and its weight (`rational`).
    fn pole(&self, v: &Value) -> Option<(P3, Option<f64>)> {
        match v.get("point") {
            Some(_) => Some((self.point(v, "point")?, Some(num(v, "weight").filter(|w| *w > 0.0)?))),
            None => Some((self.point(&serde_json::json!({ "p": v }), "p")?, None)),
        }
    }

    /// Poles in a row, with their weights when every pole has one.
    fn poles(&self, row: &[Value]) -> Option<(Vec<P3>, Option<Vec<f64>>)> {
        let mut points = Vec::with_capacity(row.len());
        let mut weights = Vec::with_capacity(row.len());
        for v in row {
            let (p, w) = self.pole(v)?;
            points.push(p);
            weights.extend(w);
        }
        let weights = (weights.len() == points.len() && !points.is_empty()).then_some(weights);
        Some((points, weights))
    }

    fn nurbs_curve(&self, g: &Value) -> Option<Curve> {
        let degree = usize::try_from(g.get("degree")?.as_u64()?).ok().filter(|d| (1..=MAX_DEGREE).contains(d))?;
        let row = g.get("poles")?.get("points")?.as_array()?;
        if row.len() > MAX_POLES {
            return None;
        }
        let (points, weights) = self.poles(row)?;
        let full: Vec<f64> = g.get("knots")?.as_array()?.iter().map(Value::as_f64).collect::<Option<_>>()?;
        if full.len() != points.len() + degree + 1 {
            return None;
        }
        let (knots, multiplicities) = distinct(&full);
        Some(Curve::BSpline(Box::new(BSplineCurve {
            degree,
            points,
            weights,
            knots,
            multiplicities,
            form: CurveForm::Unspecified,
            closed: Logical::Unknown,
            self_intersect: Logical::Unknown,
            knot_spec: KnotSpec::Unspecified,
        })))
    }

    fn nurbs_surface(&self, g: &Value) -> Option<Surface> {
        let ud = usize::try_from(g.get("u_degree")?.as_u64()?).ok().filter(|d| (1..=MAX_DEGREE).contains(d))?;
        let vd = usize::try_from(g.get("v_degree")?.as_u64()?).ok().filter(|d| (1..=MAX_DEGREE).contains(d))?;
        let rows = g.get("poles")?.get("rows")?.as_array()?;
        let (mut points, mut weights, mut rational) = (Vec::new(), Vec::new(), true);
        let mut total = 0usize;
        for row in rows {
            let row = row.as_array()?;
            total = total.checked_add(row.len())?;
            if total > MAX_POLES {
                return None;
            }
            let (p, w) = self.poles(row)?;
            if points.first().is_some_and(|f: &Vec<P3>| f.len() != p.len()) {
                return None;
            }
            match w {
                Some(w) => weights.push(w),
                None => rational = false,
            }
            points.push(p);
        }
        let uf: Vec<f64> = g.get("u_knots")?.as_array()?.iter().map(Value::as_f64).collect::<Option<_>>()?;
        let vf: Vec<f64> = g.get("v_knots")?.as_array()?.iter().map(Value::as_f64).collect::<Option<_>>()?;
        let columns = points.first().map_or(0, Vec::len);
        if uf.len() != points.len() + ud + 1 || vf.len() != columns + vd + 1 {
            return None;
        }
        let (u_knots, u_multiplicities) = distinct(&uf);
        let (v_knots, v_multiplicities) = distinct(&vf);
        Some(Surface::BSpline(Box::new(BSplineSurface {
            u_degree: ud,
            v_degree: vd,
            points,
            weights: rational.then_some(weights),
            u_knots,
            u_multiplicities,
            v_knots,
            v_multiplicities,
            form: SurfaceForm::Unspecified,
            u_closed: Logical::Unknown,
            v_closed: Logical::Unknown,
            self_intersect: Logical::Unknown,
            knot_spec: KnotSpec::Unspecified,
        })))
    }
}

/// Highest B-spline degree read, and most poles of one curve or surface.
const MAX_DEGREE: usize = 25;
const MAX_POLES: usize = 1 << 20;

/// A carrier's solved geometry: its own, or the cache of a procedural one.
fn solved(g: Option<&Value>) -> Option<&Value> {
    let g = g?;
    if g.get("construction").is_some() { g.get("cache") } else { Some(g) }
}

/// Full knots as distinct knots and multiplicities.
fn distinct(full: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let (mut knots, mut mults) = (Vec::new(), Vec::<usize>::new());
    for &k in full {
        match (knots.last(), mults.last_mut()) {
            (Some(last), Some(m)) if *last == k => *m += 1,
            _ => {
                knots.push(k);
                mults.push(1);
            }
        }
    }
    (knots, mults)
}

fn s(v: &str) -> Param {
    Param::Str(v.into())
}
fn r(id: u64) -> Param {
    Param::Ref(id)
}
fn b(v: bool) -> Param {
    Param::Enum(if v { "T" } else { "F" }.into())
}
fn list(ids: &[u64]) -> Param {
    Param::List(ids.iter().map(|i| Param::Ref(*i)).collect())
}

fn add(a: P3, b: P3, k: f64) -> P3 {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: P3) -> Option<P3> {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    (l.is_finite() && l > 1e-300).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

/// The seams that bound a whole sphere or torus, as solidcore writes them, and the points that
/// box the surface: a sphere's meridian from pole to pole, used both ways; a torus's meridian and
/// its outer equator, each used both ways.
fn seam_loop(w: &mut Writer, surface: &Surface) -> Option<(u64, Vec<P3>)> {
    let (position, outer, minor) = match surface {
        Surface::Sphere { position, radius } => (position, *radius, None),
        Surface::Torus { position, major_radius, minor_radius } => (position, major_radius + minor_radius, Some((*major_radius, *minor_radius))),
        _ => return None,
    };
    let (c, z, x) = (position.origin, unit(position.axis)?, unit(position.ref_direction)?);
    // The meridian's plane holds the axis and the reference direction; its normal is x × z.
    let normal = cross(x, z);
    let extent: Vec<P3> = [x, z, cross(z, x)].iter().flat_map(|d| [add(c, *d, outer), add(c, *d, -outer)]).collect();
    let mut g = GeomWriter::new(w);
    let loop_id = match minor {
        None => {
            let south = g.point(add(c, z, -outer));
            let north = g.point(add(c, z, outer));
            let meridian = g.curve(&Curve::Circle { position: Placement { origin: c, axis: normal, ref_direction: x }, radius: outer });
            let south = w.add_distinct("VERTEX_POINT", &[s(""), r(south)]);
            let north = w.add_distinct("VERTEX_POINT", &[s(""), r(north)]);
            let e = w.add_distinct("EDGE_CURVE", &[s(""), r(south), r(north), r(meridian), b(true)]);
            let back = w.add_distinct("ORIENTED_EDGE", &[s(""), Param::Derived, Param::Derived, r(e), b(false)]);
            let on = w.add_distinct("ORIENTED_EDGE", &[s(""), Param::Derived, Param::Derived, r(e), b(true)]);
            w.add_distinct("EDGE_LOOP", &[s(""), list(&[back, on])])
        }
        Some((major, small)) => {
            let at = g.point(add(c, x, outer));
            let meridian =
                g.curve(&Curve::Circle { position: Placement { origin: add(c, x, major), axis: normal, ref_direction: x }, radius: small });
            let equator = g.curve(&Curve::Circle { position: Placement { origin: c, axis: z, ref_direction: x }, radius: outer });
            let v = w.add_distinct("VERTEX_POINT", &[s(""), r(at)]);
            let m = w.add_distinct("EDGE_CURVE", &[s(""), r(v), r(v), r(meridian), b(true)]);
            let e = w.add_distinct("EDGE_CURVE", &[s(""), r(v), r(v), r(equator), b(true)]);
            let uses: Vec<u64> = [(m, false), (e, true), (m, true), (e, false)]
                .iter()
                .map(|(id, forward)| w.add_distinct("ORIENTED_EDGE", &[s(""), Param::Derived, Param::Derived, r(*id), b(*forward)]))
                .collect();
            w.add_distinct("EDGE_LOOP", &[s(""), list(&uses)])
        }
    };
    Some((loop_id, extent))
}

/// The decoded B-rep's solids as STEP text.
fn step(v: &Value, mm: f64) -> Result<(String, usize)> {
    let unplaced = Geometry::new(mm);
    let faces = by_id(v, "faces");
    if faces.len() > MAX_FACES {
        return Err(err("the solid has too many faces"));
    }
    let (loops, coedges, edges, vertices, points, surfaces, curves, shells, regions) = (
        by_id(v, "loops"),
        by_id(v, "coedges"),
        by_id(v, "edges"),
        by_id(v, "vertices"),
        by_id(v, "points"),
        by_id(v, "surfaces"),
        by_id(v, "curves"),
        by_id(v, "shells"),
        by_id(v, "regions"),
    );
    let mut w = Writer::new();
    let mut vertex_ids: HashMap<String, u64> = HashMap::new();
    let mut edge_ids: HashMap<&str, Option<u64>> = HashMap::new();
    let mut solids = Vec::new();

    let bodies = v.get("bodies").and_then(Value::as_array).cloned().unwrap_or_default();
    for body in &bodies {
        if body.get("kind").and_then(Value::as_str) != Some("solid") {
            continue;
        }
        let geometry = unplaced.transformed(body.get("transform"))?;
        for region in ids(body.get("regions")) {
            let Some(region) = regions.get(region) else { continue };
            // Each closed shell: its STEP shell, and the size of its box (the outer one is the
            // largest).
            let mut made: Vec<(u64, f64)> = Vec::new();
            for shell in ids(region.get("shells")) {
                let Some(shell) = shells.get(shell) else { continue };
                let mut face_ids = Vec::new();
                let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for member in shell.get("members").and_then(Value::as_array).into_iter().flatten() {
                    if member.get("kind").and_then(Value::as_str) != Some("face") {
                        continue;
                    }
                    let Some(face) = member.get("id").and_then(Value::as_str).and_then(|f| faces.get(f)) else { continue };
                    let surface =
                        face.get("surface").and_then(Value::as_str).and_then(|s| surfaces.get(s)).ok_or_else(|| err("a face without its surface"))?;
                    let surface_geometry = geometry.surface(surface)?;
                    let surface = GeomWriter::new(&mut w).surface(&surface_geometry);
                    let mut bounds = Vec::new();
                    for lp in ids(face.get("loops").and_then(|l| l.get("loops"))) {
                        let Some(lp) = loops.get(lp) else { continue };
                        let mut oriented = Vec::new();
                        let mut apex = None;
                        for co in ids(lp.get("boundary").and_then(|b| b.get("coedges"))) {
                            let Some(co) = coedges.get(co) else { continue };
                            let Some(edge_key) = co.get("edge").and_then(Value::as_str) else { continue };
                            let edge = match edge_ids.get(edge_key) {
                                Some(e) => *e,
                                None => {
                                    let e = edges.get(edge_key).ok_or_else(|| err("a coedge without its edge"))?;
                                    let mut vertex = |key: Option<&Value>| -> Result<u64> {
                                        let key = key.and_then(Value::as_str).ok_or_else(|| err("an edge without its vertex"))?;
                                        if let Some(id) = vertex_ids.get(key) {
                                            return Ok(*id);
                                        }
                                        let p = vertices
                                            .get(key)
                                            .and_then(|x| x.get("point"))
                                            .and_then(Value::as_str)
                                            .and_then(|p| points.get(p))
                                            .and_then(|p| geometry.point(p, "position"))
                                            .ok_or_else(|| err("a vertex without its point"))?;
                                        let pt = GeomWriter::new(&mut w).point(p);
                                        let id = w.add_distinct("VERTEX_POINT", &[s(""), r(pt)]);
                                        vertex_ids.insert(key.to_string(), id);
                                        Ok(id)
                                    };
                                    let (a, z) = (vertex(e.get("start"))?, vertex(e.get("end"))?);
                                    let carrier = e.get("carrier").and_then(|c| c.get("curve")).and_then(Value::as_str);
                                    let c = match carrier.and_then(|c| curves.get(c)) {
                                        Some(c) => geometry.curve(c)?,
                                        None => None,
                                    };
                                    let made = match c {
                                        Some(c) => {
                                            let c = GeomWriter::new(&mut w).curve(&c);
                                            Some(w.add_distinct("EDGE_CURVE", &[s(""), r(a), r(z), r(c), b(true)]))
                                        }
                                        // A point edge (an apex): its vertex only.
                                        None => {
                                            apex = Some(a);
                                            None
                                        }
                                    };
                                    edge_ids.insert(edge_key, made);
                                    made
                                }
                            };
                            match edge {
                                Some(e) => {
                                    let forward = co.get("sense").and_then(Value::as_str) != Some("reversed");
                                    oriented.push(w.add_distinct("ORIENTED_EDGE", &[s(""), Param::Derived, Param::Derived, r(e), b(forward)]));
                                }
                                None if apex.is_none() => {
                                    if let Some(vx) =
                                        edges.get(edge_key).and_then(|e| e.get("start")).and_then(Value::as_str).and_then(|k| vertex_ids.get(k))
                                    {
                                        apex = Some(*vx);
                                    }
                                }
                                None => {}
                            }
                        }
                        let lp = match (oriented.is_empty(), apex) {
                            (false, _) => w.add_distinct("EDGE_LOOP", &[s(""), list(&oriented)]),
                            (true, Some(vx)) => w.add_distinct("VERTEX_LOOP", &[s(""), r(vx)]),
                            (true, None) => continue,
                        };
                        bounds.push(w.add_distinct("FACE_BOUND", &[s(""), r(lp), b(true)]));
                    }
                    // A sphere or a torus face with no loops is the whole surface: STEP bounds it
                    // with its seams.
                    if bounds.is_empty()
                        && let Some((lp, extent)) = seam_loop(&mut w, &surface_geometry)
                    {
                        bounds.push(w.add_distinct("FACE_BOUND", &[s(""), r(lp), b(true)]));
                        for p in extent {
                            for k in 0..3 {
                                lo[k] = lo[k].min(p[k]);
                                hi[k] = hi[k].max(p[k]);
                            }
                        }
                    }
                    let same_sense = face.get("sense").and_then(Value::as_str) != Some("reversed");
                    face_ids.push(w.add_distinct("ADVANCED_FACE", &[s(""), list(&bounds), r(surface), b(same_sense)]));
                    // The face's box, from its edges' vertices.
                    for lp in ids(face.get("loops").and_then(|l| l.get("loops"))) {
                        for co in ids(loops.get(lp).and_then(|l| l.get("boundary")).and_then(|b| b.get("coedges"))) {
                            let e = coedges.get(co).and_then(|c| c.get("edge")).and_then(Value::as_str).and_then(|e| edges.get(e));
                            for end in ["start", "end"] {
                                let p = e
                                    .and_then(|e| e.get(end))
                                    .and_then(Value::as_str)
                                    .and_then(|k| vertices.get(k))
                                    .and_then(|x| x.get("point"))
                                    .and_then(Value::as_str)
                                    .and_then(|p| points.get(p))
                                    .and_then(|p| geometry.point(p, "position"));
                                if let Some(p) = p {
                                    for k in 0..3 {
                                        lo[k] = lo[k].min(p[k]);
                                        hi[k] = hi[k].max(p[k]);
                                    }
                                }
                            }
                        }
                    }
                }
                if face_ids.is_empty() {
                    continue;
                }
                let size = (0..3).map(|k| (hi[k] - lo[k]).max(0.0)).product::<f64>();
                made.push((w.add_distinct("CLOSED_SHELL", &[s(""), list(&face_ids)]), if size.is_finite() { size } else { f64::INFINITY }));
            }
            made.sort_by(|a, b| b.1.total_cmp(&a.1));
            let Some((outer, _)) = made.first().copied() else { continue };
            let solid = if made.len() == 1 {
                w.add_distinct("MANIFOLD_SOLID_BREP", &[s(""), r(outer)])
            } else {
                let voids: Vec<u64> =
                    made.iter().skip(1).map(|(v, _)| w.add_distinct("ORIENTED_CLOSED_SHELL", &[s(""), Param::Derived, r(*v), b(true)])).collect();
                w.add_distinct("BREP_WITH_VOIDS", &[s(""), r(outer), list(&voids)])
            };
            solids.push(solid);
        }
    }
    Ok((w.file("acis", "AUTOMOTIVE_DESIGN"), solids.len()))
}

/// The kernel making the pcurves STEP and ACIS leave out: each use's curve projected onto its
/// face's surface.
struct Kernel;

impl Trims for Kernel {
    fn trims(&self, face: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String> {
        let surface = face.nurbs.unwrap_or(face.surface);
        let mut loops = Vec::new();
        for l in &face.loops {
            let mut uses = Vec::new();
            for u in l {
                let e = face.edges.get(usize::try_from(u.edge).map_err(|e| e.to_string())?).ok_or("an edge out of range")?;
                uses.push(EdgeUse {
                    edge: u.edge,
                    curve: e.nurbs.unwrap_or(e.curve),
                    interval: e.interval,
                    forward: u.forward,
                    tolerance: e.tolerance,
                    given: u.pcurve.as_ref(),
                });
            }
            loops.push(uses);
        }
        solidcore_ops::pcurve::pcurves_within(surface, &loops, face.tolerance, Some(face.domain)).map_err(|e| e.to_string())
    }
}

fn to_bodies(v: &Value, mm: f64) -> Result<Vec<Body>> {
    let (text, count) = step(v, mm)?;
    if count == 0 {
        return Err(err("the ACIS data holds no solid"));
    }
    let ex = solidcore_step::p21::parse(&text).map_err(|e| err(format!("the solid can't be read ({e})")))?;
    let read = solidcore_step::topology::read_solids_with(&ex, &Kernel);
    if let Some(f) = read.failures.first() {
        return Err(err(format!("the solid can't be read ({})", f.message)));
    }
    let mut out = Vec::new();
    for solid in &read.solids {
        let (b, _) = ops::Body::from_record(&solid.brep, false)?;
        out.push(Body(b));
    }
    if out.is_empty() {
        return Err(err("the ACIS data holds no solid"));
    }
    Ok(out)
}
