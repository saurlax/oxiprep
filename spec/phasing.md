# Phasing

Priorities: **P0** ship first, **P1** complete preprocessor, **P2** stronger CAD/mesh, **L** later stages.

Unless the task says otherwise, implement P0 before P1, and do not start L work (solver UI, post UI, Python) while P0 is open.

## P0 — Solid preprocessor core

- Native project save/load (geometry + meshes + groups)
- Undo/redo
- STEP/BRep import-export; STL/VTK mesh import-export
- Tree show/hide, rename, close
- Geometry primitives (point, line, rectangle, disk, box, cylinder, cone, sphere)
- Face from closed edges; extrude; delete geometry
- Pick: geometry vertex/edge/face/body; mesh node/cell
- Standard views, fit, Faces / Edges / Mesh / Vertices display, axis clip plane
- Surface + tet volume mesh with global size
- Mesh sets from selection
- Measure distance
- Quality (core metrics) as cell scalar
- Console logging; background import/mesh

## P1 — Industry complete preprocessor

- IGES; OBJ; INP; CGNS; MEDIT; Gmsh msh; BDF read
- Booleans, move/rotate/mirror, fillet/chamfer, split, fill hole, remove face
- Sketch + revolve; loft/sweep; datum plane
- Geometry groups; mesh set merge/subtract; box select
- Local mesh size; triangle/quad-dominant surface options
- Node merge / unused node removal; create/delete faces
- Free clip plane widget for volume meshes
- Dimension queries (length, area, volume, angle, radius)
- Materials + BC types + INP/CGNS-oriented export
- Units in UI
- Screenshot

## P2

- Pattern/array, variable fillet, fill gap/sew
- Quadratic elements; hex-dominant; CFD layers
- Assembly-aware mesh-per-part improvements
- Command script replay; CLI
- Extra mesh formats (Fluent, SU2) as adapters

## L — Same architecture

- Solver manager and jobs ([solve.md](solve.md))
- Post pipeline ([post.md](post.md))
- Python API
- Reports (Word/PDF) only if still needed after plots/images

## Current baseline (not requirements)

The running app already: opens STEP/BRep/STL, lists models/bodies, shows a tessellated viewport with Faces / Edges / Mesh / Vertices toggles, axis clip, standard views, fit all/selection, click pick (body/face/edge/vertex/cell/node) with highlight and Shift/Command modifiers, properties (volume/area/bbox/counts), console, native project save/load (`.oxiprep`, BRep and meshes embedded), File New/Open/Save/Save As plus Import Geometry/Mesh, Cmd+O and drag-drop (`.oxiprep` opens a project; CAD/mesh files import), undo/redo of import, close, create, and delete (selected models, or the bodies that own the current selection). Geometry primitives (point, line, rectangle, disk, box, cylinder, cone, sphere). Edit menu has Undo/Redo/Delete. There is no extrude, no mesh generator, no groups. Box select is P1. Viewport camera and display are not on the undo stack.

Gaps against P0 are the rest of the P0 list above.

## Acceptance (P0)

A user can: New project → import STEP → create a box → boolean cut → name a face group → surface+tet mesh with a global size → see quality coloring → export VTK and STEP → save project → reopen with tree and mesh intact → undo the mesh and the boolean. The UI never explains missing features; they are simply absent until shipped.
