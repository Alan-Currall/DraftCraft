//! The body type and the kernel operations DraftCraft uses.
//!
//! Every call goes through the kernel's own `guard` (a panic inside truck becomes an error) and,
//! on top of it, `catch` here, so a kernel failure is always a [`SolidError`] and never takes the
//! drawing down.

use draftcraft_geom::{Mat4, Vec2, Vec3};
use solidcraft_geom as sg;
use solidcraft_kernel as k;

/// A solid body. The kernel's type never leaves this crate.
#[derive(Clone)]
pub struct Body(pub(crate) k::Body);

/// A kernel operation that failed, or input it refused. The message is for the user.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct SolidError(pub String);

impl From<k::KernelError> for SolidError {
    fn from(e: k::KernelError) -> Self {
        SolidError(match e {
            k::KernelError::Invalid(m) => m,
            k::KernelError::Failed(m) | k::KernelError::TooLarge(m) => m,
            k::KernelError::Internal(m) => format!("the solid modeler failed ({m})"),
        })
    }
}

pub type Result<T> = std::result::Result<T, SolidError>;

fn err(m: impl Into<String>) -> SolidError {
    SolidError(m.into())
}

/// Run kernel work under the kernel's panic guard (a no-op on wasm, where a panic aborts).
fn guarded<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let mut out = None;
    let r = k::guard(what, || {
        out = Some(f());
        Ok(())
    });
    match (r, out) {
        (Ok(()), Some(v)) => v,
        (Err(e), _) => Err(e.into()),
        (Ok(()), None) => Err(err(format!("{what}: no result"))),
    }
}

/// Largest coordinate or size accepted from input (the kernel's own limit on a body's size is
/// 1e7): beyond it tolerances lose their meaning. Bodies far from the origin are kept near their
/// own origin by the caller, with a placement (docs/solids.md, "Units and scale").
pub const MAX_SIZE: f64 = 1e7;

fn finite(v: f64, what: &str) -> Result<f64> {
    if v.is_finite() && v.abs() < MAX_SIZE { Ok(v) } else { Err(err(format!("{what} must be a finite number below {MAX_SIZE}"))) }
}

fn point(p: Vec3, what: &str) -> Result<sg::Vec3> {
    Ok(sg::Vec3::new(finite(p.x, what)?, finite(p.y, what)?, finite(p.z, what)?))
}

pub(crate) fn to_sg(p: Vec3) -> sg::Vec3 {
    sg::Vec3::new(p.x, p.y, p.z)
}

pub(crate) fn from_sg(p: sg::Vec3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

/// A plane with an orthonormal frame: points in it are `origin + u x + v y`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: Vec3,
    pub x: Vec3,
    pub y: Vec3,
}

impl Frame {
    /// The world XY plane at height `z`.
    pub fn xy(z: f64) -> Frame {
        Frame { origin: Vec3::new(0.0, 0.0, z), x: Vec3::new(1.0, 0.0, 0.0), y: Vec3::new(0.0, 1.0, 0.0) }
    }
    /// A frame on the plane through `origin` with `normal`, its x axis as near world X as the
    /// plane allows (world Y for a plane facing along X).
    pub fn from_normal(origin: Vec3, normal: Vec3) -> Result<Frame> {
        let n = unit(normal).ok_or_else(|| err("the plane needs a normal direction"))?;
        let seed = if n.x.abs() > 0.9 { Vec3::new(0.0, 1.0, 0.0) } else { Vec3::new(1.0, 0.0, 0.0) };
        let x = unit(seed - n * seed.dot(n)).ok_or_else(|| err("the plane needs a normal direction"))?;
        Ok(Frame { origin, x, y: n.cross(x) })
    }
    pub fn normal(&self) -> Vec3 {
        self.x.cross(self.y)
    }
    pub fn to_world(&self, p: Vec2) -> Vec3 {
        self.origin + self.x * p.x + self.y * p.y
    }
    pub fn to_local(&self, p: Vec3) -> Vec2 {
        let d = p - self.origin;
        Vec2::new(d.dot(self.x), d.dot(self.y))
    }
    fn plane(&self) -> Result<sg::Plane> {
        let (x, y) = (unit(self.x), unit(self.y));
        match (x, y) {
            (Some(x), Some(y)) if x.dot(y).abs() < 1e-9 => {
                Ok(sg::Plane { origin: point(self.origin, "the plane's origin")?, x: to_sg(x), y: to_sg(y) })
            }
            _ => Err(err("the plane's axes must be perpendicular directions")),
        }
    }
}

