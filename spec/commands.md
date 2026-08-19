# Commands, tasks, scripting, extensions

## Commands and undo

- Every mutating action is a command with undo/redo (geometry feature, mesh replace, group edit, assignment).
- Viewport-only changes (camera, clip, display mode) are not on the undo stack.
- After undo, selection that pointed at destroyed IDs clears.

## Tasks

Import, tessellate, mesh, and quality run as tasks: cancellable, logged, one writer per component at a time (no two topology edits on the same body in parallel).

## Scripting and headless (reserved)

All commands must be invokable without the GUI (same functions the buttons call). That enables:

- Command log in the console (human-readable)
- Later: save/replay script
- Later: CLI `oxiprep mesh --in a.step --out a.vtk`
- Later: Python bindings on `document` / `geometry` / `mesh`

Do not build a second, UI-only code path.

## Extensibility

Register, don’t fork:

| Extension | Registers |
| --- | --- |
| Geometry/mesh I/O | extensions + read/write |
| Mesher | size/algorithm options + run |
| Case exporter | format + document walk |
| Tool / measure-like | parameters, pick mode, execute |
| Solver (later) | executable, input writer, output glob |
| Result reader (later) | format → field arrays on mesh |

UI builds menus from registrations (path + label + icon). No per-plugin `if name == ...` in the shell.
