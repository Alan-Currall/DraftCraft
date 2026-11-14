//! How a DraftCraft 3D solid is kept in DXF and DWG files until DraftCraft writes ACIS
//! (docs/solids.md, "Interim file form"): a polyface mesh every CAD program can see, and in the
//! mesh's extension dictionary an XRECORD keyed `DRAFTCRAFT_SOLID` holding the exact body in
//! SolidCraft's native format, its placement and its counts. DraftCraft (built with solids)
//! reopens the mesh as the solid; other programs, and DraftCraft without solids, see the mesh.
//!
//! A mesh larger than a polyface's 16-bit counts is written as several meshes in a row; the first
//! carries the record, which says how many there are.

use std::collections::HashMap;

use draftcraft_doc::{PolyfaceMesh, Solid3d};
use draftcraft_geom::Vec3;

/// The extension dictionary key of the record.
pub(crate) const KEY: &str = "DRAFTCRAFT_SOLID";
/// The record's first group (1): what it is and its layout's version.
pub(crate) const MARK: &str = "DraftCraft solid 1";
/// Bytes per binary chunk (group 310), as AutoCAD writes them.
const CHUNK: usize = 127;
/// Most vertices or faces of one polyface mesh (16-bit counts).
const MAX_MESH: usize = 32_767;

