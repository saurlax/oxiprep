# Oxiprep specification

Product requirements. These files describe the intended product, not the current code.

Change management now uses [OpenSpec](../openspec/config.yaml). Treat this
directory as the brownfield product baseline and source material when creating
an OpenSpec change. New or changed behavior is proposed under
`openspec/changes/`; after implementation and archive, its testable requirements
accumulate under `openspec/specs/`. The baseline remains binding for areas that
OpenSpec has not touched.

Preprocessor first (geometry, mesh, case setup). Solver control and postprocessing are later stages on the same document and UI shell. UI copy in the running app stays factual; unused menus and panels stay empty.

## How to read

1. Start here, then [product](product.md) and [architecture](architecture.md).
2. Open the domain file that matches the task.
3. Check [phasing](phasing.md) so work stays on the agreed priority.

Agents: follow `AGENTS.md` and the OpenSpec workflow. Implement against these
files plus the active approved change; do not add chrome or commands that
neither calls for.

## Files

| File | Contents |
| --- | --- |
| [product.md](product.md) | What Oxiprep is; in scope, reserved, out of scope |
| [architecture.md](architecture.md) | Dependency stack, modules, write path |
| [document.md](document.md) | Project, tree, IDs, units |
| [ui.md](ui.md) | Shell, menus, status bar, preferences |
| [viewport.md](viewport.md) | Camera, picking, clip, field coloring |
| [geometry.md](geometry.md) | CAD import, create, features, groups |
| [mesh.md](mesh.md) | Representation, I/O, generation, sets, quality |
| [tools.md](tools.md) | Measure and dimension queries |
| [case.md](case.md) | Materials, properties, BCs, case export |
| [commands.md](commands.md) | Undo, tasks, scripting, extension points |
| [solve.md](solve.md) | Later: solver jobs |
| [post.md](post.md) | Later: results visualization |
| [phasing.md](phasing.md) | P0 / P1 / P2 / L, current baseline, P0 acceptance |