fn unit(v: Vec3) -> Option<Vec3> {
    let l = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    (l.is_finite() && l > 1e-12).then(|| v * (1.0 / l))
}

/// One piece of a 2D profile boundary or of a 2D result, in a plane's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line {
        a: Vec2,
        b: Vec2,
    },
    /// Counter-clockwise for a positive `sweep` (radians), from the angle `start`.
    Arc {
        center: Vec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

impl Seg {
    pub fn start(&self) -> Vec2 {
        match *self {
            Seg::Line { a, .. } => a,
            Seg::Arc { center, radius, start, .. } => Vec2::new(center.x + radius * start.cos(), center.y + radius * start.sin()),
        }
    }
    pub fn end(&self) -> Vec2 {
        match *self {
            Seg::Line { b, .. } => b,
            Seg::Arc { center, radius, start, sweep } => {
                Vec2::new(center.x + radius * (start + sweep).cos(), center.y + radius * (start + sweep).sin())
            }
        }
    }
    pub fn reversed(&self) -> Seg {
        match *self {
            Seg::Line { a, b } => Seg::Line { a: b, b: a },
            Seg::Arc { center, radius, start, sweep } => Seg::Arc { center, radius, start: start + sweep, sweep: -sweep },
        }
    }
    fn to_sg(self) -> Result<sg::Seg2> {
        Ok(match self {
            Seg::Line { a, b } => sg::Seg2::Line { a: v2(a, "a profile point")?, b: v2(b, "a profile point")? },
            Seg::Arc { center, radius, start, sweep } => {
                if !(radius > 0.0 && start.is_finite() && sweep.is_finite() && sweep != 0.0) {
                    return Err(err("a profile arc needs a positive radius and a sweep"));
                }
                sg::Seg2::Arc { center: v2(center, "an arc centre")?, radius: finite(radius, "a radius")?, start, sweep }
            }
        })
    }
}

fn v2(p: Vec2, what: &str) -> Result<sg::Vec2> {
    Ok(sg::Vec2::new(finite(p.x, what)?, finite(p.y, what)?))
}

/// Most segments of one profile, and loops of one region.
pub const MAX_PROFILE_SEGMENTS: usize = 100_000;

/// A closed planar region: its outer boundary and its holes, each a closed chain of segments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub outer: Vec<Seg>,
    pub holes: Vec<Vec<Seg>>,
}

impl Profile {
    fn to_region(&self) -> Result<sg::Region2> {
        let total = self.outer.len().saturating_add(self.holes.iter().map(Vec::len).sum());
        if self.outer.is_empty() || total > MAX_PROFILE_SEGMENTS {
            return Err(err("the profile must be a closed boundary of 1 to 100000 segments"));
        }
        let lp = |segs: &[Seg]| -> Result<sg::Loop2> { Ok(sg::Loop2 { segs: segs.iter().map(|s| s.to_sg()).collect::<Result<_>>()? }) };
        Ok(sg::Region2 { outer: lp(&self.outer)?, holes: self.holes.iter().map(|h| lp(h)).collect::<Result<_>>()? })
    }
}

/// Mass properties of a body (MASSPROP), in drawing units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mass {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
    pub min: Vec3,
    pub max: Vec3,
}

/// What a body looks like: its edges as polylines and its faces as triangles, in the body's
/// coordinates. The document keeps it with the solid, so drawing never calls the kernel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Display {
    pub wires: Vec<Vec<Vec3>>,
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// The face each triangle belongs to.
    pub tri_face: Vec<u32>,
}

