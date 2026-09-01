# Oxiprep

Rust CAE preprocessor. egui / eframe, cadrum (OpenCASCADE).

## OpenSpec

OpenSpec manages product changes. Project configuration is in [`openspec/config.yaml`](openspec/config.yaml), active proposals live in `openspec/changes/`, and requirements accumulated from archived changes live in `openspec/specs/`.

Use the OpenSpec workflow for new features, behavior changes, breaking changes, and substantial refactors:

1. Explore the affected code and product baseline.
2. Create and review an OpenSpec proposal, delta specs, design, and tasks.
3. Implement only after the planning artifacts are ready.
4. Validate and archive the completed change so its requirements are merged into the main OpenSpec specs.

The Codex workflows are installed under `.agents/skills/`. Use `$openspec-propose`, `$openspec-apply-change`, and `$openspec-archive-change` for the normal lifecycle. Use `$openspec-explore` when the scope still needs investigation.

The existing documents in [`spec/`](spec/README.md) are the brownfield product baseline and source material; they are not a description of the current code and are not bulk-copied into OpenSpec. Before proposing or implementing a feature, read `spec/README.md`, `spec/phasing.md`, `spec/architecture.md`, and the domain file that matches the task (`spec/geometry.md`, `spec/mesh.md`, `spec/ui.md`, …). Stay on the current priority unless the user asks otherwise.

Do not add commands, panels, or menus that neither the product baseline nor an approved OpenSpec change calls for. Do not start solver or postprocessing UI while preprocessor P0 is open. Architecture rules (one-way dependencies, commands as the only write path, empty Results/Case slots) are in [`spec/architecture.md`](spec/architecture.md).

If a requested change conflicts with the product baseline, surface the conflict in the proposal and ask whether the baseline should change; do not silently override it.

## UI

The product UI is a tool, not a status report. Do not put implementation notes, TODOs, kernel names, or “not wired yet” in menus, panels, or the status bar. Empty panels stay blank or use a short factual empty state. Unused menus stay empty.

Use egui defaults. Do not add a custom theme unless asked.

Details: [`spec/ui.md`](spec/ui.md).

## Code

Do not commit unless asked. Oxiprep is Apache-2.0; OCCT stays LGPL-2.1. Do not relicense.
