# Handoff: 3D solids in DraftCraft, what's still missing compared with AutoCAD

Written 2026-11-15. Branch `feat/solids-3d`, PR https://github.com/SkryxDev/DraftCraft/pull/20 (open, not to be merged without the owner's go-ahead). Everything is behind the cargo feature `solids`. The kernel is solidcore (`tangency-labs/solidcore` @ `ce5c6e9`), ACIS reading goes through `cadmpeg-asm` 0.6.12. Technical detail is in `docs/solids.md`, which must be kept up to date.

---

## 0. Where we are

**Done:**
- Primitives: BOX, WEDGE, CYLINDER, CONE, SPHERE, TORUS.
- EXTRUDE and REVOLVE.
- UNION, SUBTRACT, INTERSECT.
- SLICE, FILLETEDGE, CHAMFEREDGE, SOLIDEDIT shell.
- INTERFERE, MASSPROP, FLATSHOT, SECTION.
- 3D view: VPOINT, 3DORBIT, PLAN, VSCURRENT (wireframe, hidden, shaded).
- *3D Modeling* ribbon tab.
- **ACIS translator:**
  - AutoCAD 3DSOLIDs (SAT and SAB) open as editable solids. Tested against drawings made in AutoCAD 2027: volume and centroid match MASSPROP to four decimals.
  - DraftCraft solids save as real 3DSOLIDs, NURBS faces included: SAT 400 for 2000–2010, SAB 700 in the ACDSDATA data store for 2013+.
  - Code: `crates/solid/src/acis/` (`read.rs`, `write.rs`, `model.rs`) and `crates/io/src/solid_acis.rs`.
  - Test drawings: `crates/io/tests/data/acad-solids/`.

**AutoCAD check of the files DraftCraft writes** (full notes in `side-session-notes/…/dc-acis/autocad-check.md`). AutoCAD 2027 showed three problems; the fixes are in commit `c3629868`:
1. **Fixed:** the 3DSOLID in DXF was missing the `100 AcDbEntity` group, so AutoCAD discarded the whole DXF. A test copy with the group added opens with 0 AUDIT errors.
2. **Fixed:** the cone's face was missing the apex loop (one point edge with no curve), so AutoCAD called it an invalid solid with volume 0. The fix is not yet re-checked in AutoCAD.
3. **Fixed, not yet checked in AutoCAD:** AutoCAD rejected the SAT text in DWG 2000/2010 (`Error Code Number is 75003`, `eCannotRestoreFromAcisFile`, solids discarded), though the same text opens from DXF. Ending lines with LF alone didn't fix it. Comparing with AutoCAD's own DWG 2000, the only real difference is the SAT version: AutoCAD writes 400, we wrote 700. The container is the same, apart from AutoCAD writing a reference point and isolines in the wireframe block. SAT is now written as save format 400, in DXF too. The changes are no history fields after an entity's attribute, no parameters or convexity on an edge, and a header that counts the records. Test: `sat_text_looks_like_acis_400`, and a check in `solids_save_as_autocad_3dsolids_in_every_version`.
   - **Check files** in `side-session-notes/…/dc-acis/round3/`:
     - `x-acad400-*.dwg` and `x-acad21500-2010.dwg`: AutoCAD's own SAT through our DWG writer. They tell whether the container is right.
     - `x-dc700-2004/2007.dwg`: the old 700 SAT.
     - `sat400-*`: the new output.
   - **If the 400 files are still refused:** the container is the suspect. Write the reference point (the body's translation), isolines 4 and the wires flag as AutoCAD does.

DWG 2013 and 2018 open cleanly with 0 AUDIT errors, and 7 of the 8 solids match MASSPROP exactly (the 8th is the cone, fixed in 2).

Still to do: re-check every version in AutoCAD with the current files (the README of expected values is in `dc-acis/README.md`), then update the "Checked against AutoCAD" line in `docs/solids.md`.

Side note: every DXF draws the warning "unknown header variable `$CTABLESTYLE`". It's harmless, but it should be fixed.

---

## 1. File compatibility (top priority)

### 1.1 Free-form (NURBS) solids, reading — done, not yet checked in AutoCAD
- AutoCAD's LOFT and SWEEP-along-HELIX solids open as solids from DXF 2000 (SAT 400), DXF 2010 (ASM text 21500) and DWG 2018 (SAB in the data store), volume and centroid as MASSPROP. Test: `autocad_free_form_solids_read`.
- How (`crates/solid/src/acis/read.rs`):
  - **ACIS pcurves.** `impl Trims for Kernel` hands the kernel ACIS's own pcurves on spline faces (`Kernel::given`). The data's face is matched to the kernel's `TrimFace` by each use's edge ends and direction, trying the loop both ways.
    - A reversed coedge's pcurve is turned round (`pcurve`).
    - `uv_offset` is applied.
    - Periodic pcurves (clamped knots) are accepted.
  - **Parameter mapping.** Where the kernel's edge parameter differs from ACIS's (record circles in quarter turns, lines on [0, 1]), the pcurve is resampled onto it (`remapped`: a Hermite cubic through 32 spans). The ACIS parameter of each point comes from the edge's own curve (`AcisParam`) and its range (`EdgeParam`).
  - **Helix edges.** A procedural helix becomes a cubic spline of 64 Bézier spans per turn (`helix_spline`), at most 2^16 spans.
  - **ACIS 4 short names.** cadmpeg matches a coedge's pcurve only as `exp_par_cur`; SAT 400 writes `exppc`. `long_names` renames it in the parsed records before decoding.
  - **Edges without parameters.** SAT 400 keeps no parameter range on an edge: a circle edge's range comes from its ends' angles (`edge_param_of`).
  - **What cadmpeg only reads word by word** (`read/legacy.rs`, filled into the decoded B-rep):
    - SAT 400's `parcur` (AutoCAD 2000's SWEEP rail): its B-spline cache becomes the edge's curve, and the two pcurves that point into it (`pcurve $-1 ±1 $curve du dv`) its coedges' pcurves;
    - ASM text's procedural surfaces (`loft_spl_sur`, `sweep_sur` in DXF 2004–2010): the `full nubs …` cache at the subtype's own depth, `{ ref k }` resolved to the k-th subtype written;
    - ASM text's `helix_int_cur`: the same 64-spans-per-turn spline as the binary form.
  - **Closed circle edges.** The circle's reference passes through the vertex, nudged a few roundings until solidcore's STEP reader puts the vertex exactly at angle 0 (`closed_circle_placement`). Otherwise the kernel starts the turn a whole turn later and refuses the form.