/// Which solids a boolean keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

// ---- construction ----

/// A box with opposite corners `a` and `b`, its sides along the axes.
pub fn box_solid(a: Vec3, b: Vec3) -> Result<Body> {
    let (a, b) = (point(a, "a corner")?, point(b, "a corner")?);
    guarded("box", || Ok(Body(k::box_solid(a, b)?)))
}

/// A cylinder standing on the circle of `radius` about `base`, `height` along `axis`.
pub fn cylinder(base: Vec3, axis: Vec3, radius: f64, height: f64) -> Result<Body> {
    let (base, axis) = (point(base, "the base centre")?, point(axis, "the axis")?);
    let (radius, height) = (finite(radius, "the radius")?, finite(height, "the height")?);
    if !(height > 1e-9) {
        return Err(err("the height must be positive"));
    }
    guarded("cylinder", || Ok(Body(k::cylinder(base, axis, radius, height)?)))
}

/// A cone (or a frustum when `top_radius` is above 0) on the circle of `radius` about `base`,
/// `height` along `axis`: the revolution of its side profile.
pub fn cone(base: Vec3, axis: Vec3, radius: f64, top_radius: f64, height: f64) -> Result<Body> {
    let (radius, top, height) = (finite(radius, "the radius")?, finite(top_radius, "the top radius")?, finite(height, "the height")?);
    if !(radius > 1e-9 && top >= 0.0 && height > 1e-9) {
        return Err(err("a cone needs a positive base radius and height, and a top radius of 0 or more"));
    }
    let frame = Frame::from_normal(base, axis)?;
    // The profile in the plane through the axis: x along the radius, y along the axis.
    let plane = Frame { origin: base, x: frame.x, y: unit(axis).ok_or_else(|| err("the axis needs a direction"))? };
    let (o, r0, r1, h) = (Vec2::new(0.0, 0.0), Vec2::new(radius, 0.0), Vec2::new(top, height), Vec2::new(0.0, height));
    let outer = if top > 1e-9 {
        vec![Seg::Line { a: o, b: r0 }, Seg::Line { a: r0, b: r1 }, Seg::Line { a: r1, b: h }, Seg::Line { a: h, b: o }]
    } else {
        vec![Seg::Line { a: o, b: r0 }, Seg::Line { a: r0, b: h }, Seg::Line { a: h, b: o }]
    };
    let mut bodies = revolve(&plane, &Profile { outer, holes: Vec::new() }, Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), std::f64::consts::TAU)?;
    bodies.pop().ok_or_else(|| err("the cone made no solid"))
}

/// A sphere.
pub fn sphere(center: Vec3, radius: f64) -> Result<Body> {
    let (center, radius) = (point(center, "the centre")?, finite(radius, "the radius")?);
    if !(radius > 1e-9) {
        return Err(err("the radius must be positive"));
    }
    guarded("sphere", || Ok(Body(k::sphere(center, radius)?)))
}

/// A torus about the world Z axis through `center`.
pub fn torus(center: Vec3, major: f64, minor: f64) -> Result<Body> {
    let (center, major, minor) = (point(center, "the centre")?, finite(major, "the torus radius")?, finite(minor, "the tube radius")?);
    guarded("torus", || Ok(Body(k::torus(center, major, minor)?)))
}

/// Extrude a region from 0 to `height` along the plane's normal (negative goes the other way),
/// its sides leaning in by `taper` radians (0: straight).
pub fn extrude(frame: &Frame, profile: &Profile, height: f64, taper: f64) -> Result<Vec<Body>> {
    let (plane, region) = (frame.plane()?, profile.to_region()?);
    let height = finite(height, "the height")?;
    if height.abs() <= 1e-9 {
        return Err(err("the height must not be 0"));
    }
    if !(taper.is_finite() && taper.abs() < std::f64::consts::FRAC_PI_2) {
        return Err(err("the taper angle must be between -90 and 90 degrees"));
    }
    guarded("extrude", || {
        if taper == 0.0 {
            let (a, b) = if height > 0.0 { (0.0, height) } else { (height, 0.0) };
            Ok(k::extrude(&plane, &[region], a, b)?.into_iter().map(Body).collect())
        } else {
            // The kernel's taper grows the profile for a positive angle; AutoCAD's leans it in.
            Ok(vec![Body(k::extrude_tapered(&plane, &region, height.abs(), height.signum(), -taper)?)])
        }
    })
}

