# Architecture

Keep a one-way dependency stack. UI never owns CAD or mesh truth. Later crates (solve, post) depend on the same core, not on egui.

```text
┌─────────────────────────────────────────────────────────┐
│  UI (egui): menus, docks, viewport, property editors     │
└────────────────────────────┬────────────────────────────┘
                             │ commands / events
┌────────────────────────────▼────────────────────────────┐
│  Session: command stack, selection, tasks, preferences   │
└────────────────────────────┬────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────┐
│  Document                                                │
│  Geometry │ Mesh │ Groups │ Materials │ Case │ Results*  │
└──────┬──────────┬───────────┬────────────┬──────────────┘
       │          │           │            │
   kernel I/O  mesh algos   case I/O    result I/O*
```

`*` Results are empty until postprocessing ships. The document still has a results slot so save/load and the tree do not get a breaking redesign.

Logical modules (one crate until a module is large enough to split):

| Module | Responsibility |
| --- | --- |
| `document` | Project graph, IDs, dirty flag, snapshot/undo boundaries |
| `geometry` | BRep solids/faces/edges/vertices, features, tessellation for display |
| `mesh` | Nodes, elements, sets, quality, geometry association |
| `groups` | Named selections spanning geometry and/or mesh |
| `materials` | Material library and assignments |
| `case` | Physics model, BCs, loads, solver-neutral settings |
| `io` | Format adapters registered by extension |
| `command` | Undoable operations; all writes go through here |
| `task` | Background jobs (import, mesh, later solve); UI stays responsive |
| `view` | Display meshes, picking IDs, clip planes — no mutation |
| `ui` | Shell only |
| `solve` | Later: solver registry and process control |
| `post` | Later: result pipeline consuming `mesh` + field arrays |

Rules:

- Geometry and mesh are first-class and may exist together on one component (CAD + mesh of that CAD).
- A component may be mesh-only (imported STL/VTK) or geometry-only (unmeshed CAD).
- Field data (quality scalars now; result fields later) hang on mesh entities with a name, association (node/cell), and components. Viewport attribute display is shared with postprocessing.
- Plugins/backends (mesher, file format, later solver) register into systems; the UI does not special-case plugin names.
- Writes go through operators/commands so undo, dirty tracking, and scripting can hook one path.
