# Oxiprep

Rust CAE preprocessor. egui / eframe, cadrum (OpenCASCADE).

## Spec

Product requirements live in [`spec/`](spec/README.md). Before implementing a feature, read `spec/README.md` and the domain file that matches the task (`spec/geometry.md`, `spec/mesh.md`, `spec/ui.md`, …). Check [`spec/phasing.md`](spec/phasing.md) and stay on the current priority unless the user asks otherwise.

The spec is the intended product, not a description of the current code. Implement against it. Do not add commands, panels, or menus the spec does not call for. Do not start solver or postprocessing UI while preprocessor P0 is open. Architecture rules (one-way dependencies, commands as the only write path, empty Results/Case slots) are in [`spec/architecture.md`](spec/architecture.md).

If a requested change conflicts with the spec, say so and ask whether to update `spec/` first.

## UI

The product UI is a tool, not a status report. Do not put implementation notes, TODOs, kernel names, or “not wired yet” in menus, panels, or the status bar. Empty panels stay blank or use a short factual empty state. Unused menus stay empty.

Use egui defaults. Do not add a custom theme unless asked.

Details: [`spec/ui.md`](spec/ui.md).

## Code

Do not commit unless asked. Oxiprep is Apache-2.0; OCCT stays LGPL-2.1. Do not relicense.
