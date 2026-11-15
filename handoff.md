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
  - DraftCraft solids save as real 3DSOLIDs: SAT 700 for 2000–2010, SAB in the ACDSDATA data store for 2013+.
  - Code: `crates/solid/src/acis/` (`read.rs`, `write.rs`, `model.rs`) and `crates/io/src/solid_acis.rs`.
  - Test drawings: `crates/io/tests/data/acad-solids/`.

**AutoCAD check of the files DraftCraft writes** (full notes in `side-session-notes/…/dc-acis/autocad-check.md`). AutoCAD 2027 showed three problems; the fixes are in commit `c3629868`:
1. **Fixed:** the 3DSOLID in DXF was missing the `100 AcDbEntity` group, so AutoCAD discarded the whole DXF. A test copy with the group added opens with 0 AUDIT errors.
2. **Fixed:** the cone's face was missing the apex loop (one point edge with no curve), so AutoCAD called it an invalid solid with volume 0. The fix is not yet re-checked in AutoCAD.
3. **Still open:** AutoCAD rejects the SAT text in DWG 2000/2010 (`Error Code Number is 75003`, `eCannotRestoreFromAcisFile`, solids discarded). Lines now end with LF alone, as in AutoCAD's files, but that didn't fix it. Next suspect: the SAT version. AutoCAD writes 400 (2000) and 21500 (2010) in DWG, and we write 700. Try writing 400/21500 for DWG ≤ 2010, then compare the object bits after the SAT with an AutoCAD file.

DWG 2013 and 2018 open cleanly with 0 AUDIT errors, and 7 of the 8 solids match MASSPROP exactly (the 8th is the cone, fixed in 2).

Still to do: re-check every version in AutoCAD with the current files (the README of expected values is in `dc-acis/README.md`), then update the "Checked against AutoCAD" line in `docs/solids.md`.

Side note: every DXF draws the warning "unknown header variable `$CTABLESTYLE`". It's harmless, but it should be fixed.

---

## 1. File compatibility (top priority)

### 1.1 Free-form (NURBS) solids, reading
- **Symptom:** solids made with LOFT, SWEEP and blends on curved surfaces don't open as solids. They stay as AutoCAD saved them, and DraftCraft writes them back intact, so nothing is lost. The note on open says why. Test: `autocad_free_form_solids_never_crash_and_say_what_is_left` (`solids-free-2018.dwg`).
- **Cause:** solidcore can't make pcurves of a NURBS edge on a spline surface ("no exact pcurve of a NURBS on a spline surface"), and the solidcore STEP reader ignores the file's PCURVEs.
- **Fix:** pass ACIS's own pcurves to the kernel in `impl Trims for Kernel` (`crates/solid/src/acis/read.rs`).
  - In cadmpeg's IR, each coedge has `pcurves: [{parameter_range, pcurve}]`.
  - The geometry of each pcurve is in `v["pcurves"]`, as `{kind:"nurbs", degree, knots, poles:{form, points:[{u,v}]}}`.
  - For a `reversed` coedge the parameter is negated (range `[-0, -40]` against an edge on `[0, 40]`): flip the poles, and negate and flip the knots, so the pcurve runs in the edge's direction on its interval (`EdgeUse.given`).
  - You need to match the `TrimFace` passed by solidcore to the ACIS face (for example by the face's vertex positions and each edge's endpoints).
  - Then check the result by lifting the pcurve onto the surface.
- **Helix edges** (SWEEP along HELIX): in the IR the curve is `kind:"procedural"`, with the definition in `v["procedural_curves"]` (`kind:"helix"`: center, axis, major, minor, pitch, angle_range) and no cache. Build an interpolating B-spline of it, dense enough to stay within tolerance.
- NURBS surfaces and curves are already mapped from the real IR (`nurbs_surface`, `nurbs_curve`). The `u_periodic` ones have clamped knots and are read as ordinary B-splines.

### 1.2 Free-form (NURBS) solids, writing (W3)
- `crates/solid/src/acis/write.rs` refuses B-spline curves and surfaces. A DraftCraft solid with such faces is saved in the interim form (polyface mesh plus the exact body in an XRECORD), and AutoCAD sees a mesh.
- To do:
  - `spline-surface` with `exact_spl_sur` (the bs3 surface inline);
  - `intcurve` with `exact_int_cur`;
  - the coedges' `pcurve` records (`exp_par_cur`).
- Layouts: Spatial's published ones, checked by reading them back through cadmpeg-asm (as the current tests do).

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

1. Fix the SAT in DWG 2000/2010 (error 75003), then re-check every version in AutoCAD, the cone included.
2. NURBS solids, reading (1.1), then writing (1.2).
3. REGION and BODY (1.3), DWG 2004–2010 with SAB (1.4).
4. 3D UCS, 3D snaps, selecting parts of a solid, grips (3).
5. PRESSPULL, SWEEP, LOFT, SOLIDEDIT on faces (2).
6. STEP import/export (1.5).
7. Everything else: surfaces, MESH, documentation views, materials and render, the web.
