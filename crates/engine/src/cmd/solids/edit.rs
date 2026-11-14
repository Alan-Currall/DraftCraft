//! Combining, cutting and editing solids: UNION, SUBTRACT and INTERSECT (through the region
//! commands, which hand solids over here), SLICE, FILLETEDGE, CHAMFEREDGE, SOLIDEDIT (Body ▸
//! Shell), INTERFERE and MASSPROP's solids part.

use draftcraft_doc::{EntityKind, Handle, Solid3d};
use draftcraft_geom::{Vec2, Vec3};
use draftcraft_solid::{self as solid, Body, BoolOp};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase, number};
use super::super::{CommandSpec, bad, f64_req, point_value, str_param, targets};
use super::{add_like, body_in, other, pick_edge, pick_face, point3, point3_req, replace, solid_kind, solids, translation};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("slice", "Slice", run_slice)
            .menu(&["Modify", "3D Operations", "Slice"])
            .alias(&["sl"])
            .params("{handles?, origin: [x, y, z], normal: [x, y, z] | points: [p1, p2] (a vertical plane through them), keep?: \"both\" | point [x, y, z] on the side to keep} → {handles}")
            .interactive(|_| Ok(Box::new(SliceM::default()))),
        CommandSpec::new("filletedge", "Fillet Edge", run_filletedge)
            .menu(&["Modify", "Solid Editing", "Fillet Edges"])
            .params("{handle?, edges: [[x, y, z] | [x, y], …] (a point on each edge), radius} → {handle}")
            .interactive(|_| Ok(Box::new(EdgeM::new(true)))),
        CommandSpec::new("chamferedge", "Chamfer Edge", run_chamferedge)
            .menu(&["Modify", "Solid Editing", "Chamfer Edges"])
            .params("{handle?, edges: [[x, y, z] | [x, y], …], distance} → {handle}")
            .interactive(|_| Ok(Box::new(EdgeM::new(false)))),
        CommandSpec::new("solidedit", "Solid Editing", run_solidedit)
            .menu(&["Modify", "Solid Editing", "Shell"])
            .params("{handle, operation: \"shell\", thickness, remove?: [[x, y, z] | [x, y], …] (a point on each face to open)} → {handle}")
            .interactive(|_| Ok(Box::new(ShellM::default()))),
        CommandSpec::new("interfere", "Interference Checking", run_interfere)
            .menu(&["Modify", "3D Operations", "Interference Checking"])
            .alias(&["inf"])
            .params("{first: [handles], second?: [handles], create?: bool} → {pairs: [{a, b, volume, handle?}]}")
            .interactive(|_| Ok(Box::new(InterfereM::default()))),
    ]
}

/// The solid's body in drawing directions and size, relative to where its own origin sits in
/// the drawing (so the kernel works near the origin), and that point.
pub(crate) fn world_body(sol: &Solid3d) -> Result<(Body, Vec3)> {
    let o = sol.world(Vec3::ZERO);
    Ok((body_in(sol, &translation(o))?, o))
}

/// UNION and INTERSECT of the solids among `hs`. `None` when there are none, or when regions are
/// selected too (the region command then combines those).
pub(crate) fn try_boolean(s: &mut Session, hs: &[Handle], union: bool) -> Result<Option<Value>> {
    let sols = solids(s, hs)?;
    let Some(((h0, first), rest)) = sols.split_first() else { return Ok(None) };
    let op = if union { BoolOp::Union } else { BoolOp::Intersect };
    let mut acc = Some(body_in(first, &first.placement)?);
    for (_, so) in rest {
        let b = body_in(so, &first.placement)?;
        acc = match acc {
            Some(a) => solid::boolean(&a, &b, op).map_err(super::kernel)?,
            None => None,
        };
    }
    let others: Vec<Handle> = sols.iter().skip(1).map(|(h, _)| *h).collect();
    let v = finish(s, *h0, acc, first.placement, &others)?;
    Ok(if has_regions(s, hs)? { None } else { Some(v) })
}

