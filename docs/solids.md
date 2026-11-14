# 3D solids with solidcore

This page has two parts:
- **Status**: what branch `feat/solids-3d` built, how to try it, measurements, decisions and open questions. It is kept current.
- **The advisory** that planned the work (written 2026-11-09), unchanged except for its status line. It planned the work on SolidCraft's kernel; the kernel is now solidcore (see Decisions), and the plan's stages, file form and ACIS work stand as written.

## Status

Updated 2026-11-14. Stages S0, S2, S3 and S6 are done, and S4 is done in part. S1, S5 and S7 are not started. Everything is behind the cargo feature `solids`. Without the feature, the build, the tests and the wasm check are exactly what they were.

The kernel is [solidcore](https://github.com/tangency-labs/solidcore) (Apache-2.0): a pure-Rust B-rep modeller, a port of NVIDIA's SMLib, with no `unsafe`, building for wasm32. It replaced SolidCraft's truck-based kernel on 2026-11-14, at the owner's request.

### Trying it

```bash
cargo run --release -p draftcraft --features solids -- --sample
```

The feature chain:
- `draftcraft-engine/solids` turns on `draftcraft-solid/kernel`.
- `draftcraft-io/solids` reopens saved solids.
- `draftcraft-ui-egui/solids` adds the 3D view and the ribbon tab.
- `draftcraft-mcp/solids`.
- The apps `draftcraft`, `draftcraft-cli` and `draftcraft-web` each have a `solids` feature that turns on the ones they use.

`cargo xtask ci --solids` runs the clippy and test steps with the feature on (see Measurements for the cost).

### Where it sits

| Crate | Layer | What it has |
|---|---|---|
| `draftcraft-solid` (new) | L0 | The only crate that names the kernel (`cargo xtask layers` enforces it). It holds the body type, primitives, extrude and revolve, booleans, slice, fillet, chamfer, shell, mass properties, the display look, exact hidden-line projection and sections. It converts between `draftcraft-geom` and the kernel's vectors here and nowhere else. It uses `solidcore-ops` (the modelling API), `solidcore-record` (profiles, planes, the portable B-rep record) and `solidcore-tess` (display meshes). Without its `kernel` feature it is empty. |
| `draftcraft-doc` | L1 | `EntityKind::Solid3d`, which holds: the exact body in solidcore's binary `.smb` format behind DraftCraft's 8-byte header `DCSMB001`; a placement matrix; the look (edges and a triangle mesh in the body's own coordinates); and the face count, edge count and volume. The document never calls the kernel. MOVE, ROTATE, SCALE and MIRROR change only the placement. 2D drawing, snapping and selection read the edges projected to XY. |
| `draftcraft-io` | L3 | The DXF and DWG form (below). |
| `draftcraft-engine` | L4 | The commands (`cmd/solids/*`) and the 3D view state (`view3d.rs`). |
| `draftcraft-ui-egui` | L5 | The *3D Modeling* ribbon tab, its icons (drawn in `icons.rs`), and the 3D drawing area (`canvas/view3d.rs`). |

Bodies are kept near their own origin. A solid is made around its base point and placed by its matrix. Booleans and other edits work in a frame at the first solid's origin. This keeps kernel tolerances relative to the part even at survey coordinates.

### Commands

| Command | Notes |
|---|---|
| BOX, WEDGE, CYLINDER, CONE, SPHERE, TORUS | JSON and interactive prompts. |
| EXTRUDE, REVOLVE | Profiles can be circles, closed polylines (with bulges) or regions. The profiles are erased unless `keepProfiles` is set. EXTRUDE takes a taper angle. |
| UNION, SUBTRACT, INTERSECT | These are the existing region commands. When solids are selected, they use the kernel. |
| SLICE | Cuts by a plane given as origin and normal, two points, or XY/YZ/ZX. Keeps both sides or the side with a given point. |
| FILLETEDGE, CHAMFEREDGE | Edges are picked by a point near them. |
| SOLIDEDIT | Only the Shell operation; it can remove faces. Other operations report "not available yet". |
| INTERFERE | Reports whether solids interfere. It can create the interference solid. |
| MASSPROP | For solids: mass at density 1, volume, surface area, bounding box, centroid, moments and products of inertia about the origin, radii of gyration, and moments about the centroid. |
| FLATSHOT | Hidden-line view as lines, arcs and circles (exact where the projection is a line or an arc). Hidden lines can be shown in the HIDDEN linetype. |
| SECTION | The cut through solids as regions. |
| VPOINT, 3DORBIT, PLAN, VSCURRENT | The 3D view (S4, below). Without the feature, 3DORBIT still says "not available yet". |

All of them are `CommandSpec`s with tests (`crates/engine/src/cmd/solids/tests.rs`). They are reachable from the command line, scripts, the CLI, the control channel and MCP. They sit on the ribbon's *3D Modeling* tab, which has four panels: Modeling, Solid Editing, Section and View. Their labels are translated into Italian and Chinese.

### Saving (S3, the interim form)

In DXF and DWG, a solid is written as two things:

1. **A polyface mesh** (POLYLINE) that any program shows. The edges between triangles inside one face are invisible. A mesh past 32767 vertices or faces is split into several meshes.
2. **An extension dictionary on the first mesh**, with the entry `DRAFTCRAFT_SOLID` pointing to an XRECORD. The XRECORD groups are:

   | Group | Holds |
   |---|---|
   | 1 | `DraftCraft solid 1` |
   | 90 | Mesh count |
   | 91, 92 | Face count, edge count |
   | 140 | Volume |
   | 16 × 40 | Placement, row-major |
   | 93 | Body length |
   | 310 | The body, in chunks of 127 bytes |

On open, `draftcraft-io` with `solids` finds the marked meshes and turns them back into the solid. It drops the extra meshes, the record and the dictionary. The engine then redraws each reopened solid from its body.

- **Without the feature:** the meshes stay meshes and the record is kept as foreign data, so a save writes it back.
- **A damaged record:** the mesh is kept and nothing crashes (tested).
- **The save tells the user:** it says on the command line (and in `solidsAsMeshes`) that other programs see solids as meshes.
- **Open time:** reopening a file saved with DraftCraft solids decodes each body once. That covers up to 100,000 solids (`MAX_REFRESHED`) and skips any look past 200,000 triangles. Files with no DraftCraft solids never call the kernel.

S1 (native DWG items) was not needed for this. The DWG writer already writes POLYLINE, DICTIONARY and XRECORD with binary 310 groups. The direct DWG reader keeps the extension dictionaries and objects, and DWG 2013/2018 solids from other programs already keep their data-store geometry (`e284f21e`).

### Drawings from solids (S6)

- **FLATSHOT.** solidcore has no hidden-line module, so `draftcraft-solid` makes the view (`crates/solid/src/views.rs`):
  - Each B-rep edge is projected exactly where it can be: a straight edge always, and a line or arc that lies in a plane parallel to the paper, read exactly by the kernel (`prism::planar_edges`). Other curves (a circle seen at an angle) are drawn as short chords.
  - The silhouettes of curved faces are found on the display mesh: the mesh edges, inside one face, between a triangle facing the viewer and one facing away. They are marked inexact.
  - Each line is sampled finely and every sample tested against the triangles of all the solids' meshes in front of it. Runs of visible and hidden samples become pieces, and an exact edge keeps its exact form cut to each run.
  - A piece lying on top of a nearer line (a back edge behind a front edge) is left out, as a drafter would draw it. Cylinder seams are not drawn.
  - Faces the mesher could not mesh are counted and reported.
- **SECTION** slices the solid and outlines the cut face from its exact edges.
  - The output is regions.
  - A cut parallel to XY stays where it is; other cuts are laid flat at a base point.

### 3D view (S4, in part)

The engine keeps a 3D view per drawing: a direction, a framing and a visual style.
- **VPOINT** takes named views (top … nwiso) or a viewpoint x,y,z.
- **3DORBIT** turns the view by azimuth and elevation. Interactively, you drag with the left button; Shift plus the middle button orbits at any time.
- **PLAN** goes back to the plan view.
- **VSCURRENT** picks the style: 2D Wireframe (the plan view), Wireframe, Hidden, Realistic, Conceptual, Shaded, Shaded with edges, Shades of Gray, X-Ray.

These change the view, never the drawing, and are not undone.

The drawing area paints the 3D view on the CPU with egui's painter:
- **Projection:** orthographic.
- **What it draws:** 2D objects lie on the XY plane; solids are drawn from their looks.
- **Faces:** filled back to front.
- **Edges and silhouettes:** cut into short pieces, each tested against the faces in front of it on a screen grid. So the Hidden and shaded styles show only the edges a viewer sees, including the silhouettes of curved faces.
- **Navigation:** the wheel zooms about the cursor and the middle button pans.
- **Clicks:**
  - At a selection prompt, a click picks the front-most solid, or a 2D object within 5 points.
  - With no command running, a click toggles the selection.
  - At a point prompt, a click gives the point on the XY plane under the cursor. A view looking along that plane asks the user to type the point instead.

### The 3D Modeling workspace

The interface follows the structure of AutoCAD's 3D Modeling workspace on Windows. Only the structure is taken from it: the names of tabs, panels and commands and where they sit. Every icon is drawn in code (`crates/ui-egui/src/icons.rs`); no artwork, icons or help text come from AutoCAD.

- **Workspaces.** With `solids` on, there are two built-in workspaces, *2D Drafting* and *3D Modeling*. You pick one from the workspace switch in the Quick Access Toolbar, from the Workspace Switching button in the status bar, or with WSCURRENT. Neither can be renamed, deleted or overwritten (`crates/ui-egui/src/workspaces.rs`).
- **Ribbon in 3D Modeling** (`SOLID_TABS` in `crates/ui-egui/src/ribbon.rs`). Then come the shared tabs (Insert, Annotate, View, Manage, Output and so on).
  - **Home:** Modeling, Solid Editing, Draw, Modify, Section, View (visual style, preset views, orbit, plan) and Layers.
  - **Solid:** Primitive, Solid, Boolean, Solid Editing and Section.
  - **Visualize:** Views (the ten preset views and the plan view) and Visual Styles.
  
  In 2D Drafting the ribbon is the 2D one, as before.
- **Preset commands.** The ribbon and the viewport controls run one command per preset (`crates/engine/src/cmd/solids/look.rs`), so scripts, the command line and MCP reach them too.
  - `vpoint.top`, `vpoint.bottom`, `vpoint.left`, `vpoint.right`, `vpoint.front`, `vpoint.back`, `vpoint.swiso`, `vpoint.seiso`, `vpoint.neiso`, `vpoint.nwiso`.
  - `vscurrent.2dwireframe` … `vscurrent.xray`.
- **Viewport controls.** At the top left of the 3D view, `[-][SE Isometric][Realistic]` (`crates/ui-egui/src/canvas/view3d_overlay.rs`). Each bracket is a menu:
  - `[-]` shows or hides the ViewCube and goes to the plan view.
  - The view menu lists the ten presets, and shows *Custom View* when the view is between them.
  - The style menu lists the nine visual styles.
- **ViewCube.** At the top right, a cube drawn in code that turns with the view.
  - Its faces carry the labels TOP, FRONT, RIGHT and so on.
  - Each face is cut into nine spots, which gives 26 views (6 faces, 12 edges, 8 corners). A click on a spot runs VPOINT from there.
  - Dragging the cube orbits the view.
  - The compass ring (N, S, E, W) lies on the XY plane under the cube.
  - The Home button goes to SE Isometric.
  - The right-click menu has Home, Plan View and Hide ViewCube.
  - The cube follows the ViewCube setting (`ui.toggle.viewcube`), the same setting as in the plan view.
- **Navigation bar.** On the right, under the ViewCube: Zoom Extents, Zoom In, Zoom Out and Orbit. Panning is the middle button, as in the plan view.
- **Left out, for lack of commands.** These panels from AutoCAD are not there: Surface, Mesh, Coordinates, Model Viewports, Selection, Lights, Materials and Render. Pyramid, Sweep, Loft and Presspull are missing as well. The 3D view has no UCS of its own and belongs to model space, not to a viewport.

Not done yet:
- A view per tiled viewport and per layout viewport (the 3D view is model space's only).
- Object snaps and grips in the 3D view.
- Face and edge picking for FILLETEDGE, CHAMFEREDGE and SOLIDEDIT (they pick by XY position, so use them in the plan view).
- UCS on a face.
- A GPU path.
- The "smooth at 10,000 faces in the browser" acceptance, which has not been measured.

### S0 spike: measurements and the three checks

Measured on an Apple M4 (10 cores).

| Measure | Without `solids` | With `solids` |
|---|---|---|
| Cold build of the kernel chain (`draftcraft-solid` with `kernel`, which includes solidcore; dev profile with dependencies at opt-level 2) | — | 2 min 21 s (141 s wall, 889 s CPU) |
| Release binary `draftcraft` (macOS arm64) | 61.4 MB | 105.8 MB (+44.4 MB) |
| `draftcraft-web` wasm (release, `wasm32-unknown-unknown`, before wasm-opt) | 53.8 MB | 105.3 MB (+51.5 MB) |
| `cargo xtask ci` | as before | `cargo xtask ci --solids` adds clippy and the tests with the feature on, plus a wasm32 check of the kernel |

The three checks from the advisory's section 5:

1. **Units and scale.** `crates/solid/src/tests.rs::units_and_scale` runs the same part at three scales and far from the origin, and the answers agree relative to the part's size:
   - at 1/100 scale, within 1e-3;
   - at 1 and at 1000, within 1e-4;
   - at survey coordinates, within 1e-3.

   Each body is held near its own origin with a placement, as the advisory expected.
2. **Panics in the browser.** solidcore runs every call inside its session, which turns a panic into an error with `catch_unwind`. On `wasm32-unknown-unknown` a panic aborts, so that guard (and DraftCraft's own `guard.rs`) does nothing there. A kernel panic in the browser ends the app, where on desktop it would become an error. **So the web build keeps `solids` off** (open question 1). The kernel builds for wasm: see the wasm row above.
3. **Determinism.** `crates/solid/src/tests.rs::determinism` repeats the same operations and gets byte-identical bodies. Undo and the tests compare drawings, so this matters. `Body::encode` writes the form a decoded body writes, so a solid opened and saved again keeps its bytes (`native_round_trip_is_exact`).

### Decisions

- **The kernel: solidcore**, the crates `solidcore-ops`, `solidcore-record` and `solidcore-tess` as git dependencies pinned to `tangency-labs/solidcore` `ce5c6e9` (Apache-2.0). This is the owner's decision of 2026-11-14 and replaces SolidCraft's truck-based kernel (`withshalom/solidcraft` `d11d268`), which the branch used first. The facade's API did not change, so the document model, the file form, the engine and the UI are the same with either kernel.
  - What solidcore brings: primitives, extrude (with draft), revolve, exact booleans, fillets and chamfers (`blend_ordered`), shell, slicing, exact mass properties with the inertia tensor (so MASSPROP now reports moments), point classification, contact tests, a native `.smb` format and a display mesher.
  - What it doesn't have: a hidden-line module. FLATSHOT's projection is DraftCraft's own (see S6).
  - Its positive shell thickness grows the body outward; the facade passes it negated, so a shell's walls go inside as in AutoCAD.
- **The ACIS record layer** (owner's call 3), for when S5 and S7 start:
  - **cadmpeg-asm** (Apache-2.0, "Parse and write Autodesk Shape Manager record streams", v0.6.12) is the candidate. Its licence is compatible, so it can be a dependency.
  - **acadrust** (MPL-2.0) is out. DraftCraft no longer depends on it: the DWG writer replaced it. Bringing back an MPL dependency for one layer isn't worth it.
  - Writing our own `draftcraft-acis` stays the fallback if cadmpeg-asm doesn't cover ACIS 700 writing.
  - Nothing ACIS is in this branch. S5 and S7 were out of scope. solidcore's portable B-rep record (`solidcore-record`: faces, surfaces and edges with their kinds) is what an ACIS writer would read.
- **The interim DWG/DXF form** is mesh plus exact body (owner's call 4), as described under Saving. The body bytes are solidcore's `.smb`; the XRECORD layout is unchanged.

### Open questions

1. **Panics on wasm.** How does solidcore's own browser build survive a kernel panic when `catch_unwind` can't catch it? Is there a panic-free mode, or a plan for one? Until then the web build keeps `solids` off.
2. **API stability.** `tangency-labs/solidcore` is a read-only mirror of `tangency-labs/solidcraft/solidcore`. How often does its API change, and is there a release to pin instead of a commit?
3. **Shell limits.** Which bodies is SOLIDEDIT shell expected to handle (curved faces, several removed faces)? What should a user see when it can't?

### Next steps

1. S4 remainder: a view per viewport, snaps in 3D, face and edge picking, a GPU path, and measuring the 10,000-face acceptance.
2. S5, the ACIS writer: W1, then W2, then W3, with cadmpeg-asm as the record layer, reading solidcore's portable record.
3. PRESSPULL, SWEEP, LOFT, THICKEN; STEP export.
4. S7, the ACIS reader.

---

## The advisory

### Solid modelling in DraftCraft with SolidCraft's kernel: advisory

Written 2026-11-09. Status: **S0, S2, S3 and S6 done; S4 in part, on solidcore instead of SolidCraft's kernel (see Status above).** Checked against `withshalom/solidcraft` at `d11d268` (v0.27.0) and `withshalom/draftcraft-app` at `3d1cf5e` (v2.0.2). Read with `claude/PLAN-ONE-MODEL.md` and `claude/DWG-READER-ROADMAP.md`; this plan adds to them and changes neither.


#### 1. The question: does DWG fidelity come first?

No. Three separate layers are involved, and only one of them is new work.

| Layer | What it is | State today |
|---|---|---|
| A. DWG/DXF container | The 3DSOLID record, and where its geometry blob is kept in the file | Carried through untouched on the DXF and acadrust path. Planned for the native reader (roadmap phase 3, item 10). |
| B. ACIS content | The solid itself: faces, edges and surfaces written in ACIS SAT (text) or SAB (binary) inside that blob | **Not understood by anything in DraftCraft or SolidCraft.** This is the gap. |
| C. Kernel | Code that builds and edits solids | SolidCraft has it. It speaks its own format and STEP, not ACIS. |

So the accurate version of your two readings:

- **Layer A is fidelity work you already planned.** It makes DraftCraft *keep* solids that are in a file. It does not make them editable, and the kernel isn't needed for it.
- **Layer B is the new development.** A translator between ACIS data and the kernel's bodies. Without it, a solid made in DraftCraft can't be saved as a real DWG solid, and a solid from a file can't be edited.
- **The DWG code needs only a small extension for 3D** (section 7). The large piece, the ACIS translator, sits beside the DWG code, not inside it.
- **The kernel doesn't have to wait for either.** It can come in first, with solids saved in ways that don't need ACIS (section 8, stage S3).

One correction to what I said earlier: the ACIS writer doesn't strictly need the native DWG writer. Today's save path (model → DXF → acadrust) already writes the data-store records back, so a new solid's ACIS blob could travel that way too.

#### 2. What the repos show

##### SolidCraft

- **`crates/kernel`** (about 95k lines, 906 tests) is the whole solid-modelling boundary. It depends only on `solidcraft-geom` (about 26k lines) and the truck crates. No truck type appears in its public API.
- **Operations:** `extrude`, `revolve`, `sweep`, `loft`, `box_solid`, `cylinder`, `sphere`, `torus`, `boolean`, `fillet`, `chamfer`, `shell`, `draft`, `split_by_plane`, `thicken`, `transform`, `measure_mass`, plus surfaces (`extrude_sheet`, `stitch`, `trim`, `offset_surface`).
- **Formats:** STEP in and out, IGES in and out, and an exact native binary body format (`encode_native_body` / `decode_native_body`, magic `SCBREP04`).
- **No ACIS.** `docs/step-import.md` says `.sat`/`.sab` files are not read yet. cadmpeg has a `sat` codec that SolidCraft deliberately leaves switched off.
- **Drawing support in `geom`:** `hlr` (exact hidden-line removal), `section` and `drawing_view`. These work on the portable B-rep, not on truck types.
- **Patched truck.** `vendor/` holds truck-geometry, truck-shapeops, truck-stepio, truck-meshalgo and matext4cgmath with local fixes, wired in by `[patch.crates-io]`.
- **Safety:** every call into truck runs under `guard`, which turns a panic into an error. Rayon on desktop, serial on wasm. A browser app already ships.
- **Licence:** MIT OR Apache-2.0; truck is Apache-2.0. Same as DraftCraft.
- **Versions:** Rust 1.95, edition 2024 and wgpu 30 in both repos. egui is 0.36 in SolidCraft and 0.37 (a patched fork) in DraftCraft.

##### DraftCraft

- **No solids yet.** `ROADMAP.md` says there is no 3D modelling, and lists "the first 3D commands" as a next step.
- **Solids are opaque.** 3DSOLID, BODY and foreign REGION are kept as `EntityKind::Unknown`. Their geometry in 2013+ files lives in the data-store section (`ACDSDATA`), which `doc::foreign::data_store` keeps as read and writes back (`io/tests/it/acis_data_store.rs`).
- **The native DWG codec names the types only.** `crates/dwg/src/read/object.rs` names REGION and 3DSOLID. I found no code in `crates/dwg` that decodes their record or reads the data-store section, so today that section comes through the acadrust path.
- **Regions are DraftCraft's own.** `Region` is a set of 2D loops. UNION, SUBTRACT, INTERSECT and MASSPROP already exist for regions. On save, a region is written as closed polylines with `DRAFTCRAFT` xdata, because `dxf_region.rs` treats ACIS as undocumented. That is a useful precedent (stage S3).
- **Interaction is 2D.** Entities decompose into `Prim`s built on `Vec2`. Snapping, selection, trimming and rendering all read those. Some entities hold 3D points (`Polyline3d`, `Face3d`, `PolyfaceMesh`, `Helix`).
- **Rules that bear on this:** crate layering (`cargo xtask layers`: L0 `geom`, `dxf` → L1 `doc` → L2 `render` → L3 `io` → L4 `engine` → L5 `ui-egui`), no panics, `unsafe_code = "forbid"`, and from `PLAN-ONE-MODEL.md`: no phase may make open slower, and no new runtime dependency in the open/save path.

#### 3. Recommendation

1. **Use SolidCraft's `kernel` and `geom` crates, and nothing above them.** SolidCraft's `doc` and `engine` are a parametric feature timeline. Bringing them in would be a second model, against the one-model rule.
2. **Keep every solid's original ACIS bytes, and write them back unchanged unless the solid was edited.** Fidelity then never depends on how good the translator is.
3. **Decode lazily.** Opening a file never calls the kernel. A solid is translated only when it is edited or needs a display the file didn't supply.
4. **Build the ACIS translator in tiers, writer first.** Writing only has to cover what DraftCraft's own commands can produce. Reading has to cope with anything AutoCAD wrote.
5. **Ship the drafting payoff early.** Hidden-line and section views from solids, as real 2D entities, is where a drafting product gains most from a kernel.

Alternatives considered:

| Option | Why not |
|---|---|
| Use truck directly | Loses SolidCraft's fixes, guards, fillets and 906 tests. |
| OpenCascade | C++ and LGPL: breaks pure Rust, forbid-unsafe and the browser build. |
| Write a kernel in BlackCAD's `cad_core` | Years of work that already exists under a compatible licence. |

#### 4. Where it sits in DraftCraft

| Crate | Layer | Holds |
|---|---|---|
| `draftcraft-acis` (new) | L0 | ACIS record streams: SAT text and SAB binary, read and write. No kernel, no `doc`. The same role `draftcraft-dxf` has for DXF. |
| `draftcraft-solid` (new) | L0 | A thin facade over `solidcraft-kernel`: the body type, the operations DraftCraft uses, tessellation, edge extraction, and the mapping between ACIS records and bodies. Converts between `draftcraft-geom` and `solidcraft-geom` vectors here and nowhere else. |
| `draftcraft-doc` | L1 | A new `EntityKind::Solid3d` (the name `Solid` is taken by the 2D SOLID entity), and later `Surface`. |
| `draftcraft-render` | L2 | Edges and silhouettes as `Prim`s; later a shaded mesh. |
| `draftcraft-io` | L3 | DXF and DWG mapping for the 3DSOLID record and its data-store record. |
| `draftcraft-engine` | L4 | Commands. |
| `draftcraft-ui-egui` | L5 | View direction, orbit, visual styles. |

**Why a facade.** Only `draftcraft-solid` names the kernel. The kernel can then be pinned, patched or replaced without touching `doc` or the engine, and `xtask layers` needs one new entry.

**The entity.** `Solid3d` holds:
- the original ACIS bytes and their form (SAT or SAB), if it came from a file;
- the decoded body (`Arc`, filled on demand), so undo snapshots stay cheap;
- an `edited` flag: clear means the original bytes are written back exactly;
- the display wires the file supplied. The DWG record carries wireframe and silhouette data (ODA spec 18.1.39), so an unedited solid can be drawn without decoding it.

#### 5. Bringing the kernel in

- **Pin a commit of your fork**, `withshalom/solidcraft`, as a git dependency. Same gate as `withshalom/cadcraft`: you decide which upstream changes enter the fork.
- **Repeat the truck patches in DraftCraft's root `Cargo.toml`.** Cargo ignores a dependency's own `[patch]` section; the root manifest already notes this for craft-pdf. The patch entries point at the vendored crates inside the solidcraft repo. Confirm this resolves in the spike.
- **Don't take SolidCraft's `gpu` or `ui-egui` crates.** They are on egui 0.36 and SolidCraft vendors its own egui-wgpu. Both repos use wgpu 30, so shaders and techniques can be adapted.
- **Put it behind a cargo feature** (`solids`) until the cost is measured: build time, binary size, wasm size.
- **Attribution:** add truck and SolidCraft to `ATTRIBUTION.md` and the third-party licence list.

Things to verify in the spike:

- **Units and scale.** SolidCraft's drawing code speaks in millimetres. DraftCraft drawings are in whatever units the file uses, sometimes far from the origin. Check how kernel tolerances behave on an inch drawing, a metre drawing, and one at survey coordinates. Likely answer: hold each body near its own origin, with a placement transform.
- **Panics in the browser.** `guard` relies on catching a panic. Confirm how SolidCraft's web build handles that, and that DraftCraft's wasm profile matches.
- **Determinism.** The vendored truck fixes include making boolean results come out in a stable order. Confirm results are stable across runs, since undo and tests compare drawings.

#### 6. The ACIS translator

##### What is publicly known

- **SAT is documented.** Spatial published the save-file format for ACIS 4.0 and 7.0. SAB holds the same data elements in binary.
- **AutoCAD's dialect moved on.** Autodesk's ShapeManager grew out of ACIS. The ODA notes that from version 20800 AutoCAD uses modified SAT and SAB, that Spatial versions above 700 are incompatible with DWG, and that data downgraded to version 700 is usable.
- **One body per entity.** The ODA notes a DWG entity holds a single body.
- **The container side is in the ODA spec** already in this Project (18.1.39): version 1 is SAT text with a simple character substitution; version 2 is SAT or SAB inline; SAB ends with a fixed end marker.

So **version 700 is the writer's target**: documented, and accepted by DWG readers. The reader must accept newer dialects too.

##### Existing Rust code to evaluate (not yet reviewed)

- **acadrust** (MPL-2.0, already a dependency, used unmodified). Its docs describe SAT and SAB parsing and writing, and builders for box, wedge, pyramid, cylinder, cone, sphere and torus. Per your rule, MPL code may be *used* but never copied into MIT/Apache files.
- **cadmpeg-asm.** Its docs describe parsing and writing ShapeManager record streams. Confirm its licence before use; the cadmpeg crates SolidCraft uses are Apache-2.0.
- **cadmpeg's `sat` codec**, which SolidCraft leaves off. Worth asking upstream why.
- **Never read GPL sources** (LibreDWG) for this. That rule stands.

Either library could supply the record layer, leaving DraftCraft to write only the mapping to kernel bodies. Decide after reading them (owner's call 3).

##### Writer tiers (kernel body → ACIS 700)

| Tier | Covers | Unlocks |
|---|---|---|
| W1 | Planar faces, straight edges | BOX, WEDGE, EXTRUDE of straight-sided profiles, booleans between them. Also real REGION entities, replacing the polyline stand-in. |
| W2 | Cylinder, cone, sphere, torus; circular and elliptical edges | CYLINDER, holes, REVOLVE, EXTRUDE of arcs, straight fillets and chamfers |
| W3 | NURBS surfaces and curves | LOFT, SWEEP, variable fillets, anything free-form |

**The main technical risk is here.** truck's booleans produce intersection curves as approximations (SolidCraft's `vendor/README.md` says so). ACIS expects edges to lie on their faces within a tight tolerance. For W1 and W2 the writer should **recompute each edge exactly from its two faces** (plane and plane give a line; plane and cylinder give a circle or ellipse) and not export the approximated curve. The kernel already classifies faces and edges (`face_surface`, `edge_circle`, `planar_face`), which gives the writer what it needs. W3 has no such shortcut and needs testing against AutoCAD.

**Acceptance per tier** (your black-box check): AutoCAD and TrueView open the file with no recovery prompt, and AutoCAD's MASSPROP volume and centroid match the kernel's `measure_mass`.

##### Reader tiers (ACIS → kernel body)

| Tier | Covers |
|---|---|
| R1 | Planar bodies |
| R2 | Analytic surfaces |
| R3 | Spline surfaces, and procedural ones (blends, offsets, sweeps) |

- **R3 is the long tail.** A possible shortcut, to verify: procedural surfaces often carry a NURBS approximation alongside their definition, which could be read in their place.
- **Fallback rule.** A body that can't be translated exactly stays opaque: shown, moved, copied, saved byte for byte, but not edited. The user is told why. Nothing is guessed.
- **Round-trip test.** Read, write, read again, and compare volume, area and topology counts.

#### 7. What the DWG and DXF code needs

Small next to section 6, and mostly already on the roadmap:

- [ ] **Native reader:** decode the REGION/3DSOLID/BODY record (ODA 18.1.39): the inline ACIS data for older versions, the wireframe and silhouette data, and the history handle.
- [ ] **Native reader:** read the data-store section for 2013+ files, where the geometry lives. I found no reader for it in `crates/dwg`.
- [ ] **Native writer:** write the record and its data-store record for 2018.
- [ ] **New solids:** allocate a handle, write a data-store record bound to it, and extend the schema records if the file had none.
- [ ] **Class-defined 3D objects carried through:** SURFACE and its kinds, MESH, solid-history objects. These belong with roadmap phase 3, item 12.
- [ ] **DXF:** the same record on the DXF side. Pre-2013 DXF holds SAT text in the entity; 2013+ uses the data store (`sat_text_dropped` already reports that case).

#### 8. Stages

Sizes are relative: S, M, L, XL.

| # | Stage | Size | Done when |
|---|---|---|---|
| S0 | **Spike.** Branch, add the kernel behind a feature, extrude a rectangle, tessellate, print volume. | S | Builds on desktop and wasm; build time, binary and wasm size recorded; section 5's three checks answered. |
| S1 | **Carry solids natively.** Section 7's reader and writer items. No kernel. | M | A file with solids opens through the native reader and saves with every ACIS blob byte-identical. |
| S2 | **Model and wireframe.** `Solid3d` in `doc`; edges drawn as `Prim`s projected to the current view, from the file's wires or the kernel's edges. | M | Solids show, select, snap, move and copy in the existing 2D pipeline; open time on the 200k-entity benchmark unchanged. |
| S3 | **Create and edit, saved without ACIS.** First commands (below). Solids saved exactly in DraftCraft's own format with `encode_native_body`; STEP export. In DWG/DXF: a mesh AutoCAD can see, plus the exact body in an extension-dictionary record so DraftCraft reopens it as a solid. The save-loss notice says so. | L | Make, edit, save, reopen in DraftCraft: identical body. AutoCAD shows the mesh. |
| S4 | **3D view.** View direction per viewport, orbit, hidden and shaded styles, face and edge picking, UCS on a face. | L | Orbit and shade a 10,000-face model smoothly on desktop and in the browser. |
| S5 | **ACIS writer** W1 → W2 → W3. Replaces the S3 mesh tier by tier. | XL | Per tier, the AutoCAD acceptance in section 6. |
| S6 | **Drawings from solids.** FLATSHOT-style and section views through `hlr` and `section`, output as ordinary lines, arcs and hatches. | M | A projected view of a test part matches hand-checked geometry; uncertain edges are reported, not guessed. |
| S7 | **ACIS reader** R1 → R2 → R3. Existing solids become editable. | XL | Per tier, the round-trip test; untranslatable bodies stay opaque. |

**First commands (S3), by AutoCAD name:**

| Command | Kernel call |
|---|---|
| BOX, CYLINDER, SPHERE, TORUS | `box_solid`, `cylinder`, `sphere`, `torus` |
| EXTRUDE, PRESSPULL | `extrude`, `extrude_tapered` (from closed polylines, circles, regions) |
| REVOLVE | `revolve` |
| UNION, SUBTRACT, INTERSECT | `boolean` (extending the region commands that exist) |
| SLICE | `split_by_plane` |
| FILLETEDGE, CHAMFEREDGE | `fillet`, `chamfer` |
| SOLIDEDIT shell | `shell` |
| MASSPROP, INTERFERE | `measure_mass`, `meets` |
| Later: SWEEP, LOFT, THICKEN | `sweep`, `loft`, `thicken` |

Each keeps DraftCraft's command front half (names, aliases, prompts, ribbon) and adds only a back half.

**Order against the DWG roadmap:**

- **S0** any time. It is isolated and answers the cost questions.
- **S1** is roadmap phase 3 items 10 and 12 done properly. It belongs inside the reader work, not after it.
- **S2 onward** after the native reader takes over (roadmap phase 8), so the entity lands in a settled model.
- **S5** can use today's DXF → acadrust save path. It does not have to wait for the native writer.
- **S6** needs only S2 and S3. It can ship before any ACIS work.

#### 9. Risks

| Risk | Effect | Answer |
|---|---|---|
| AutoCAD rejects written ACIS | No DWG solids from DraftCraft | Tiers; exact edges for W1/W2; S3's mesh-plus-exact-body stays as the fallback |
| Kernel fails on a boolean or fillet | Command fails | `guard` returns an error; the command reports it and changes nothing |
| Kernel tolerances at odd units or large coordinates | Wrong or failed results | Spike check; local frame per body |
| Binary and wasm size, build time | Slower builds, bigger download | Measure in S0; cargo feature |
| Slower open | Breaks a standing rule | Lazy decode; benchmark gate at S2 |
| Upstream SolidCraft changes the kernel API | Pin breaks | Facade crate; pinned fork |
| ShapeManager dialects the reader doesn't know | Some solids not editable | Opaque fallback; never rewritten |
| Scope: this is a second product's worth of work | DWG fidelity slips | S0 and S1 only until the reader switch-over |

#### 10. Owner's calls

1. **Does SkryxDev agree?** This adds a large dependency and a model entity to the shared repo. Options: behind a feature in the shared repo, or on your fork until S3 works.
2. **Kernel as a git dependency on your fork** (recommended, like craft-libs) or copied into the DraftCraft repo.
3. **ACIS record layer:** acadrust's, cadmpeg-asm's, or our own `draftcraft-acis`. Decide after reviewing the first two.
4. **S3's interim DWG form:** mesh plus exact body (recommended), or refuse to save solids to DWG until S5.
5. **Is 3D in scope for DraftCraft's identity** as a drafting product, and how far: solids for drafting (through S6), or a full modeller (S7 and free-form)?

#### Sources

- `withshalom/solidcraft`: `crates/kernel/src/lib.rs`, `crates/geom/src/hlr/mod.rs`, `vendor/README.md`, `docs/step-import.md`, `ROADMAP.md`
- `withshalom/draftcraft-app`: `crates/doc/src/entity.rs`, `crates/doc/src/foreign.rs`, `crates/io/src/dxf_region.rs`, `crates/io/tests/it/acis_data_store.rs`, `crates/dwg/src/read/object.rs`, `AGENTS.md`, `ROADMAP.md`
- Open Design Specification for .dwg files, 18.1.39 (in this Project)
- [ODA: Native and modified .sat and .sab file versions](https://opendesign.com/node/1285)
- [SAT Save File Format (Spatial, via Paul Bourke)](https://paulbourke.net/dataformats/sat)
- [acadrust `entities::acis`](https://docs.rs/crate/acadrust/0.3.4/source/src/entities/acis/mod.rs)
- [cadmpeg-asm](https://docs.rs/cadmpeg-asm)
- [CAD Exchanger: ACIS format overview](https://cadexchanger.com/blog/3d-formats-overview-acis/)
