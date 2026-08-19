# Product

Oxiprep is a desktop tool for preparing simulation models:

1. Import or build CAD geometry.
2. Clean, measure, and organize topology.
3. Generate and inspect mesh.
4. Name selections and attach materials, properties, and boundary conditions.
5. Export a mesh/case that a solver can run.

Later, the same application opens results on that model: contours, clips, plots, animation. Solver launch is also later. The document model and command layer must not assume preprocessing is the last stage.

Target users: analysts who mesh CAD for structural, thermal, and CFD work; developers who need a stable geometry/mesh API.

Platforms: Windows, macOS, Linux. UI: egui. Geometry kernel: OpenCASCADE via cadrum. Rendering: GPU (wgpu). License: Apache-2.0 for this project; OCCT remains LGPL-2.1.

## In scope now (preprocessor)

| Domain | Intent |
| --- | --- |
| Document | Native project; undo; units; object tree |
| Geometry | Import, primitives, booleans, transforms, common CAD features, defeaturing, measure |
| Mesh | Surface and volume generation, import/export, sets, quality, display, clip |
| Selections | Topology pick, box pick, named groups on geometry and mesh |
| Case setup | Materials, element properties, boundary conditions attached to groups |
| I/O | Industry CAD and mesh formats; solver input export |
| UI | Docked outliner, viewport, properties, console; File / Edit / View plus domain menus as features land |

## Reserved (same product, later)

| Domain | Intent |
| --- | --- |
| Solve | Registered external solvers, job launch, log tail, working directory |
| Post | Result files, field display, clip/slice/iso/vector/streamline, 2D plots, animation, image/video |
| Script | Recordable commands, headless CLI, Python or equivalent bindings |
| Remote | Submit/monitor jobs on another host |

Reserved means types, document slots, and UI places exist (or are planned with empty menus). It does not mean stub buttons that say “not wired”.

## Out of scope

- Bundling a specific commercial solver.
- Being a general CAD modeler (drawings, PMI, CAM).
- Real-time co-simulation.
- Cloud multi-user editing.
