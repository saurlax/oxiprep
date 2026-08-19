# User interface

The product UI is a tool, not a status report. See also `AGENTS.md`.

## Shell

Keep the current dock layout as the default:

| Panel | Role |
| --- | --- |
| Outliner | Project tree |
| Viewport | 3D view, pick, clip |
| Properties | Selection stats and active-command parameters |
| Console | Log of commands, errors, task progress |

Add when needed, still dockable:

- Message / output (solver and mesher stdout) — can start as Console
- Attribute / field legend (mesh quality now; result fields later)
- Task progress for long mesh/import jobs

Panels that have no content stay blank or use a short empty state (“No selection.”, “No models loaded.”). Do not put implementation notes in the chrome.

Menus exist only for shipped commands. Planned domains may have empty parent menus (Edit, View already exist). Do not add Solve or Post menus until those commands exist.

## File menu (preprocessor)

- New project
- Open project
- Save / Save as
- Import geometry…
- Import mesh…
- Export geometry…
- Export mesh…
- Export case / solver input… (when case export exists)
- Close
- Quit

Drag-and-drop onto the viewport imports using the same format registry as File → Import.

## Edit

- Undo / Redo
- Delete — selected models, or the bodies that own the current selection (face, edge, vertex, cell, node). If every body of a model is targeted, the model is removed. Mesh sub-entity delete is later.
- Preferences (units, tessellation, mouse, working directory)

## Geometry

- Point, Line, Rectangle, Disk
- Box, Cylinder, Cone, Sphere

Parameters appear in Properties. Create appends a new model, or a body on the current model when that option is on.

## Mesh

- Surface
- Volume

Parameters (global size) appear in Properties. Mesh applies to the selected solids, or all solids if nothing is selected. Volume fills the surface mesh with tetrahedra.

## View

- Fit all / fit selection
- Standard views: +X −X +Y −Y +Z −Z, isometric
- Display toggles: Faces, Edges (CAD feature edges), Mesh (tessellation edges), Vertices
- Clip plane: axis X/Y/Z, offset, flip (view filter, not a geometry split)
- Pick mode is on the viewport bar: Off, Body, Face, Edge, Vertex, Cell, Node
- Save screenshot
- Background / appearance (egui defaults; no custom theme unless requested)

## Status bar

Factual: app version, model/mesh counts, current units, running task if any. No kernel names, no “not implemented”.

## Preferences

- Display units
- Tessellation deflection (linear/angular) for CAD display
- Mouse navigation convention
- Default mesher and default sizes
- Working directory
- Recent files

Theme: egui default.
