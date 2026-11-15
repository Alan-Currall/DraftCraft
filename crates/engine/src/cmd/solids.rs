//! 3D solids (cargo feature `solids`, docs/solids.md): the commands that make, combine, edit,
//! measure and draw solids, by AutoCAD's names.
//!
//! Every kernel call goes through `draftcraft-solid`. A solid's body is kept in its own
//! coordinates near the origin, with a placement saying where it sits in the drawing ("Units and
//! scale" in docs/solids.md), so parts far from the origin keep the kernel's tolerances.

use draftcraft_doc::{Entity, EntityKind, Handle, Solid3d, SolidLook};
use draftcraft_geom::{Mat4, Vec2, Vec3};
use draftcraft_solid::{self as solid, Body};
use serde_json::{Value, json};

use super::CommandSpec;
use crate::{EngineError, Result, Session};

mod edit;
pub(crate) mod look;
mod prims;
mod sweep;
#[cfg(test)]
mod tests;
mod views;

pub(crate) use edit::{try_boolean, try_massprop, try_subtract};

/// Most solids whose look an opened drawing gets from their bodies; the others keep the look of
/// their saved meshes.
const MAX_REFRESHED: usize = 100_000;

/// Give every solid of `d` the look its exact body has (a solid read from a file has the look of
/// the mesh it was saved with). A body that can't be read keeps the mesh's look.
pub(crate) fn refresh_looks(d: &mut draftcraft_doc::Drawing) {
    let mut n = 0usize;
    let mut fix = |st: &mut draftcraft_doc::EntityStore| {
        let hs: Vec<Handle> = st.iter().filter(|e| matches!(e.kind, EntityKind::Solid3d(_))).map(|e| e.handle).collect();
        for h in hs {
            if n >= MAX_REFRESHED {
                return;
            }
            n += 1;
            let Some(EntityKind::Solid3d(so)) = st.get(h).map(|e| &e.kind) else { continue };
            let Ok(body) = Body::decode(&so.body) else { continue };
            let Ok(look) = body.display(0.0) else { continue };
            if look.triangles.len() > MAX_LOOK_TRIANGLES {
                continue;
            }
            let look = SolidLook { wires: look.wires, positions: look.positions, triangles: look.triangles, tri_face: look.tri_face };
            st.modify(h, |e| {
                if let EntityKind::Solid3d(so) = &mut e.kind {
                    so.look = std::sync::Arc::new(look);
                }
            });
        }
    };
    fix(&mut d.model);
    for l in &mut d.layouts {
        fix(&mut l.entities);
    }
    for b in d.blocks.values_mut() {
        if b.entities.iter().any(|e| matches!(e.kind, EntityKind::Solid3d(_))) {
            fix(&mut std::sync::Arc::make_mut(b).entities);
        }
    }
}

/// Whether a solid is saved as AutoCAD's own 3DSOLID (its body written as ACIS); the others are
/// saved as meshes with the exact body attached (docs/solids.md, "The ACIS translator").
pub(crate) fn saves_as_acis(so: &Solid3d) -> bool {
    let Ok(body) = Body::decode(&so.body) else { return false };
    let body = if is_identity(&so.placement) { Ok(body) } else { body.transformed(&so.placement) };
    body.is_ok_and(|b| solid::acis::write_sat(&b).is_ok())
}

/// The solids commands.
pub fn specs() -> Vec<CommandSpec> {
    let mut v = prims::specs();
    v.extend(sweep::specs());
    v.extend(edit::specs());
    v.extend(views::specs());
    v.extend(look::specs());
    v
}

pub(crate) fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

/// A kernel error as the command's error.
pub(crate) fn kernel(e: solid::SolidError) -> EngineError {
    EngineError::Other(e.0)
}

/// A 3D point parameter: `[x, y]` (at Z 0) or `[x, y, z]`.
pub(crate) fn point3(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    let z = match a.get(2) {
        None | Some(Value::Null) => 0.0,
        Some(z) => z.as_f64()?,
    };
    let p = Vec3::new(x, y, z);
    p.is_finite().then_some(p)
}