/// SUBTRACT with solids: the `remove` solids taken from the union of the `from` solids.
pub(crate) fn try_subtract(s: &mut Session, from: &[Handle], remove: &[Handle]) -> Result<Option<Value>> {
    let (a, b) = (solids(s, from)?, solids(s, remove)?);
    let Some(((h0, first), rest)) = a.split_first() else { return Ok(None) };
    let mut acc = Some(body_in(first, &first.placement)?);
    for (_, so) in rest {
        let x = body_in(so, &first.placement)?;
        acc = match acc {
            Some(a) => solid::boolean(&a, &x, BoolOp::Union).map_err(super::kernel)?,
            None => Some(x),
        };
    }
    for (h, so) in &b {
        if a.iter().any(|(x, _)| x == h) {
            return Err(other("A solid can't be subtracted from itself."));
        }
        let x = body_in(so, &first.placement)?;
        acc = match acc {
            Some(a) => solid::boolean(&a, &x, BoolOp::Subtract).map_err(super::kernel)?,
            None => None,
        };
    }
    let others: Vec<Handle> = a.iter().skip(1).chain(&b).map(|(h, _)| *h).collect();
    let v = finish(s, *h0, acc, first.placement, &others)?;
    Ok(if has_regions(s, from)? || has_regions(s, remove)? { None } else { Some(v) })
}

fn has_regions(s: &Session, hs: &[Handle]) -> Result<bool> {
    let d = s.doc()?;
    Ok(hs.iter().any(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Region(_)))))
}

/// Keep the result in entity `keep` (or erase it when the result is empty) and erase `others`.
fn finish(s: &mut Session, keep: Handle, result: Option<Body>, placement: draftcraft_geom::Mat4, others: &[Handle]) -> Result<Value> {
    let empty = result.is_none();
    match result {
        Some(b) => replace(s, keep, solid_kind(&b, placement)?)?,
        None => {
            s.doc_mut()?.remove_entity(keep);
        }
    }
    let d = s.doc_mut()?;
    for h in others {
        d.remove_entity(*h);
    }
    Ok(json!({
        "handle": if empty { Value::Null } else { json!(keep.hex()) },
        "message": if empty { "The result is empty: the solids were erased." } else { "" },
    }))
}

/// MASSPROP for the solids among `hs`; `None` when there are none or regions are selected too.
pub(crate) fn try_massprop(s: &mut Session, hs: &[Handle]) -> Result<Option<Value>> {
    let sols = solids(s, hs)?;
    if sols.is_empty() || has_regions(s, hs)? {
        return Ok(None);
    }
    let f = |v: f64| super::super::inquiry::fmt_distance(s, v);
    let mut out = Vec::new();
    let mut lines = Vec::new();
    for (h, so) in &sols {
        let (body, o) = world_body(so)?;
        let m = body.mass().map_err(super::kernel)?;
        let (c, lo, hi) = (m.centroid + o, m.min + o, m.max + o);
        out.push(json!({
            "handle": h.hex(), "mass": m.volume, "volume": m.volume, "area": m.area,
            "boundingBox": [[lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]], "centroid": [c.x, c.y, c.z],
        }));
        lines.push(format!(
            "----------------   SOLIDS   ----------------\nMass:                    {}\nVolume:                  {}\nSurface area:            {}\nBounding box:         X: {}  --  {}\n                      Y: {}  --  {}\n                      Z: {}  --  {}\nCentroid:             X: {}\n                      Y: {}\n                      Z: {}\n(Mass at density 1. Moments of inertia of solids are not available yet.)",
            f(m.volume),
            f(m.volume),
            f(m.area),
            f(lo.x),
            f(hi.x),
            f(lo.y),
            f(hi.y),
            f(lo.z),
            f(hi.z),
            f(c.x),
            f(c.y),
            f(c.z),
        ));
    }
    Ok(Some(json!({ "solids": out, "message": lines.join("\n") })))
}

// ---- SLICE ---------------------------------------------------------------------------------

/// Which part of a sliced solid to keep.
#[derive(Clone, Copy)]
enum Keep {
    Both,
    Side(Vec3),
}

