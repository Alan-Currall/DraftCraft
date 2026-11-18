# Release handoff: main after the 3D split (2026-11-18)

**For:** SkryxDev. **From:** Sophie (Claude Code), for Shalom (`withshalom`).

## What changed

The 3D work is off main and lives on the `3D` branch until it is finished.

- **PR #26** took the 3D work off main: PR #22 (3D solids on solidcore), PR #23 (3D transforms, lumps, mirrors) and `3D-dev-state.MD`. It reverted them; main's history was not rewritten.
- **PR #27** put the 3D work back on `3D`. From that point, main merges into `3D` without deleting the 3D code.

**These stayed on main:**
- all the DWG work, ACIS in DWG included
- AutoComplete (#24), the Settings file (#25), AREATOTAL, conic Rho
- the `$CTABLESTYLE` fix (`afccf04`)
- the DWG 2000–2010 SAT-text fixes (from `c362986` and `2f3b3ef`)

## Branches

| Branch | What it is |
|---|---|
| `main` | Releasable. No 3D code (`crates/solid`, the `solids` feature, the 3D commands and views are on `3D`). |
| `3D` | All 3D work. Codie works here. 3D pull requests target `3D`. |
| `backup-11-18-2026` | main as it was before the split (`1506ce6`). Do not change it. |

The rules are in `AGENTS.md`, under "Branches: main is always releasable":
- **3D work goes to `3D`.** A change that needs the solid kernel goes to `3D`.
- **DWG and DXF work goes to main**, ACIS data included, when it works without the `solids` feature.
- **Merge main into `3D` at least weekly.** These merges are ordinary now. When 3D is finished, `3D` comes back to main in one pull request.

## The release: 3.0.0 is reserved for 3D

Main's next release is **not 3.0.0**. That number goes with the 3D release from `3D`.

Main still says 3.0.0, so renumber before tagging:
- `Cargo.toml` says `version = "3.0.0"`.
- `CHANGELOG.md` has a `## 3.0.0` section (DWG writer, What's New, ATTOUT/ATTIN, installers, …). Under `## Unreleased` it also has the Settings file, AutoComplete, AREATOTAL, Rho and the `$CTABLESTYLE` fix.

Suggested steps (2.2.0 is a suggestion; Shalom has not picked the number):
1. Merge the `## 3.0.0` notes and the `## Unreleased` lines into one section for the new version. Run `cargo xtask version set X.Y.Z` and check `the_changelog_has_notes_for_this_version`.
2. Run `cargo xtask ci`, merge, then tag `vX.Y.Z` on main. The tag builds every package (`docs/releasing.md`).
3. On the next merge of main into `3D`, resolve the version line and the changelog headings by hand: `3D` keeps 3.0.0 for itself.

## Open before the release: DXF → DWG drops some objects

`draftcraft-cli roundtrip` in strict mode (`DRAFTCRAFT_CORPUS_STRICT=1`) shows that saving an AutoCAD DXF as DWG drops:
- MATERIAL
- MESH
- the surface entities (EXTRUDEDSURFACE, LOFTEDSURFACE, …)
- AutoCAD's solid-history and associative objects (ACSH_*, ACDBASSOC*)

3DSOLID, REGION and BODY are kept. This was already on main before the split; the split did not cause it.

The 3.0.0 notes say DWG "keeps … materials". Either fix the writer (`crates/io/src/dwg/write/`) or word the notes to match. The drawings that show it are on `3D` under `crates/io/tests/data/acad-solids/`.

## Small things

- With Rust 1.97, clippy flags `manual_range_contains` in `crates/engine/tests/it/arc_ctrl_direction.rs:119`. CI's toolchain passes it today; it is a one-line fix when CI moves to a newer Rust.
- `gpu_viewport_stencil` needs a GPU adapter, so it fails on machines without one (it passes on CI).