pub(crate) fn point3_req(cmd: &str, p: &Value, key: &str) -> Result<Vec3> {
    p.get(key).and_then(point3).ok_or_else(|| super::bad(cmd, format!("`{key}` ([x, y] or [x, y, z]) is required")))
}

pub(crate) fn translation(o: Vec3) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    m.m[0][3] = o.x;
    m.m[1][3] = o.y;
    m.m[2][3] = o.z;
    m
}

/// The inverse of an affine placement; `None` when it flattens.
pub(crate) fn inverse(m: &Mat4) -> Option<Mat4> {
    let a = &m.m;
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if !(det.is_finite() && det.abs() > 1e-300) {
        return None;
    }
    let inv = 1.0 / det;
    let mut r = [[0.0; 4]; 4];
    r[0][0] = (a[1][1] * a[2][2] - a[1][2] * a[2][1]) * inv;
    r[0][1] = (a[0][2] * a[2][1] - a[0][1] * a[2][2]) * inv;
    r[0][2] = (a[0][1] * a[1][2] - a[0][2] * a[1][1]) * inv;
    r[1][0] = (a[1][2] * a[2][0] - a[1][0] * a[2][2]) * inv;
    r[1][1] = (a[0][0] * a[2][2] - a[0][2] * a[2][0]) * inv;
    r[1][2] = (a[0][2] * a[1][0] - a[0][0] * a[1][2]) * inv;
    r[2][0] = (a[1][0] * a[2][1] - a[1][1] * a[2][0]) * inv;
    r[2][1] = (a[0][1] * a[2][0] - a[0][0] * a[2][1]) * inv;
    r[2][2] = (a[0][0] * a[1][1] - a[0][1] * a[1][0]) * inv;
    for row in r.iter_mut().take(3) {
        row[3] = -(row[0] * a[0][3] + row[1] * a[1][3] + row[2] * a[2][3]);
    }
    r[3][3] = 1.0;
    let out = Mat4 { m: r };
    out.m.iter().flatten().all(|v| v.is_finite()).then_some(out)
}

fn det3(m: &Mat4) -> f64 {
    let a = &m.m;
    a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
}

/// Most triangles kept to draw one solid (and to save it as a mesh AutoCAD can see).
pub(crate) const MAX_LOOK_TRIANGLES: usize = 200_000;

/// The entity for `body` (in its own coordinates) placed by `placement`.
pub(crate) fn solid_kind(body: &Body, placement: Mat4) -> Result<EntityKind> {
    let bytes = body.encode().map_err(kernel)?;
    let d = body.display(0.0).map_err(kernel)?;
    if d.triangles.len() > MAX_LOOK_TRIANGLES {
        return Err(other("The solid is too detailed to draw."));
    }
    let volume = body.mass().map(|m| m.volume).unwrap_or(0.0) * det3(&placement).abs();
    let look = SolidLook { wires: d.wires, positions: d.positions, triangles: d.triangles, tri_face: d.tri_face };
    Ok(EntityKind::Solid3d(Solid3d {
        body: std::sync::Arc::new(bytes),
        placement,
        look: std::sync::Arc::new(look),
        faces: u32::try_from(body.face_count()).unwrap_or(u32::MAX),
        edges: u32::try_from(body.edge_count()).unwrap_or(u32::MAX),
        volume,
    }))
}

/// A body made in drawing coordinates, kept relative to `origin` (so the placement is a move to
/// it): `make` receives `origin` and builds the body around the drawing origin.
pub(crate) fn placed(origin: Vec3, make: impl FnOnce(Vec3) -> solid::Result<Body>) -> Result<EntityKind> {
    let body = make(origin).map_err(kernel)?;
    solid_kind(&body, translation(origin))
}

/// The solid's body moved into the coordinates of `frame` (a placement): `inverse(frame) *
/// placement` applied to it.
pub(crate) fn body_in(sol: &Solid3d, frame: &Mat4) -> Result<Body> {
    let body = Body::decode(&sol.body).map_err(|e| other(format!("This solid's data can't be read: {e}")))?;
    let inv = inverse(frame).ok_or_else(|| other("The solid's placement is not valid."))?;
    let m = inv.mul(&sol.placement);
    if is_identity(&m) { Ok(body) } else { body.transformed(&m).map_err(kernel) }
}