fn slice(s: &mut Session, hs: &[Handle], origin: Vec3, normal: Vec3, keep: Keep) -> Result<Value> {
    let sols = solids(s, hs)?;
    if sols.is_empty() {
        return Err(other("Select 3D solids to slice."));
    }
    let n = normal.normalized();
    if !(n.is_finite() && n.len() > 0.5) {
        return Err(other("The slicing plane isn't valid."));
    }
    let mut made = Vec::new();
    for (h, so) in sols {
        let (body, o) = world_body(&so)?;
        let (pos, neg) = solid::slice(&body, origin - o, n).map_err(super::kernel)?;
        let keep_pos = match keep {
            Keep::Both => None,
            Keep::Side(p) => Some((p - origin).dot(n) >= 0.0),
        };
        let parts: Vec<Body> = match keep_pos {
            None => pos.into_iter().chain(neg).collect(),
            Some(true) => pos.into_iter().collect(),
            Some(false) => neg.into_iter().collect(),
        };
        let mut parts = parts.into_iter();
        match parts.next() {
            // The plane misses the solid on the kept side: it's gone, as in AutoCAD.
            None => {
                s.doc_mut()?.remove_entity(h);
            }
            Some(first) => {
                replace(s, h, solid_kind(&first, translation(o))?)?;
                made.push(h);
                for b in parts {
                    made.push(add_like(s, h, solid_kind(&b, translation(o))?)?);
                }
            }
        }
    }
    Ok(json!({ "handles": made.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

/// A vertical plane through two points in plan: origin and normal.
fn vertical_plane(a: Vec2, b: Vec2) -> Result<(Vec3, Vec3)> {
    let d = b - a;
    if d.len() <= 1e-12 {
        return Err(other("The two points of the plane must differ."));
    }
    Ok((a.to3(0.0), Vec3::new(-d.y, d.x, 0.0)))
}

fn run_slice(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let (origin, normal) = match p.get("points").and_then(Value::as_array) {
        Some(pts) => {
            let a = pts.first().and_then(point_value).ok_or_else(|| bad("slice", "`points` needs two points"))?;
            let b = pts.get(1).and_then(point_value).ok_or_else(|| bad("slice", "`points` needs two points"))?;
            vertical_plane(a, b)?
        }
        None => (point3_req("slice", p, "origin")?, point3_req("slice", p, "normal")?),
    };
    let keep = match p.get("keep") {
        None | Some(Value::Null) => Keep::Both,
        Some(Value::String(t)) if t.eq_ignore_ascii_case("both") => Keep::Both,
        Some(v) => Keep::Side(point3(v).ok_or_else(|| bad("slice", "`keep` is \"both\" or a point on the side to keep"))?),
    };
    slice(s, &hs, origin, normal, keep)
}

#[derive(Default)]
struct SliceM {
    sel: SelectPhase,
    picked: Option<Vec<Handle>>,
    /// A plane parallel to XY, YZ or ZX: its normal, waiting for a point on it.
    axis_plane: Option<Vec3>,
    first: Option<Vec2>,
    plane: Option<(Vec3, Vec3)>,
}

impl Interactive for SliceM {
    fn name(&self) -> &'static str {
        "SLICE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.picked = Some(self.sel.picked.clone());
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.picked.is_none() {
            let mut p = self.sel.prompt();
            p.message = "Select objects to slice".into();
            return p;
        }
        match (self.plane, self.axis_plane, self.first) {
            (Some(_), ..) => Prompt::new("Specify a point on desired side", Accept::POINT).kw(&["keep Both sides"]).default("Both"),
            (None, Some(_), _) => Prompt::new("Specify a point on the plane", Accept::POINT).default("0,0,0"),
            (None, None, Some(a)) => Prompt::new("Specify second point on plane", Accept::POINT).base(a),
            (None, None, None) => Prompt::new("Specify start point of slicing plane", Accept::POINT).kw(&["XY", "YZ", "ZX"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.picked.is_none() {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.picked = Some(hs);
                    Ok(Step::Continue)
                }
            };
        }
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        if let Some((o, n)) = self.plane {
            let keep = match i {
                Input::Point(p) => Keep::Side(p.to3(o.z)),
                Input::Keyword(_) | Input::Enter => Keep::Both,
                _ => return Ok(Step::Continue),
            };
            let hs = self.picked.clone().unwrap_or_default();
            slice(s, &hs, o, n, keep)?;
            s.set_selection(Vec::new());
            return Ok(Step::Done);
        }
        if let Some(n) = self.axis_plane {
            let o = match i {
                Input::Point(p) => p.to3(0.0),
                Input::Enter => Vec3::ZERO,
                _ => return Ok(Step::Continue),
            };
            self.plane = Some((o, n));
            return Ok(Step::Continue);
        }
        match (self.first, i) {
            (None, Input::Keyword(k)) => {
                self.axis_plane = Some(match k.to_ascii_uppercase().as_str() {
                    "XY" => Vec3::Z,
                    "YZ" => Vec3::X,
                    _ => Vec3::Y,
                });
            }
            (None, Input::Point(p)) => self.first = Some(p),
            (Some(a), Input::Point(b)) => self.plane = Some(vertical_plane(a, b)?),
            _ => {}
        }
        Ok(Step::Continue)
    }
}

// ---- FILLETEDGE, CHAMFEREDGE -----------------------------------------------------------------

/// Points on edges: `[x, y, z]`, or `[x, y]` picked in plan (the solid edge nearest it).
fn edge_points(s: &Session, p: &Value, handle: Option<Handle>) -> Result<(Handle, Vec<Vec3>)> {
    let arr = p.get("edges").and_then(Value::as_array).ok_or_else(|| bad("filletedge", "`edges` (points on the edges) is required"))?;
    if arr.is_empty() || arr.len() > 10_000 {
        return Err(bad("filletedge", "`edges` needs 1 to 10000 points"));
    }
    let only: Vec<Handle> = handle.into_iter().collect();
    let mut owner = handle;
    let mut pts = Vec::with_capacity(arr.len());
    for v in arr {
        let q = point3(v).ok_or_else(|| bad("filletedge", format!("`edges`: {v} is not a point")))?;
        let is3 = v.as_array().is_some_and(|a| a.len() >= 3);
        let (h, at) = if is3 && owner.is_some() {
            (owner.unwrap_or_default(), q)
        } else {
            pick_edge(s, q.xy(), &only)?.ok_or_else(|| other("No solid edge there."))?
        };
        if owner.is_some_and(|o| o != h) {
            return Err(other("All edges must belong to one solid."));
        }
        owner = Some(h);
        pts.push(at);
    }
    let h = owner.ok_or_else(|| other("Select edges of a 3D solid."))?;
    Ok((h, pts))
}

fn round_edges(s: &mut Session, h: Handle, pts: &[Vec3], size: f64, fillet: bool) -> Result<Value> {
    let so = solids(s, &[h])?.into_iter().next().map(|(_, so)| so).ok_or_else(|| other("That is not a 3D solid."))?;
    let (body, o) = world_body(&so)?;
    let local: Vec<Vec3> = pts.iter().map(|p| *p - o).collect();
    let out = if fillet { solid::fillet(&body, &local, size) } else { solid::chamfer(&body, &local, size) }.map_err(super::kernel)?;
    replace(s, h, solid_kind(&out, translation(o))?)?;
    super::added(h)
}

fn handle_param(p: &Value) -> Option<Handle> {
    p.get("handle").and_then(super::super::curves::handle_value)
}

fn run_filletedge(s: &mut Session, p: &Value) -> Result<Value> {
    let radius = f64_req("filletedge", p, "radius")?;
    let (h, pts) = edge_points(s, p, handle_param(p))?;
    round_edges(s, h, &pts, radius, true)
}

fn run_chamferedge(s: &mut Session, p: &Value) -> Result<Value> {
    let d = f64_req("chamferedge", p, "distance")?;
    let (h, pts) = edge_points(s, p, handle_param(p))?;
    round_edges(s, h, &pts, d, false)
}

/// FILLETEDGE and CHAMFEREDGE at the command line: the size first (as an option, like AutoCAD),
/// then edges, Enter to apply.
struct EdgeM {
    fillet: bool,
    size: f64,
    asking_size: bool,
    owner: Option<Handle>,
    edges: Vec<Vec3>,
}

impl EdgeM {
    fn new(fillet: bool) -> Self {
        EdgeM { fillet, size: 1.0, asking_size: false, owner: None, edges: Vec::new() }
    }
}

impl Interactive for EdgeM {
    fn name(&self) -> &'static str {
        if self.fillet { "FILLETEDGE" } else { "CHAMFEREDGE" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking_size {
            let m = if self.fillet { "Enter fillet radius" } else { "Specify distance" };
            return Prompt::new(m, Accept::NUMBER).default(format!("{}", self.size));
        }
        let kw = if self.fillet { "Radius" } else { "Distance" };
        let m = if self.edges.is_empty() { "Select an edge" } else { "Select an edge or press Enter to apply" };
        Prompt::new(m, Accept::POINT).kw(&[kw])
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_size {
            self.asking_size = false;
            if let Input::Text(t) = &i {
                self.size = number(t).filter(|v| *v > 0.0).ok_or_else(|| other("Requires a positive value."))?;
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Keyword(_) => self.asking_size = true,
            Input::Point(p) => {
                let only: Vec<Handle> = self.owner.into_iter().collect();
                match pick_edge(s, p, &only)? {
                    Some((h, q)) => {
                        self.owner = Some(h);
                        self.edges.push(q);
                    }
                    None => s.echo("No solid edge there."),
                }
            }
            Input::Enter => {
                let Some(h) = self.owner else { return Ok(Step::Done) };
                round_edges(s, h, &self.edges, self.size, self.fillet)?;
                return Ok(Step::Done);
            }
            Input::Cancel => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
}

// ---- SOLIDEDIT ▸ Body ▸ Shell -----------------------------------------------------------------

fn shell(s: &mut Session, h: Handle, remove: &[Vec2], thickness: f64) -> Result<Value> {
    let so = solids(s, &[h])?.into_iter().next().map(|(_, so)| so).ok_or_else(|| other("That is not a 3D solid."))?;
    let mut open = Vec::with_capacity(remove.len());
    for p in remove {
        open.push(pick_face(&so, *p).ok_or_else(|| other("No face of the solid there."))?);
    }
    let (body, o) = world_body(&so)?;
    let local: Vec<Vec3> = open.iter().map(|p| *p - o).collect();
    let out = solid::shell(&body, &local, thickness).map_err(super::kernel)?;
    replace(s, h, solid_kind(&out, translation(o))?)?;
    super::added(h)
}

fn run_solidedit(s: &mut Session, p: &Value) -> Result<Value> {
    let op = str_param(p, "operation").unwrap_or("shell");
    if !op.eq_ignore_ascii_case("shell") {
        return Err(other(format!("SOLIDEDIT {op} is not available yet.")));
    }
    let h = handle_param(p).ok_or_else(|| bad("solidedit", "`handle` is required"))?;
    let thickness = f64_req("solidedit", p, "thickness")?;
    let remove: Vec<Vec2> = match p.get("remove").and_then(Value::as_array) {
        Some(a) if a.len() > 10_000 => return Err(bad("solidedit", "`remove` has too many points")),
        Some(a) => {
            a.iter().map(|v| point_value(v).ok_or_else(|| bad("solidedit", format!("`remove`: {v} is not a point")))).collect::<Result<_>>()?
        }
        None => Vec::new(),
    };
    shell(s, h, &remove, thickness)
}

#[derive(Default)]
struct ShellM {
    /// 0 option, 1 body option, 2 select solid, 3 remove faces, 4 distance.
    stage: u8,
    solid: Option<Handle>,
    remove: Vec<Vec2>,
}

impl Interactive for ShellM {
    fn name(&self) -> &'static str {
        "SOLIDEDIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            0 => Prompt::new("Enter a solids editing option", Accept::TEXT).kw(&["Body", "eXit"]).default("Body"),
            1 => Prompt::new("Enter a body editing option", Accept::TEXT).kw(&["Shell", "eXit"]).default("Shell"),
            2 => Prompt::new("Select a 3D solid", Accept::SELECT),
            3 => Prompt::new("Remove faces or press Enter", Accept::POINT),
            _ => Prompt::new("Enter the shell offset distance", Accept::NUMBER),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        let word = match &i {
            Input::Keyword(k) | Input::Text(k) => k.to_ascii_lowercase(),
            _ => String::new(),
        };
        match self.stage {
            0 | 1 => {
                if word.starts_with('x') || word == "exit" {
                    return Ok(Step::Done);
                }
                self.stage += 1;
            }
            2 => {
                let picked = match i {
                    Input::Pick(hs) => hs,
                    Input::Point(p) => pick_edge(s, p, &[])?.map(|(h, _)| vec![h]).unwrap_or_default(),
                    _ => Vec::new(),
                };
                if let Some((h, _)) = solids(s, &picked)?.into_iter().next() {
                    self.solid = Some(h);
                    self.stage = 3;
                }
            }
            3 => match i {
                Input::Point(p) => self.remove.push(p),
                Input::Enter => self.stage = 4,
                _ => {}
            },
            _ => {
                let Input::Text(t) = i else { return Ok(Step::Continue) };
                let d = number(&t).ok_or_else(|| other("Requires a distance."))?;
                let h = self.solid.ok_or_else(|| other("Select a 3D solid."))?;
                shell(s, h, &self.remove, d)?;
                return Ok(Step::Done);
            }
        }
        Ok(Step::Continue)
    }
}

// ---- INTERFERE -----------------------------------------------------------------------------

/// Most solid pairs INTERFERE compares.
const MAX_PAIRS: usize = 1_000_000;

fn interfere(s: &mut Session, first: &[Handle], second: &[Handle], create: bool) -> Result<Value> {
    let a = solids(s, first)?;
    let b = solids(s, second)?;
    if a.is_empty() {
        return Err(other("Select 3D solids."));
    }
    let mut pairs: Vec<(usize, usize, bool)> = Vec::new();
    if b.is_empty() {
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                pairs.push((i, j, false));
            }
        }
    } else {
        for i in 0..a.len() {
            for j in 0..b.len() {
                pairs.push((i, j, true));
            }
        }
    }
    if pairs.len() > MAX_PAIRS {
        return Err(other("Too many solids to compare."));
    }
    let mut found = Vec::new();
    let mut total = 0.0;
    for (i, j, cross) in pairs {
        let (Some((ha, sa)), Some((hb, sb))) = (a.get(i), if cross { b.get(j) } else { a.get(j) }) else { continue };
        if ha == hb {
            continue;
        }
        let x = body_in(sa, &sa.placement)?;
        let y = body_in(sb, &sa.placement)?;
        if !x.meets(&y).map_err(super::kernel)? {
            continue;
        }
        let Some(common) = solid::boolean(&x, &y, BoolOp::Intersect).map_err(super::kernel)? else { continue };
        let kind = solid_kind(&common, sa.placement)?;
        let volume = match &kind {
            EntityKind::Solid3d(so) => so.volume,
            _ => 0.0,
        };
        if volume <= 0.0 {
            continue;
        }
        total += volume;
        let made = if create { Some(add_like(s, *ha, kind)?.hex()) } else { None };
        found.push(json!({ "a": ha.hex(), "b": hb.hex(), "volume": volume, "handle": made }));
    }
    let message = if found.is_empty() {
        "Objects do not interfere".to_string()
    } else {
        format!("Interfering pairs: {}  Total interference volume: {}", found.len(), super::super::inquiry::fmt_distance(s, total))
    };
    Ok(json!({ "pairs": found, "message": message }))
}

fn run_interfere(s: &mut Session, p: &Value) -> Result<Value> {
    let list = |key: &str| -> Result<Vec<Handle>> {
        match p.get(key).and_then(Value::as_array) {
            Some(a) => a
                .iter()
                .map(|v| super::super::curves::handle_value(v).ok_or_else(|| bad("interfere", format!("`{key}`: {v} is not a handle"))))
                .collect(),
            None => Ok(Vec::new()),
        }
    };
    let mut first = list("first")?;
    if first.is_empty() {
        first = targets(s, p)?;
    }
    let second = list("second")?;
    let create = p.get("create").and_then(Value::as_bool).unwrap_or(false);
    interfere(s, &first, &second, create)
}

#[derive(Default)]
struct InterfereM {
    sel: SelectPhase,
    first: Option<Vec<Handle>>,
}

impl Interactive for InterfereM {
    fn name(&self) -> &'static str {
        "INTERFERE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let mut p = self.sel.prompt();
        p.message = if self.first.is_none() {
            "Select first set of objects".into()
        } else {
            "Select second set of objects or press Enter to check the first set".into()
        };
        p
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let outcome = self.sel.feed(s, &i)?;
        let hs = match outcome {
            SelOutcome::More => return Ok(Step::Continue),
            SelOutcome::Empty => Vec::new(),
            SelOutcome::Done(hs) => hs,
        };
        match self.first.take() {
            None if hs.is_empty() => Ok(Step::Done),
            None => {
                self.first = Some(hs);
                self.sel = SelectPhase::default();
                s.set_selection(Vec::new());
                Ok(Step::Continue)
            }
            Some(first) => {
                let r = interfere(s, &first, &hs, true)?;
                if let Some(m) = r.get("message").and_then(Value::as_str) {
                    s.echo(m.to_string());
                }
                s.set_selection(Vec::new());
                Ok(Step::Done)
            }
        }
    }
}