/// The meshes AutoCAD sees, in drawing coordinates: shared corners merged, each triangle a face,
/// and the edges inside a face of the solid hidden (negative indices), so only the solid's own
/// edges show.
pub(crate) fn meshes(so: &Solid3d) -> Vec<PolyfaceMesh> {
    let look = &so.look;
    // Edges inside a face: shared by two triangles of that face.
    let key = |p: Vec3| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
    let mut shared: HashMap<((u64, u64, u64), (u64, u64, u64), u32), u32> = HashMap::new();
    let tris: Vec<([Vec3; 3], u32)> = look
        .triangles
        .iter()
        .enumerate()
        .filter_map(|(i, t)| Some((look.triangle(t)?, look.tri_face.get(i).copied().unwrap_or(u32::MAX))))
        .collect();
    let edge = |a: Vec3, b: Vec3, f: u32| {
        let (ka, kb) = (key(a), key(b));
        if ka <= kb { (ka, kb, f) } else { (kb, ka, f) }
    };
    for (c, f) in &tris {
        for k in 0..3 {
            let (a, b) = (c[k], c[(k + 1) % 3]);
            *shared.entry(edge(a, b, *f)).or_insert(0) += 1;
        }
    }
    let mut out = Vec::new();
    let mut cur = PolyfaceMesh { vertices: Vec::new(), faces: Vec::new() };
    let mut index: HashMap<(u64, u64, u64), i32> = HashMap::new();
    for (c, f) in &tris {
        if cur.vertices.len() + 3 > MAX_MESH || cur.faces.len() + 1 > MAX_MESH {
            out.push(std::mem::replace(&mut cur, PolyfaceMesh { vertices: Vec::new(), faces: Vec::new() }));
            index.clear();
        }
        let mut face = [0i32; 4];
        for k in 0..3 {
            let p = c[k];
            let ix = *index.entry(key(p)).or_insert_with(|| {
                cur.vertices.push(so.world(p));
                i32::try_from(cur.vertices.len()).unwrap_or(i32::MAX)
            });
            let hidden = shared.get(&edge(p, c[(k + 1) % 3], *f)).is_some_and(|n| *n >= 2);
            face[k] = if hidden { -ix } else { ix };
        }
        cur.faces.push(face);
    }
    if !cur.faces.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

/// The record's groups after its cloning flag (280): mark, mesh count, counts, placement, body.
pub(crate) fn record_groups(so: &Solid3d, parts: usize) -> Vec<(i32, Value)> {
    let mut g = vec![(1, Value::S(MARK.into())), (90, Value::I(i64::try_from(parts).unwrap_or(1)))];
    g.push((91, Value::I(i64::from(so.faces))));
    g.push((92, Value::I(i64::from(so.edges))));
    g.push((140, Value::F(so.volume)));
    for row in &so.placement.m {
        for v in row {
            g.push((40, Value::F(*v)));
        }
    }
    g.push((93, Value::I(i64::try_from(so.body.len()).unwrap_or(0))));
    for c in so.body.chunks(CHUNK) {
        g.push((310, Value::S(c.iter().map(|b| format!("{b:02X}")).collect())));
    }
    g
}

/// A group's value.
pub(crate) enum Value {
    S(String),
    I(i64),
    F(f64),
}

#[cfg(feature = "solids")]
pub(crate) use recover::recover;

#[cfg(feature = "solids")]
mod recover {
    use std::sync::Arc;

    use draftcraft_doc::foreign::Foreign;
    use draftcraft_doc::{Drawing, EntityKind, EntityStore, Handle, PolyfaceMesh, RawTag, Solid3d, SolidLook};
    use draftcraft_geom::Mat4;

    use super::{KEY, MARK};

    /// Most bytes of one solid's body read back (the writer has no such solids; a file may claim it).
    const MAX_BODY: usize = 256 << 20;

    /// What a record holds.
    struct Record {
        parts: usize,
        faces: u32,
        edges: u32,
        volume: f64,
        placement: Mat4,
        body: Vec<u8>,
    }

    fn parse(tags: &[RawTag]) -> Option<Record> {
        if !tags.iter().any(|t| t.code == 1 && t.value.trim() == MARK) {
            return None;
        }
        let int = |code: i32| tags.iter().find(|t| t.code == code).and_then(|t| t.value.trim().parse::<i64>().ok());
        let reals: Vec<f64> = tags.iter().filter(|t| t.code == 40).filter_map(|t| t.value.trim().parse::<f64>().ok()).collect();
        if reals.len() != 16 || reals.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let mut m = [[0.0; 4]; 4];
        for (i, v) in reals.iter().enumerate() {
            *m.get_mut(i / 4)?.get_mut(i % 4)? = *v;
        }
        let len = usize::try_from(int(93)?).ok().filter(|n| *n <= MAX_BODY)?;
        let mut body = Vec::with_capacity(len);
        for t in tags.iter().filter(|t| t.code == 310) {
            let s = t.value.trim().as_bytes();
            if s.len() % 2 != 0 {
                return None;
            }
            for pair in s.chunks(2) {
                let txt = std::str::from_utf8(pair).ok()?;
                body.push(u8::from_str_radix(txt, 16).ok()?);
                if body.len() > len {
                    return None;
                }
            }
        }
        if body.len() != len {
            return None;
        }
        let parts = usize::try_from(int(90).unwrap_or(1)).ok().filter(|n| (1..=10_000).contains(n))?;
        let volume = tags.iter().find(|t| t.code == 140).and_then(|t| t.value.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0);
        let count = |code| u32::try_from(int(code).unwrap_or(0)).unwrap_or(0);
        Some(Record { parts, faces: count(91), edges: count(92), volume, placement: Mat4 { m }, body })
    }

    /// The record an entity's extension dictionary holds under [`KEY`]: the dictionary's and the
    /// record's handles, and the record.
    fn find(f: &Foreign, h: Handle) -> Option<(Handle, Handle, Record)> {
        let dh = f.entities.get(&h)?.xdict?;
        let dict = f.objects.iter().find(|o| o.handle == dh)?;
        let at = dict.tags.iter().position(|t| t.code == 3 && t.value.trim().eq_ignore_ascii_case(KEY))?;
        let xh = dict.tags.get(at + 1).filter(|t| matches!(t.code, 350 | 360)).and_then(|t| Handle::parse_hex(t.value.trim()))?;
        let rec = f.objects.iter().find(|o| o.handle == xh && o.kind.eq_ignore_ascii_case("XRECORD"))?;
        Some((dh, xh, parse(&rec.tags)?))
    }

    /// The inverse of an affine placement; `None` when it flattens.
    fn inverse(m: &Mat4) -> Option<Mat4> {
        let a = &m.m;
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        if !(det.is_finite() && det.abs() > 1e-300) {
            return None;
        }
        let k = 1.0 / det;
        let mut r = [[0.0; 4]; 4];
        r[0][0] = (a[1][1] * a[2][2] - a[1][2] * a[2][1]) * k;
        r[0][1] = (a[0][2] * a[2][1] - a[0][1] * a[2][2]) * k;
        r[0][2] = (a[0][1] * a[1][2] - a[0][2] * a[1][1]) * k;
        r[1][0] = (a[1][2] * a[2][0] - a[1][0] * a[2][2]) * k;
        r[1][1] = (a[0][0] * a[2][2] - a[0][2] * a[2][0]) * k;
        r[1][2] = (a[0][2] * a[1][0] - a[0][0] * a[1][2]) * k;
        r[2][0] = (a[1][0] * a[2][1] - a[1][1] * a[2][0]) * k;
        r[2][1] = (a[0][1] * a[2][0] - a[0][0] * a[2][1]) * k;
        r[2][2] = (a[0][0] * a[1][1] - a[0][1] * a[1][0]) * k;
        for row in r.iter_mut().take(3) {
            row[3] = -(row[0] * a[0][3] + row[1] * a[1][3] + row[2] * a[2][3]);
        }
        r[3][3] = 1.0;
        let out = Mat4 { m: r };
        out.m.iter().flatten().all(|v| v.is_finite()).then_some(out)
    }

    /// A look from the meshes: the triangles, and the visible edges as the solid's edges. The
    /// engine works out the exact look again from the body when it has the kernel.
    fn look(meshes: &[&PolyfaceMesh], inv: &Mat4) -> SolidLook {
        let mut l = SolidLook::default();
        for m in meshes {
            let base = u32::try_from(l.positions.len()).unwrap_or(u32::MAX);
            l.positions.extend(m.vertices.iter().map(|p| inv.apply(*p)));
            let n = m.vertices.len();
            for (fi, f) in m.faces.iter().enumerate() {
                let ix: Vec<usize> = f.iter().filter(|i| **i != 0).filter_map(|i| usize::try_from(i.unsigned_abs()).ok()).filter(|i| (1..=n).contains(i)).collect();
                if ix.len() < 3 {
                    continue;
                }
                let at = |i: usize| base.saturating_add(u32::try_from(i - 1).unwrap_or(0));
                for k in 1..ix.len() - 1 {
                    if let (Some(a), Some(b), Some(c)) = (ix.first(), ix.get(k), ix.get(k + 1)) {
                        l.triangles.push([at(*a), at(*b), at(*c)]);
                        l.tri_face.push(u32::try_from(fi).unwrap_or(u32::MAX));
                    }
                }
                let raw: Vec<i32> = f.iter().copied().filter(|i| *i != 0).collect();
                for k in 0..raw.len() {
                    let (Some(a), Some(b)) = (raw.get(k), raw.get((k + 1) % raw.len())) else { continue };
                    if *a < 0 {
                        continue;
                    }
                    let p = |i: i32| usize::try_from(i.unsigned_abs()).ok().and_then(|i| m.vertices.get(i.checked_sub(1)?)).map(|p| inv.apply(*p));
                    if let (Some(pa), Some(pb)) = (p(*a), p(*b)) {
                        l.wires.push(vec![pa, pb]);
                    }
                }
            }
        }
        l
    }

    fn recover_store(st: &mut EntityStore, f: &mut Foreign) {
        let order: Vec<Handle> = st.iter().map(|e| e.handle).collect();
        let mut gone: Vec<Handle> = Vec::new();
        for (i, h) in order.iter().enumerate() {
            if gone.contains(h) {
                continue;
            }
            let Some(e) = st.get(*h) else { continue };
            let EntityKind::PolyfaceMesh(first) = &e.kind else { continue };
            let Some((dh, xh, rec)) = find(f, *h) else { continue };
            let Some(inv) = inverse(&rec.placement) else { continue };
            // The other meshes of the solid follow it.
            let mut parts: Vec<Handle> = Vec::new();
            for next in order.iter().skip(i + 1).take(rec.parts.saturating_sub(1)) {
                if matches!(st.get(*next).map(|e| &e.kind), Some(EntityKind::PolyfaceMesh(_))) {
                    parts.push(*next);
                }
            }
            let mut meshes: Vec<&PolyfaceMesh> = vec![first];
            for p in &parts {
                if let Some(EntityKind::PolyfaceMesh(m)) = st.get(*p).map(|e| &e.kind) {
                    meshes.push(m);
                }
            }
            let look = look(&meshes, &inv);
            let so = Solid3d { body: Arc::new(rec.body), placement: rec.placement, look: Arc::new(look), faces: rec.faces, edges: rec.edges, volume: rec.volume };
            if !so.placement_ok() {
                continue;
            }
            let _ = st.modify(*h, |e| e.kind = EntityKind::Solid3d(so));
            for p in parts {
                st.remove(p);
                gone.push(p);
            }
            // The record is the solid now: drop it, and the dictionary when nothing else is in it.
            f.objects.retain(|o| o.handle != xh);
            let mut empty = false;
            if let Some(d) = f.objects.iter_mut().find(|o| o.handle == dh) {
                let mut tags = Vec::with_capacity(d.tags.len());
                let mut it = d.tags.iter().peekable();
                while let Some(t) = it.next() {
                    if t.code == 3 && t.value.trim().eq_ignore_ascii_case(KEY) && it.peek().is_some_and(|n| matches!(n.code, 350 | 360)) {
                        it.next();
                        continue;
                    }
                    tags.push(t.clone());
                }
                empty = !tags.iter().any(|t| t.code == 3);
                d.tags = tags;
            }
            if empty {
                f.objects.retain(|o| o.handle != dh);
                if let Some(x) = f.entities.get_mut(h) {
                    x.xdict = None;
                    if x.is_empty() {
                        f.entities.remove(h);
                    }
                }
            }
        }
    }

    /// Turn the meshes DraftCraft saved for its solids back into solids, everywhere in the drawing.
    pub(crate) fn recover(d: &mut Drawing) {
        if d.foreign.entities.is_empty() {
            return;
        }
        let mut f = (*d.foreign).clone();
        recover_store(&mut d.model, &mut f);
        for l in &mut d.layouts {
            recover_store(&mut l.entities, &mut f);
        }
        for b in d.blocks.values_mut() {
            if b.entities.iter().any(|e| matches!(e.kind, EntityKind::PolyfaceMesh(_)) && f.entities.contains_key(&e.handle)) {
                recover_store(&mut Arc::make_mut(b).entities, &mut f);
            }
        }
        d.foreign = Arc::new(f);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use draftcraft_doc::{Drawing, EntityKind, Solid3d, SolidLook};
    use draftcraft_dxf::Version;
    use draftcraft_geom::{Mat4, Vec3};

    /// A tetrahedron with a made-up body (the file form doesn't look inside it), placed far from
    /// the origin.
    fn tetra(body_len: usize) -> Solid3d {
        let positions = vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
        let triangles = vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];
        let wires = vec![vec![positions[0], positions[1]]];
        let look = SolidLook { wires, positions, triangles, tri_face: vec![0, 1, 2, 3] };
        let mut placement = Mat4::IDENTITY;
        placement.m[0][3] = 1.0e6;
        placement.m[1][3] = -2.5;
        placement.m[2][3] = 7.0;
        let body: Vec<u8> = (0..body_len).map(|i| (i * 31 % 256) as u8).collect();
        Solid3d { body: Arc::new(body), placement, look: Arc::new(look), faces: 4, edges: 6, volume: 1.0 / 6.0 }
    }

    fn drawing_with(so: Solid3d) -> Drawing {
        let mut d = Drawing::new_metric();
        d.add(&draftcraft_doc::Space::Model, draftcraft_doc::Common::default(), EntityKind::Solid3d(so)).unwrap();
        d
    }

    fn only_kind(d: &Drawing) -> Vec<EntityKind> {
        d.model.iter().map(|e| e.kind.clone()).collect()
    }

    #[test]
    fn the_mesh_hides_edges_inside_a_face() {
        let mut so = tetra(10);
        // Two triangles of one face: their shared edge is hidden.
        Arc::make_mut(&mut so.look).tri_face = vec![0, 0, 2, 3];
        let m = &super::meshes(&so)[0];
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(m.faces.len(), 4);
        let hidden = m.faces.iter().flatten().filter(|i| **i < 0).count();
        assert_eq!(hidden, 2, "{:?}", m.faces);
        assert!(m.vertices.iter().any(|p| (p.x - 1.0e6).abs() < 1e-9 && (p.z - 7.0).abs() < 1e-9));
    }

    #[cfg(feature = "solids")]
    #[test]
    fn a_solid_comes_back_from_dxf_and_dwg() {
        let so = tetra(5000);
        let d = drawing_with(so.clone());
        let dxf = crate::write(&d, "solid.dxf").unwrap();
        let dwg = crate::write_dwg_own(&d, Version::R2018).unwrap().0;
        for (bytes, name) in [(dxf, "solid.dxf"), (dwg, "solid.dwg")] {
            let back = crate::read(&bytes, name).unwrap();
            let kinds = only_kind(&back);
            assert_eq!(kinds.len(), 1, "{name}: {kinds:?}");
            let EntityKind::Solid3d(b) = &kinds[0] else { panic!("{name}: {:?}", kinds[0]) };
            assert_eq!(b.body, so.body, "{name}");
            assert_eq!(b.placement, so.placement, "{name}");
            assert_eq!((b.faces, b.edges), (4, 6));
            assert_eq!(b.look.triangles.len(), 4);
            // The record is the solid now, not foreign data written twice.
            assert!(back.foreign.objects.iter().all(|o| !o.kind.eq_ignore_ascii_case("XRECORD")), "{name}");
            // And saving again keeps it.
            let again = crate::read(&crate::write(&back, "again.dxf").unwrap(), "again.dxf").unwrap();
            assert!(matches!(&only_kind(&again)[0], EntityKind::Solid3d(x) if x.body == so.body));
        }
    }

    #[cfg(feature = "solids")]
    #[test]
    fn a_large_solid_is_several_meshes_and_one_solid() {
        let mut so = tetra(100);
        // 40 000 separate triangles: more corners than one polyface holds.
        let mut look = SolidLook::default();
        for i in 0..40_000u32 {
            let x = f64::from(i);
            look.positions.extend([Vec3::new(x, 0.0, 0.0), Vec3::new(x + 0.5, 1.0, 0.0), Vec3::new(x, 0.0, 1.0)]);
            look.triangles.push([3 * i, 3 * i + 1, 3 * i + 2]);
            look.tri_face.push(i);
        }
        so.look = Arc::new(look);
        assert!(super::meshes(&so).len() >= 2);
        let back = crate::read(&crate::write(&drawing_with(so.clone()), "big.dxf").unwrap(), "big.dxf").unwrap();
        let kinds = only_kind(&back);
        assert_eq!(kinds.len(), 1);
        assert!(matches!(&kinds[0], EntityKind::Solid3d(x) if x.body == so.body && x.look.triangles.len() == 40_000));
    }

    #[cfg(not(feature = "solids"))]
    #[test]
    fn without_solids_the_mesh_keeps_its_record() {
        let so = tetra(300);
        let bytes = crate::write(&drawing_with(so), "solid.dxf").unwrap();
        let back = crate::read(&bytes, "solid.dxf").unwrap();
        assert!(matches!(&only_kind(&back)[0], EntityKind::PolyfaceMesh(_)));
        // Saved again, the record is still there for a DraftCraft with solids.
        let text = String::from_utf8_lossy(&crate::write(&back, "again.dxf").unwrap()).to_string();
        assert!(text.contains(super::KEY) && text.contains(super::MARK));
    }

    #[cfg(feature = "solids")]
    #[test]
    fn a_damaged_record_leaves_a_mesh_and_no_crash() {
        let text = String::from_utf8(crate::write(&drawing_with(tetra(300)), "solid.dxf").unwrap()).unwrap().replace("\r\n", "\n");
        let mark = super::MARK;
        for (i, bad) in [
            text.replacen("\n 93\n300\n", "\n 93\n999999999999\n", 1),
            text.replacen("\n 93\n300\n", "\n 93\n299\n", 1),
            // A placement group missing.
            text.replacen("\n 40\n1000000.0\n", "\n", 1),
            text.replacen("\n310\n", "\n310\nZZ", 1),
            text.replacen(mark, "Something else", 1),
        ]
        .iter()
        .enumerate()
        {
            assert_ne!(*bad, text, "case {i}");
            let back = crate::read(bad.as_bytes(), "bad.dxf").unwrap();
            assert!(matches!(&only_kind(&back)[0], EntityKind::PolyfaceMesh(_)), "case {i}");
        }
    }
}