/// Revolve a region about the axis through `axis_origin` along `axis_dir` (both in the plane's
/// coordinates) by `angle` radians.
pub fn revolve(frame: &Frame, profile: &Profile, axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    let (plane, region) = (frame.plane()?, profile.to_region()?);
    let (o, d) = (v2(axis_origin, "the axis point")?, v2(axis_dir, "the axis direction")?);
    if !(angle.is_finite() && angle.abs() > 1e-9) {
        return Err(err("the angle must not be 0"));
    }
    let angle = angle.clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
    guarded("revolve", || Ok(k::revolve(&plane, &[region], o, d, angle)?.into_iter().map(Body).collect()))
}

/// `a` combined with `b`; `None` when nothing is left (an empty intersection or subtraction).
pub fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
    let op = match op {
        BoolOp::Union => k::BoolOp::Union,
        BoolOp::Subtract => k::BoolOp::Cut,
        BoolOp::Intersect => k::BoolOp::Intersect,
    };
    guarded("boolean", || Ok(k::boolean(&a.0, &b.0, op)?.map(Body)))
}

/// The parts of `body` on each side of a plane: (the side the normal points to, the other side).
pub fn slice(body: &Body, origin: Vec3, normal: Vec3) -> Result<(Option<Body>, Option<Body>)> {
    let plane = Frame::from_normal(origin, normal)?.plane()?;
    guarded("slice", || {
        let pos = k::half_space(&body.0, &plane, true)?.0.map(Body);
        let neg = k::half_space(&body.0, &plane, false)?.0.map(Body);
        Ok((pos, neg))
    })
}

/// Round the edges nearest to `edges` (points on them) with `radius`.
pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    let pts = edges.iter().map(|p| point(*p, "an edge point")).collect::<Result<Vec<_>>>()?;
    let radius = finite(radius, "the radius")?;
    if pts.is_empty() || !(radius > 1e-9) {
        return Err(err("select edges and give a positive radius"));
    }
    guarded("fillet", || Ok(Body(k::fillet(&body.0, &pts, radius)?)))
}

/// Bevel the edges nearest to `edges` (points on them) by `distance` on both faces.
pub fn chamfer(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    let pts = edges.iter().map(|p| point(*p, "an edge point")).collect::<Result<Vec<_>>>()?;
    let distance = finite(distance, "the distance")?;
    if pts.is_empty() || !(distance > 1e-9) {
        return Err(err("select edges and give a positive distance"));
    }
    guarded("chamfer", || Ok(Body(k::chamfer(&body.0, &pts, distance)?)))
}

/// Hollow a body: walls `thickness` thick inside it, the faces nearest to `open` removed. The
/// kernel shells convex bodies with planar faces.
pub fn shell(body: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
    let pts = open.iter().map(|p| point(*p, "a face point")).collect::<Result<Vec<_>>>()?;
    let thickness = finite(thickness, "the thickness")?;
    if !(thickness > 1e-9) {
        return Err(err("the shell thickness must be positive"));
    }
    guarded("shell", || Ok(Body(k::shell(&body.0, &pts, thickness)?)))
}

// ---- reading a body ----