fn is_identity(m: &Mat4) -> bool {
    m.m.iter().flatten().zip(Mat4::IDENTITY.m.iter().flatten()).all(|(a, b)| (a - b).abs() <= 1e-15)
}

/// The solids among `hs`, with their handles.
pub(crate) fn solids(s: &Session, hs: &[Handle]) -> Result<Vec<(Handle, Solid3d)>> {
    let d = s.doc()?;
    Ok(hs
        .iter()
        .filter_map(|h| match &d.entity(*h)?.kind {
            EntityKind::Solid3d(so) => Some((*h, so.clone())),
            _ => None,
        })
        .collect())
}

/// Replace entity `h`'s geometry, keeping its layer and other properties.
pub(crate) fn replace(s: &mut Session, h: Handle, kind: EntityKind) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| e.kind = kind)?;
    Ok(())
}

/// Add `kind` with the properties of entity `like` (layer, colour, …).
pub(crate) fn add_like(s: &mut Session, like: Handle, kind: EntityKind) -> Result<Handle> {
    let common = s.doc()?.entity(like).map(|e| e.common.clone());
    let h = s.add_entity(kind)?;
    if let Some(c) = common {
        s.doc_mut()?.modify_entity(h, |e: &mut Entity| e.common = c)?;
    }
    Ok(h)
}

pub(crate) fn added(h: Handle) -> Result<Value> {
    Ok(json!({ "handle": h.hex() }))
}

/// The solid whose edges (seen from above) pass nearest to `p`, among `hs` (all solids when
/// empty), and the 3D point on that edge nearest to `p` in plan, in drawing coordinates.
pub(crate) fn pick_edge(s: &Session, p: Vec2, hs: &[Handle]) -> Result<Option<(Handle, Vec3)>> {
    let d = s.doc()?;
    let mut best: Option<(f64, Handle, Vec3)> = None;
    let Some(store) = d.space(&s.space()) else { return Ok(None) };
    for e in store.iter() {
        let EntityKind::Solid3d(so) = &e.kind else { continue };
        if !hs.is_empty() && !hs.contains(&e.handle) {
            continue;
        }
        for w in so.world_wires() {
            for seg in w.windows(2) {
                let (Some(a), Some(b)) = (seg.first(), seg.get(1)) else { continue };
                let (a2, b2) = (a.xy(), b.xy());
                let ab = b2 - a2;
                let t = if ab.dot(ab) > 0.0 { ((p - a2).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0) } else { 0.0 };
                let q = a2 + ab * t;
                let dist = q.dist(p);
                if best.is_none_or(|(bd, ..)| dist < bd) {
                    best = Some((dist, e.handle, *a + (*b - *a) * t));
                }
            }
        }
    }
    Ok(best.map(|(_, h, q)| (h, q)))
}

/// The point of solid `so`'s faces seen from above at `p` (the topmost face under it), in
/// drawing coordinates.
pub(crate) fn pick_face(so: &Solid3d, p: Vec2) -> Option<Vec3> {
    let mut best: Option<Vec3> = None;
    for t in &so.look.triangles {
        let Some([a, b, c]) = so.look.triangle(t) else { continue };
        let (a, b, c) = (so.world(a), so.world(b), so.world(c));
        let (a2, b2, c2) = (a.xy(), b.xy(), c.xy());
        let den = (b2 - a2).cross(c2 - a2);
        if den.abs() < 1e-300 {
            continue;
        }
        let u = (p - a2).cross(c2 - a2) / den;
        let v = (b2 - a2).cross(p - a2) / den;
        if u < -1e-9 || v < -1e-9 || u + v > 1.0 + 1e-9 {
            continue;
        }
        let q = a + (b - a) * u + (c - a) * v;
        if best.is_none_or(|bq| q.z > bq.z) {
            best = Some(q);
        }
    }
    best
}