- **Still open:**
  - a helix with `apex_factor ≠ 0` (a conical helix) is refused;
  - a pcurve with `wrapper_reversed` is refused;
  - other procedural curves (offset, intersection) have no definition yet.

### 1.2 Free-form (NURBS) solids, writing — done, not yet checked in AutoCAD
- `crates/solid/src/acis/write.rs` writes B-spline faces and edges:
  - `spline-surface { exactsur nubs|nurbs … }`, the poles with u running fastest, the three discontinuity lists per direction, a periodic direction's seam knot first (`spline_surface`, `periodic`);
  - `intcurve-curve { exactcur … }` for a B-spline edge (`exact_curve`);
  - a `pcurve { exppc … spline forward { ref k } … }` on every coedge of a spline face, on the coedge's parameter (a reversed coedge's knots negated), `ref k` counting the subtypes written (`Bs2::record`, `layout`). A face on a spline surface keeps its seams.
- **SAT** (DWG/DXF 2000–2010) is laid out as AutoCAD 2000 writes its own LOFT and SWEEP (`acad-ref/solids-free-2000.dxf`).
- **SAB** (2013+) stays save format 700, as the analytic solids AutoCAD already opened. The subtypes are named in full (`exact_spl_sur`, `exact_int_cur`, `exp_par_cur`), with the later formats' fields (`Tok::Sab`): the cache form `full` before the spline; after a surface's discontinuity lists one false (as AutoCAD 2018's data store ends a spline surface's cache) instead of SAT 400's `F 1 F 0 F 1 F 0`; after a curve's lists no extension and two unbounded ends. **This tail is the least certain part:** if AutoCAD refuses a NURBS solid from DWG 2013/2018 while it opens from DWG 2010, look here first.
- Test: `autocad_free_form_solids_write_back` (AutoCAD's loft and sweep written as SAT and SAB and read again, the same volume).
- **To check in AutoCAD:** save `solids-free-2018.dwg` from DraftCraft as DWG/DXF 2000, 2010, 2013, 2018; open each; MASSPROP against `acad-ref/mp-free-*.mpr`; AUDIT.

### 1.3 Other 3D entities from AutoCAD
- **REGION** (2D, a single planar face) and **BODY**: today they stay as foreign data. Read them as regions and solids; write them as REGION and BODY.
- **Surfaces:** PLANESURF, NURBSURF, EXTRUDEDSURFACE, LOFTEDSURFACE, SWEPTSURFACE, REVOLVEDSURFACE. These need a "surface" entity in the document.
- **MESH** (subdivision mesh, `MESH` entity): read, draw and write it.
- **A 3DSOLID with several separate lumps:** today it is refused and kept as saved ("several separate solids"). Read it as one solid with several lumps, if solidcore allows it, or split it.
- **Mirrored ACIS transforms** (det < 0): refused today. Handle them by reversing face senses.

### 1.4 What gets lost on a round trip
- **Solid history** (`ACSH_*` objects, `AcDbShHistory`, SOLIDHIST): in AutoCAD you select a BOX and change its dimensions from Properties. We drop the history (`350 0`). Either keep it as foreign data linked to the solid, or rebuild it (at least for the primitives).
- **Face colours and materials:** ACIS attributes (`persubent-acadSolidHistory-attrib`, colour attributes) and MATERIAL objects aren't carried into DraftCraft's body.
- **DWG 2004–2010 with SAB data** (modeler format 2): `crates/io/src/dwg/entities.rs::modeler()` returns `None` (not read). Read it: SAB right after the header, up to the end marker.

### 1.5 Exchange with other programs
- IMPORT and EXPORT for STEP, through `solidcore-step`, which already reads and writes. Also IGES.
- STLOUT and 3DPRINT (from the display mesh).
- 3DDWF can wait.

---

## 2. Missing modelling commands

Each one is a `CommandSpec` in `crates/engine/src/cmd/solids/*`, with tests, a ribbon place on the panel where AutoCAD on Windows puts it, an icon drawn in `icons.rs`, Italian strings in `it.tsv` and the menu entry in `zh.tsv`.

- **PRESSPULL:** pull a face or a closed area.
- **SWEEP, LOFT** (sections, guides, path), **THICKEN**.
- **POLYSOLID, PYRAMID, HELIX.**
- **SOLIDEDIT, complete.** Only shell exists today. Missing:
  - Faces: extrude, move, offset, taper, rotate, delete, copy, colour.
  - Edges: copy, colour.
  - Body: imprint, separate, clean, check.
- **OFFSETEDGE, XEDGES, IMPRINT.**
- **CONVTOSOLID, CONVTOSURFACE, CONVTOMESH.**
- **Surface commands:** SURFBLEND, SURFPATCH, SURFNETWORK, SURFOFFSET, SURFFILLET, SURFTRIM, SURFUNTRIM, SURFEXTEND, SURFSCULPT.
- **3D transforms:**
  - 3DMOVE, 3DROTATE, 3DSCALE with the gizmos;
  - MIRROR3D, ROTATE3D, 3DARRAY, ALIGN/3DALIGN.
  - The placement matrix already makes the maths easy; the interaction is what's missing.
- **SECTIONPLANE** (live section, with the section object), **LIVESECTION**, **GENERATESECTION**.
- **Documentation:** SOLVIEW, SOLDRAW, SOLPROF, and VIEWBASE/VIEWPROJ/VIEWSECTION/VIEWDETAIL (2D views from the model in a layout, updated when it changes).

---

## 2b. Review of PR 20 (WithShalom, 14 points)

All fourteen are fixed, each with a regression test:
1–2. Interim file form: each mesh's fingerprint in the record; only unchanged meshes come back, parts found by fingerprint, never by position (`crates/io/src/solid_file.rs`).
3. CI: a second job, `gate-solids`, runs `cargo xtask ci --solids`.
4. Mirrored solids: triangles wound back in the 3D view and in the meshes written to files.
5. SLICE/SECTION: a typed `x,y,z` keeps its Z (`Session::point_z`); a side point on the plane is refused.
6. FILLETEDGE/CHAMFEREDGE/Shell pick along the line of sight of the current view (`sight`, `pick_edge`, `pick_face`).
7. Solids inside blocks are drawn in 3D.
8. Multi-object commands are all or nothing (`Session::all_or_nothing`).
9. A saved workspace named "3D Modeling" is no longer shadowed by the built-in one.
10. INTERFERE: box prefilter, bodies read once, no look unless the solid is created.
11. Opening no longer re-tessellates every solid: only those recovered from their meshes are drawn from their bodies.
12. The 3D view keeps what it painted until something changes; off-screen faces are left out of the hidden-line grid.
13. CHANGELOG lines for the workspace menus.
14. `draftcraft-web` has no `solids` feature; the workspace menus show 3D Modeling greyed out where solids are off.

---

## 3. Working in 3D (interaction)

- **3D UCS:** UCS Face, Object, 3point, ZAxis, rotation about X, Y and Z. The **dynamic UCS** (DUCS) follows the face under the cursor while a command runs.
- **3D object snaps:** vertex, edge midpoint, face centre, knot, perpendicular to a face, nearest on a face (3DOSNAP).
- **Selecting parts of a solid:** faces, edges and vertices with Ctrl+click (sub-object selection), and selection filters.
- **Grips on solids:** primitives resized by dragging grips (this needs the history in 1.4); grips on faces and edges.
- **Properties palette for solids:** type, history, volume, dimensions of the primitive.
- **Navigation:**
  - ViewCube, the navigation bar, continuous and free 3DORBIT, 3DZOOM/3DPAN;
  - perspective view, cameras (CAMERA), walk/fly.
- **Several 3D viewports**, each with its own view and visual style (the rest of S4).
- **The 10,000-face test** from the advisory (performance), and a GPU path for drawing.

---

## 4. Visualization and output

- **Visual styles:** today there are three (wireframe, hidden, shaded). Missing: Realistic, Conceptual with edges as in AutoCAD, Shades of Gray, Sketchy, X-ray, and the visual style manager.
- **Materials** (library, applied to objects, faces and layers), **lights**, **sun**, **shadows**, **RENDER**.
- **Plotting 3D viewports from a layout** with hidden lines or shading (shade plot: "As displayed", "Hidden", "Rendered").

---

## 5. Platform

- **Solids in the browser:** the web build keeps `solids` off. On wasm a kernel panic aborts the app, because `catch_unwind` doesn't work there (open question 1 in `docs/solids.md`). Solidcore needs a panic-free mode, or another solution.

---

## 6. Rules to follow (from AGENTS.md)

- **Never crash:**
  - no `unwrap`, `expect`, `panic!` or direct indexing on data coming from files;
  - checked arithmetic, capped loops and allocations;
  - every crash fix lands with a regression test.
- **Clean-room:**
  - AutoCAD is only observed from the outside, as a black box: never read or copy anything inside its installation, and never commit screenshots.
  - DWG and DXF files are allowed as test drawings, each with its row in `ATTRIBUTION.md`.
  - No GPL code: no LibreDWG, OpenCASCADE, FreeCAD and so on, not even read.
- **AutoCAD checks** always run in a separate side session using computer use.
- **Layers:** `draftcraft-solid` is the only crate that names `solidcore*` and `cadmpeg*` (`cargo xtask layers`).
- **Before every commit:** `cargo xtask ci` and `cargo xtask ci --solids`. Only three failures are accepted, and only on macOS: `non_unicode_output_names`, `read_file_refuses_a_name_that_is_not_unicode`, `thai_marks_stack_in_an_installed_font`.
- **Commits** end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Recommended order

1. Check in AutoCAD the `round3` files: SAT 400 in DWG 2000/2010 and the cone. Then re-check every version.
2. Check NURBS solids in AutoCAD (1.1 and 1.2 are written).
3. REGION and BODY (1.3), DWG 2004–2010 with SAB (1.4).
4. 3D UCS, 3D snaps, selecting parts of a solid, grips (3).
5. PRESSPULL, SWEEP, LOFT, SOLIDEDIT on faces (2).
6. STEP import/export (1.5).
7. Everything else: surfaces, MESH, documentation views, materials and render, the web.