impl Body {
    /// The body in SolidCraft's exact native format (`SCBREP04`): what DraftCraft saves.
    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(k::encode_native_body(&self.0)?)
    }

    /// A body from [`Body::encode`]'s bytes. The bytes come from a file: they are checked, and a
    /// body that doesn't decode is an error, never a panic.
    pub fn decode(bytes: &[u8]) -> Result<Body> {
        guarded("decode", || Ok(Body(k::decode_native_body(bytes)?)))
    }

    /// The body moved by `m` (row-major, `world = m * p`, as `draftcraft_geom::Mat4`): rotations,
    /// translations, reflections and scaling.
    pub fn transformed(&self, m: &Mat4) -> Result<Body> {
        let r = &m.m;
        // The kernel takes column-major matrices.
        let mut c = [[0.0; 4]; 4];
        for (i, row) in r.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                if let Some(cell) = c.get_mut(j).and_then(|col| col.get_mut(i)) {
                    *cell = *v;
                }
            }
        }
        guarded("transform", || Ok(Body(k::transform_matrix(&self.0, c)?)))
    }

    /// The body's size: the diagonal of its bounding box.
    pub fn size(&self) -> f64 {
        self.0.size()
    }

    pub fn face_count(&self) -> usize {
        self.0.face_count()
    }

    pub fn edge_count(&self) -> usize {
        self.0.edge_count()
    }

    /// Volume, surface area, centroid and bounding box.
    pub fn mass(&self) -> Result<Mass> {
        guarded("mass properties", || {
            let m = k::measure_mass(&self.0)?;
            Ok(Mass { volume: m.volume, area: m.area, centroid: from_sg(m.centroid), min: from_sg(m.bbox.min), max: from_sg(m.bbox.max) })
        })
    }

    /// Edges and triangles to draw it, to a chord tolerance of `tol` (0: 1/500 of its size).
    pub fn display(&self, tol: f64) -> Result<Display> {
        let tol = if tol.is_finite() && tol > 0.0 { tol } else { (self.size() / 500.0).max(1e-6) };
        guarded("display", || {
            let m = self.0.display_mesh(tol)?;
            Ok(Display {
                wires: m.edges.into_iter().map(|e| e.into_iter().map(from_sg).collect()).collect(),
                positions: m.positions.into_iter().map(from_sg).collect(),
                triangles: m.triangles,
                tri_face: m.tri_face,
            })
        })
    }

    /// Whether the two bodies touch or overlap (INTERFERE).
    pub fn meets(&self, other: &Body) -> Result<bool> {
        guarded("interference", || Ok(self.0.meets(&other.0)?))
    }

    /// The separate solids the body is made of.
    pub fn lumps(&self) -> Result<Vec<Body>> {
        guarded("lumps", || Ok(self.0.lumps()?.into_iter().map(Body).collect()))
    }

    /// Whether `p` is inside the body.
    pub fn contains(&self, p: Vec3) -> bool {
        self.0.contains(to_sg(p))
    }

    /// The edges as polylines, with their index.
    pub(crate) fn edge_polylines(&self, tol: f64) -> Result<Vec<(usize, Vec<Vec3>)>> {
        guarded("edges", || Ok(self.0.edges(tol)?.into_iter().map(|e| (e.index, e.points.into_iter().map(from_sg).collect())).collect()))
    }

    /// Edges `indices` exactly in `frame`'s coordinates (lines and arcs); `None` for an edge that
    /// doesn't lie in a plane parallel to it or isn't a line or an arc.
    pub(crate) fn planar_edges(&self, frame: &Frame, indices: &[usize]) -> Result<Vec<Option<Vec<Seg>>>> {
        let plane = frame.plane()?;
        let edges = self.0.planar_edges(&plane, indices);
        Ok(edges.into_iter().map(|e| e.and_then(|segs| segs.into_iter().map(from_seg2).collect::<Option<Vec<_>>>())).collect())
    }
}

/// A kernel segment as ours, for lines and arcs (others: `None`).
pub(crate) fn from_seg2(s: sg::Seg2) -> Option<Seg> {
    match s {
        sg::Seg2::Line { a, b } => Some(Seg::Line { a: Vec2::new(a.x, a.y), b: Vec2::new(b.x, b.y) }),
        sg::Seg2::Arc { center, radius, start, sweep } => Some(Seg::Arc { center: Vec2::new(center.x, center.y), radius, start, sweep }),
        _ => None,
    }
}
