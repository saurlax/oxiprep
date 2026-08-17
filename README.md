# Oxiprep

Open-source **Rust CAE preprocessor**. This repository is an early scaffold: geometry kernel is pinned, product architecture (especially the client UI) is not.

## Current stack

| Layer | Choice | Status |
| --- | --- | --- |
| Language | Rust 1.97 (edition 2024) | pinned in `rust-toolchain.toml` |
| CAD / B-Rep kernel | [cadrum](https://crates.io/crates/cadrum) 0.8.16 (OpenCASCADE 8.x, statically linked, headless) | declared |
| Visualization / GUI | undecided | not started |
| Meshing / BC / solver export | undecided | not started |
| License | undecided | not started |

cadrum ships prebuilt OCCT for common native targets and can also target `wasm32`. It does **not** provide a viewport. Cross-platform UI options (egui, Iced, Slint, Tauri + wgpu, etc.) are still open.

OCCT inside cadrum is LGPL-2.1; distributing a binary that links it must follow those terms.

## Build

Requires [Rust](https://rustup.rs/) 1.97+. The first build that compiles `cadrum` downloads a prebuilt OpenCASCADE tarball (no system OCCT install).

```bash
cargo run
```

## Intentional non-goals for this commit

- No UI, renderer, or plugin system yet
- No mesh/BC/solver workflow yet
- `cadrum` is listed as a dependency so the kernel choice is recorded; application code does not call it yet
