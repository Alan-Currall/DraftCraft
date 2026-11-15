//! A body as ACIS records (written as SAT 400 or SAB 700 by `model`).
//!
//! The body goes through STEP first: solidcore writes its record as ISO 10303-42 topology on
//! exact surfaces and curves (`solidcore_step::write`), every face's side and loops already
//! settled. STEP and ACIS describe a B-rep the same way, entity for entity, so the STEP entities
//! become ACIS records one to one:
//!
//! | STEP | ACIS |
//! |---|---|
//! | MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS | lump (all of a body's in one `body`) |
//! | CLOSED_SHELL | shell |
//! | ADVANCED_FACE (same_sense) | face (sense) |
//! | FACE_BOUND, FACE_OUTER_BOUND of an EDGE_LOOP | loop |
//! | ORIENTED_EDGE | coedge |
//! | EDGE_CURVE | edge, its parameters those of its vertices on its curve |
//! | VERTEX_POINT | vertex and point |
//! | LINE, CIRCLE, ELLIPSE | straight-curve, ellipse-curve |
//! | PLANE, CYLINDRICAL_ and CONICAL_SURFACE, SPHERICAL_, TOROIDAL_SURFACE | plane-, cone-, sphere-, torus-surface |
//!
//! In both, a face's loops run counter-clockwise about its outward normal. ACIS needs no seam on
//! a face that goes all the way round (a cylinder's side is bounded by its two circles alone), so
//! the seam edges STEP gives such faces are left out, and a sphere or torus with nothing else
//! bounding it is a face without loops (a sphere's poles are no loops either). A cone's apex is
//! a loop of its own, as AutoCAD writes it: one coedge on an edge with no curve that starts and
//! ends at the apex vertex. Without it ShapeManager takes the cone for an invalid solid.
//! Coedges carry no pcurve: the modeller works them out.
//!
//! What has no exact ACIS form here yet (B-spline curves and surfaces: lofts, sweeps, fillets
//! between curved faces) is refused with the reason, and the caller keeps another form.

use std::collections::HashMap;
use std::f64::consts::TAU;

use solidcore_step::geom::{Curve, P3, Placement, Surface};
use solidcore_step::p21::{Entity, Exchange, Param};
use solidcore_step::read::{Reader, Units};

use super::model::{Field, Record};
use crate::body::{Body, Result, SolidError};

fn err(m: impl Into<String>) -> SolidError {
    SolidError(m.into())
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn scale(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn unit(a: P3) -> Option<P3> {
    let n = dot(a, a).sqrt();
    (n.is_finite() && n > 0.0).then(|| scale(a, 1.0 / n))
}

/// An ACIS curve: its fields, and how a point maps to its parameter.
#[derive(Clone, Debug)]
enum ACurve {
    Straight {
        root: P3,
        dir: P3,
    },
    /// `center + cos t·major + sin t·ratio·(normal × major)`.
    Ellipse {
        center: P3,
        normal: P3,
        major: P3,
        ratio: f64,
    },
}

impl ACurve {
    fn param(&self, p: P3) -> f64 {
        match self {
            ACurve::Straight { root, dir } => dot(sub(p, *root), *dir),
            ACurve::Ellipse { center, normal, major, ratio } => {
                let a = dot(*major, *major).sqrt().max(f64::MIN_POSITIVE);
                let e1 = scale(*major, 1.0 / a);
                let e2 = cross(*normal, e1);
                let d = sub(p, *center);
                (dot(d, e2) / (a * ratio.max(f64::MIN_POSITIVE))).atan2(dot(d, e1) / a)
            }
        }
    }

    fn periodic(&self) -> bool {
        matches!(self, ACurve::Ellipse { .. })
    }

    fn record(&self) -> Record {
        match self {
            ACurve::Straight { root, dir } => {
                Record { name: "straight-curve", fields: vec![Field::Pos(*root), Field::Vector(*dir), Field::Infinite, Field::Infinite] }
            }
            ACurve::Ellipse { center, normal, major, ratio } => Record {
                name: "ellipse-curve",
                fields: vec![
                    Field::Pos(*center),
                    Field::Vector(*normal),
                    Field::Vector(*major),
                    Field::Real(*ratio),
                    Field::Infinite,
                    Field::Infinite,
                ],
            },
        }
    }
}

/// The ACIS form of a STEP curve, the edge running along it as its parameter grows.
fn curve(c: &Curve) -> Result<ACurve> {
    match c {
        Curve::Line { origin, direction, .. } => {
            Ok(ACurve::Straight { root: *origin, dir: unit(*direction).ok_or_else(|| err("a line without a direction"))? })
        }
        Curve::Circle { position, radius } => {
            Ok(ACurve::Ellipse { center: position.origin, normal: position.axis, major: scale(position.ref_direction, *radius), ratio: 1.0 })
        }
        Curve::Ellipse { position, semi_axis_1, semi_axis_2 } => {
            let (a, b) = (*semi_axis_1, *semi_axis_2);
            if !(a > 0.0 && b > 0.0) {
                return Err(err("an ellipse without size"));
            }
            if a >= b {
                Ok(ACurve::Ellipse { center: position.origin, normal: position.axis, major: scale(position.ref_direction, a), ratio: b / a })
            } else {
                // The major axis along Y: the same normal keeps the direction (t' = t - π/2).
                Ok(ACurve::Ellipse { center: position.origin, normal: position.axis, major: scale(position.y(), b), ratio: a / b })
            }
        }
        Curve::SurfaceCurve { curve_3d, .. } => curve(curve_3d),
        Curve::Trimmed { basis, .. } => curve(basis),
        Curve::BSpline(_) => Err(err("a B-spline edge (ACIS spline curves are not written yet)")),
        _ => Err(err("an edge curve ACIS has no exact form for here")),
    }
}

/// The ACIS record of a STEP surface whose normal it keeps.
fn surface(s: &Surface) -> Result<Record> {
    let four = [Field::Infinite, Field::Infinite, Field::Infinite, Field::Infinite];
    let with = |name: &'static str, mut f: Vec<Field>| {
        f.extend(four.iter().cloned());
        Record { name, fields: f }
    };
    let cone = |p: &Placement, radius: f64, sin: f64, cos: f64| {
        with(
            "cone-surface",
            vec![
                Field::Pos(p.origin),
                Field::Vector(p.axis),
                Field::Vector(scale(p.ref_direction, radius)),
                Field::Real(1.0),
                Field::Infinite,
                Field::Infinite,
                Field::Real(sin),
                Field::Real(cos),
                Field::Real(radius),
                Field::Sense(false),
            ],
        )
    };
    match s {
        Surface::Plane { position } => Ok(with(
            "plane-surface",
            vec![Field::Pos(position.origin), Field::Vector(position.axis), Field::Vector(position.ref_direction), Field::ForwardV],
        )),
        Surface::Cylinder { position, radius } if *radius > 0.0 => Ok(cone(position, *radius, 0.0, 1.0)),
        Surface::Cone { position, radius, semi_angle } if semi_angle.is_finite() && semi_angle.abs() > 0.0 => {
            let (sin, cos) = semi_angle.sin_cos();
            if *radius > 0.0 {
                Ok(cone(position, *radius, sin, cos))
            } else {
                // The base ellipse can't be a point: move it one unit along the axis, to the side
                // the cone is on.
                let v = if *semi_angle > 0.0 { 1.0 } else { -1.0 };
                let p = Placement { origin: add(position.origin, scale(position.axis, v)), ..*position };
                Ok(cone(&p, (v * semi_angle.tan()).abs(), sin, cos))
            }
        }
        Surface::Sphere { position, radius } if *radius != 0.0 => Ok(with(
            "sphere-surface",
            vec![
                Field::Pos(position.origin),
                Field::Real(*radius),
                Field::Vector(position.ref_direction),
                Field::Vector(position.axis),
                Field::ForwardV,
            ],
        )),
        Surface::Torus { position, major_radius, minor_radius } if *major_radius > *minor_radius && *minor_radius > 0.0 => Ok(with(
            "torus-surface",
            vec![
                Field::Pos(position.origin),
                Field::Vector(position.axis),
                Field::Real(*major_radius),
                Field::Real(*minor_radius),
                Field::Vector(position.ref_direction),
                Field::ForwardV,
            ],
        )),
        Surface::BSpline(_) => Err(err("a B-spline face (ACIS spline surfaces are not written yet)")),
        _ => Err(err("a face surface ACIS has no exact form for here")),
    }
}

/// One use of an edge in a loop: the edge (into the edge table) and whether the loop runs
/// against it.
#[derive(Clone, Copy, Debug)]
struct Use {
    edge: usize,
    reversed: bool,
}

struct AEdge {
    start: usize,
    end: usize,
    /// `None` for a point edge: a cone's apex.
    curve: Option<ACurve>,
}

struct AFace {
    surface: Record,
    reversed: bool,
    loops: Vec<Vec<Use>>,
}

/// The body's B-rep read from the STEP entities, in ACIS terms.
#[derive(Default)]
struct Model {
    /// Lumps, each its shells, each its faces.
    lumps: Vec<Vec<Vec<AFace>>>,
    edges: Vec<AEdge>,
    edge_of: HashMap<u64, usize>,
    points: Vec<P3>,
    vertex_of: HashMap<u64, usize>,
}

fn refs(p: Option<&Param>) -> Vec<u64> {
    p.and_then(Param::as_list).map(|l| l.iter().filter_map(Param::as_ref_id).collect()).unwrap_or_default()
}

fn entity(ex: &Exchange, id: u64) -> Result<&Entity> {
    ex.get(id).ok_or_else(|| err(format!("STEP entity #{id} is missing")))
}

impl Model {
    fn vertex(&mut self, ex: &Exchange, r: &Reader<'_>, id: u64) -> Result<usize> {
        if let Some(&v) = self.vertex_of.get(&id) {
            return Ok(v);
        }
        let point = entity(ex, id)?.params().get(1).and_then(Param::as_ref_id).ok_or_else(|| err("a vertex without a point"))?;
        let p = r.point(point).map_err(err)?;
        self.points.push(p);
        let v = self.points.len() - 1;
        self.vertex_of.insert(id, v);
        Ok(v)
    }

    /// The edge of an EDGE_CURVE, and whether it runs against the STEP edge.
    fn edge(&mut self, ex: &Exchange, r: &mut Reader<'_>, id: u64) -> Result<(usize, bool)> {
        let e = entity(ex, id)?;
        let p = e.params();
        let same_sense = p.get(4).and_then(Param::as_bool).unwrap_or(true);
        if let Some(&i) = self.edge_of.get(&id) {
            return Ok((i, !same_sense));
        }
        let (a, b, geometry) = match (p.get(1).and_then(Param::as_ref_id), p.get(2).and_then(Param::as_ref_id), p.get(3).and_then(Param::as_ref_id)) {
            (Some(a), Some(b), Some(g)) => (a, b, g),
            _ => return Err(err("an edge without its vertices or curve")),
        };
        let (start, end) = (self.vertex(ex, r, a)?, self.vertex(ex, r, b)?);
        let c = r.curve(geometry).map_err(err)?;
        let c = curve(&c)?;
        // The ACIS edge runs as its curve does: from the STEP edge's start when the senses agree.
        let (start, end) = if same_sense { (start, end) } else { (end, start) };
        self.edges.push(AEdge { start, end, curve: Some(c) });
        let i = self.edges.len() - 1;
        self.edge_of.insert(id, i);
        Ok((i, !same_sense))
    }

    /// The loop at a cone's apex: one use of a point edge (no curve, starting and ending at the
    /// apex vertex), the way AutoCAD writes it.
    fn apex_loop(&mut self, v: usize) -> Vec<Use> {
        self.edges.push(AEdge { start: v, end: v, curve: None });
        vec![Use { edge: self.edges.len() - 1, reversed: true }]
    }

    /// The vertices of `all`'s edges that no edge of `kept` reaches: where only seams went (a
    /// cone's apex, which STEP reaches by the seam line).
    fn left_by_seams(&self, all: &[Vec<Use>], kept: &[Vec<Use>]) -> Vec<usize> {
        let ends = |loops: &[Vec<Use>]| -> Vec<usize> {
            loops.iter().flatten().filter_map(|u| self.edges.get(u.edge)).flat_map(|e| [e.start, e.end]).collect()
        };
        let reached = ends(kept);
        let mut out: Vec<usize> = Vec::new();
        for v in ends(all) {
            if !reached.contains(&v) && !out.contains(&v) {
                out.push(v);
            }
        }
        out
    }

    fn face(&mut self, ex: &Exchange, r: &mut Reader<'_>, id: u64, flip: bool) -> Result<AFace> {
        let e = entity(ex, id)?;
        if e.name() == "ORIENTED_FACE" {
            let inner = e.params().get(2).and_then(Param::as_ref_id).ok_or_else(|| err("an oriented face without its face"))?;
            let orientation = e.params().get(3).and_then(Param::as_bool).unwrap_or(true);
            return self.face(ex, r, inner, flip ^ !orientation);
        }
        if e.name() != "ADVANCED_FACE" {
            return Err(err(format!("a {} face", e.name())));
        }
        let p = e.params();
        let surface_id = p.get(2).and_then(Param::as_ref_id).ok_or_else(|| err("a face without a surface"))?;
        let same_sense = p.get(3).and_then(Param::as_bool).unwrap_or(true);
        let s = r.surface(surface_id).map_err(err)?;
        let is_cone = matches!(*s, Surface::Cone { .. });
        let s = surface(&s)?;
        let mut loops = Vec::new();
        for bound in refs(p.get(1)) {
            let b = entity(ex, bound)?;
            let lp = b.params().get(1).and_then(Param::as_ref_id).ok_or_else(|| err("a face bound without its loop"))?;
            let orientation = b.params().get(2).and_then(Param::as_bool).unwrap_or(true);
            let l = entity(ex, lp)?;
            if l.name() == "VERTEX_LOOP" {
                // A cone's apex is a loop of one point edge in ACIS; a sphere's pole is nothing.
                if is_cone {
                    let vertex = l.params().get(1).and_then(Param::as_ref_id).ok_or_else(|| err("an apex loop without its vertex"))?;
                    let v = self.vertex(ex, r, vertex)?;
                    loops.push(self.apex_loop(v));
                }
                continue;
            }
            if l.name() != "EDGE_LOOP" {
                return Err(err(format!("a {} loop", l.name())));
            }
            let mut uses = Vec::new();
            for oe in refs(l.params().get(1)) {
                let o = entity(ex, oe)?;
                let edge = o.params().get(3).and_then(Param::as_ref_id).ok_or_else(|| err("an oriented edge without its edge"))?;
                let forward = o.params().get(4).and_then(Param::as_bool).unwrap_or(true);
                let (edge, against) = self.edge(ex, r, edge)?;
                uses.push(Use { edge, reversed: !forward ^ against });
            }
            if !(orientation ^ flip) {
                uses.reverse();
                for u in &mut uses {
                    u.reversed = !u.reversed;
                }
            }
            loops.push(uses);
        }
        let mut kept = self.without_seams(loops.clone());
        if is_cone {
            for v in self.left_by_seams(&loops, &kept) {
                let apex = self.apex_loop(v);
                kept.push(apex);
            }
        }
        Ok(AFace { surface: s, reversed: !(same_sense ^ flip), loops: kept })
    }

    /// Where a use starts and ends.
    fn ends(&self, u: Use) -> Option<(usize, usize)> {
        let e = self.edges.get(u.edge)?;
        Some(if u.reversed { (e.end, e.start) } else { (e.start, e.end) })
    }

    /// A face's loops without the seams STEP needs and ACIS doesn't: the edges one face uses
    /// twice, the rest of each loop split where it closes. A loop that doesn't close without its
    /// seams keeps them.
    fn without_seams(&self, loops: Vec<Vec<Use>>) -> Vec<Vec<Use>> {
        let mut count: HashMap<usize, usize> = HashMap::new();
        for u in loops.iter().flatten() {
            *count.entry(u.edge).or_insert(0) += 1;
        }
        let mut out = Vec::new();
        for l in loops {
            let kept: Vec<Use> = l.iter().copied().filter(|u| count.get(&u.edge).copied().unwrap_or(0) < 2).collect();
            if kept.len() == l.len() {
                out.push(l);
                continue;
            }
            let mut split: Vec<Vec<Use>> = Vec::new();
            let mut current: Vec<Use> = Vec::new();
            let mut closes = true;
            for (i, u) in kept.iter().enumerate() {
                if let (Some(prev), Some(here)) = (current.last().and_then(|p| self.ends(*p)), self.ends(*u))
                    && prev.1 != here.0
                {
                    closes = false;
                }
                current.push(*u);
                let first = current.first().and_then(|f| self.ends(*f)).map(|e| e.0);
                if first.is_some() && first == self.ends(*u).map(|e| e.1) {
                    split.push(std::mem::take(&mut current));
                } else if i + 1 == kept.len() {
                    closes = false;
                }
            }
            if closes && current.is_empty() {
                out.extend(split);
            } else {
                out.push(l);
            }
        }
        out
    }
}

/// The STEP text's solids as one ACIS body's records.
fn records(ex: &Exchange) -> Result<Vec<Record>> {
    let mut r = Reader::new(ex, Units::default());
    let mut m = Model::default();
    for (_, e) in ex.entities.iter().filter(|(_, e)| matches!(e.name(), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS")) {
        let p = e.params();
        let mut shells: Vec<(u64, bool)> = p.get(1).and_then(Param::as_ref_id).map(|s| vec![(s, false)]).unwrap_or_default();
        for void in refs(p.get(2)) {
            // ORIENTED_CLOSED_SHELL('', *, shell, orientation)
            let v = entity(ex, void)?;
            let inner = v.params().get(2).and_then(Param::as_ref_id).unwrap_or(void);
            let orientation = v.params().get(3).and_then(Param::as_bool).unwrap_or(true);
            shells.push((inner, !orientation));
        }
        let mut lump = Vec::new();
        for (shell, flip) in shells {
            let mut faces = Vec::new();
            for f in refs(entity(ex, shell)?.params().get(1)) {
                faces.push(m.face(ex, &mut r, f, flip)?);
            }
            lump.push(faces);
        }
        m.lumps.push(lump);
    }
    if m.lumps.is_empty() {
        return Err(err("the body has no solid to write"));
    }
    layout(&m)
}

/// The model's records in file order: the body, lumps, shells, faces, loops, coedges, edges,
/// vertices, points, surfaces and curves.
fn layout(m: &Model) -> Result<Vec<Record>> {
    let faces: Vec<&AFace> = m.lumps.iter().flatten().flatten().collect();
    let lumps = m.lumps.len();
    let shells: usize = m.lumps.iter().map(Vec::len).sum();
    let loops: usize = faces.iter().map(|f| f.loops.len()).sum();
    let coedges: usize = faces.iter().flat_map(|f| f.loops.iter()).map(Vec::len).sum();
    // Only edges some face uses, and their vertices.
    let mut edge_index: Vec<Option<usize>> = vec![None; m.edges.len()];
    let mut used_edges = Vec::new();
    for u in faces.iter().flat_map(|f| f.loops.iter()).flatten() {
        if let Some(slot) = edge_index.get_mut(u.edge)
            && slot.is_none()
        {
            *slot = Some(used_edges.len());
            used_edges.push(u.edge);
        }
    }
    let mut vertex_index: Vec<Option<usize>> = vec![None; m.points.len()];
    let mut used_vertices = Vec::new();
    for &e in &used_edges {
        let edge = m.edges.get(e).ok_or_else(|| err("an edge out of range"))?;
        for v in [edge.start, edge.end] {
            if let Some(slot) = vertex_index.get_mut(v)
                && slot.is_none()
            {
                *slot = Some(used_vertices.len());
                used_vertices.push(v);
            }
        }
    }
    let base_lump = 1;
    let base_shell = base_lump + lumps;
    let base_face = base_shell + shells;
    let base_loop = base_face + faces.len();
    let base_coedge = base_loop + loops;
    let base_edge = base_coedge + coedges;
    let base_vertex = base_edge + used_edges.len();
    let base_point = base_vertex + used_vertices.len();
    let base_surface = base_point + used_vertices.len();
    let base_curve = base_surface + faces.len();
    // Each used edge's curve record, point edges having none.
    let mut curve_index: Vec<Option<usize>> = Vec::with_capacity(used_edges.len());
    let mut curves = 0usize;
    for &e in &used_edges {
        let has = m.edges.get(e).is_some_and(|ed| ed.curve.is_some());
        curve_index.push(has.then(|| {
            curves += 1;
            base_curve + curves - 1
        }));
    }

    // Coedges: their loop, place and use, and each edge's uses in order.
    struct Co {
        loop_index: usize,
        next: usize,
        prev: usize,
        u: Use,
    }
    let mut cos: Vec<Co> = Vec::with_capacity(coedges);
    let mut loop_first: Vec<usize> = Vec::with_capacity(loops);
    let mut face_first_loop: Vec<Option<usize>> = Vec::with_capacity(faces.len());
    let mut loop_face: Vec<usize> = Vec::with_capacity(loops);
    for (fi, f) in faces.iter().enumerate() {
        face_first_loop.push((!f.loops.is_empty()).then_some(loop_first.len()));
        for l in &f.loops {
            let li = loop_first.len();
            let first = cos.len();
            loop_first.push(first);
            loop_face.push(fi);
            let n = l.len();
            for (k, u) in l.iter().enumerate() {
                cos.push(Co { loop_index: li, next: first + (k + 1) % n, prev: first + (k + n - 1) % n, u: *u });
            }
        }
    }
    let mut uses_of: Vec<Vec<usize>> = vec![Vec::new(); used_edges.len()];
    for (ci, c) in cos.iter().enumerate() {
        if let Some(e) = edge_index.get(c.u.edge).copied().flatten()
            && let Some(list) = uses_of.get_mut(e)
        {
            list.push(ci);
        }
    }

    let mut out: Vec<Record> = Vec::new();
    out.push(Record { name: "body", fields: vec![Field::Ref(Some(base_lump)), Field::Ref(None), Field::Ref(None)] });
    let mut shell_at = base_shell;
    let mut face_at = base_face;
    for (li, lump) in m.lumps.iter().enumerate() {
        out.push(Record {
            name: "lump",
            fields: vec![Field::Ref((li + 1 < lumps).then_some(base_lump + li + 1)), Field::Ref(Some(shell_at)), Field::Ref(Some(0))],
        });
        shell_at += lump.len();
    }
    let mut shell_index = base_shell;
    for (li, lump) in m.lumps.iter().enumerate() {
        for (si, shell) in lump.iter().enumerate() {
            out.push(Record {
                name: "shell",
                fields: vec![
                    Field::Ref((si + 1 < lump.len()).then_some(shell_index + 1)),
                    Field::Ref(None),
                    Field::Ref((!shell.is_empty()).then_some(face_at)),
                    Field::Ref(None),
                    Field::Ref(Some(base_lump + li)),
                ],
            });
            shell_index += 1;
            face_at += shell.len();
        }
    }
    let mut fi = 0usize;
    let mut shell_index = base_shell;
    for lump in &m.lumps {
        for shell in lump {
            for (k, f) in shell.iter().enumerate() {
                out.push(Record {
                    name: "face",
                    fields: vec![
                        Field::Ref((k + 1 < shell.len()).then_some(base_face + fi + 1)),
                        Field::Ref(face_first_loop.get(fi).copied().flatten().map(|l| base_loop + l)),
                        Field::Ref(Some(shell_index)),
                        Field::Ref(None),
                        Field::Ref(Some(base_surface + fi)),
                        Field::Sense(f.reversed),
                        Field::Single,
                    ],
                });
                fi += 1;
            }
            shell_index += 1;
        }
    }
    for (li, &first) in loop_first.iter().enumerate() {
        let face = loop_face.get(li).copied().unwrap_or(0);
        let next = loop_face.get(li + 1).filter(|f| **f == face).map(|_| base_loop + li + 1);
        out.push(Record { name: "loop", fields: vec![Field::Ref(next), Field::Ref(Some(base_coedge + first)), Field::Ref(Some(base_face + face))] });
    }
    for (ci, c) in cos.iter().enumerate() {
        let e = edge_index.get(c.u.edge).copied().flatten().ok_or_else(|| err("a coedge without its edge"))?;
        let uses = uses_of.get(e).ok_or_else(|| err("an edge without uses"))?;
        let at = uses.iter().position(|u| *u == ci).unwrap_or(0);
        let partner = (uses.len() > 1).then(|| uses.get((at + 1) % uses.len()).copied().unwrap_or(ci)).map(|p| base_coedge + p);
        out.push(Record {
            name: "coedge",
            fields: vec![
                Field::Ref(Some(base_coedge + c.next)),
                Field::Ref(Some(base_coedge + c.prev)),
                Field::Ref(partner),
                Field::Ref(Some(base_edge + e)),
                Field::Sense(c.u.reversed),
                Field::Ref(Some(base_loop + c.loop_index)),
                Field::Ref(None),
            ],
        });
    }
    for (k, &e) in used_edges.iter().enumerate() {
        let edge = m.edges.get(e).ok_or_else(|| err("an edge out of range"))?;
        let (ps, pe) = (m.points.get(edge.start), m.points.get(edge.end));
        let (Some(ps), Some(pe)) = (ps, pe) else { return Err(err("an edge without its points")) };
        // A point edge has the empty range AutoCAD gives it: from 1 to 0.
        let (t0, mut t1) = match &edge.curve {
            Some(c) => (c.param(*ps), c.param(*pe)),
            None => (1.0, 0.0),
        };
        if edge.curve.as_ref().is_some_and(ACurve::periodic) {
            while t1 <= t0 + 1e-12 {
                t1 += TAU;
            }
            while t1 > t0 + TAU + 1e-12 {
                t1 -= TAU;
            }
        }
        let v = |p: usize| vertex_index.get(p).copied().flatten().map(|i| base_vertex + i);
        let first_use = uses_of.get(k).and_then(|u| u.first()).map(|c| base_coedge + c);
        out.push(Record {
            name: "edge",
            fields: vec![
                Field::Ref(v(edge.start)),
                Field::Param(t0),
                Field::Ref(v(edge.end)),
                Field::Param(t1),
                Field::Ref(first_use),
                Field::Ref(curve_index.get(k).copied().flatten()),
                Field::Sense(false),
                Field::Convexity,
            ],
        });
    }
    for (k, &p) in used_vertices.iter().enumerate() {
        let edge = used_edges.iter().position(|&e| m.edges.get(e).is_some_and(|ed| ed.start == p || ed.end == p)).map(|i| base_edge + i);
        out.push(Record { name: "vertex", fields: vec![Field::Ref(edge), Field::Ref(Some(base_point + k))] });
    }
    for &p in &used_vertices {
        out.push(Record { name: "point", fields: vec![Field::Pos(m.points.get(p).copied().unwrap_or([0.0; 3]))] });
    }
    for f in &faces {
        out.push(f.surface.clone());
    }
    for &e in &used_edges {
        if let Some(c) = &m.edges.get(e).ok_or_else(|| err("an edge out of range"))?.curve {
            out.push(c.record());
        }
    }
    Ok(out)
}

/// The body's ACIS records: through STEP, as the module says.
pub(crate) fn body_records(body: &Body) -> Result<Vec<Record>> {
    // The record is read from a copy: reading it gives the body's faces their UV trims.
    let copy = body.0.copy()?;
    let header = solidcore_step::write::product::Header::default();
    let text = solidcore_step::write::product::solidcore_bodies(&[("solid", &copy)], &header)
        .map_err(|e| err(format!("the solid can't be written ({e})")))?;
    let ex = solidcore_step::p21::parse(&text).map_err(|e| err(format!("the solid can't be written ({e})")))?;
    records(&ex)
}
