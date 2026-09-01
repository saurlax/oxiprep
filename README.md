<p align="center">
  <img src="assets/logo.svg" width="128" alt="Oxiprep">
</p>

<h1 align="center">Oxiprep</h1>

<p align="center">
  Open-source Rust CAE preprocessor.<br>
  egui + <a href="https://crates.io/crates/cadrum">cadrum</a> (OpenCASCADE 8.x)
</p>

## Setup

Rust 1.97 (see `rust-toolchain.toml`).

```bash
cargo run
```

```bash
cargo bundle --release
```

The first build downloads a prebuilt OpenCASCADE library.

## Contributing

Issues and pull requests are welcome. Product changes are managed with
[OpenSpec](https://github.com/Fission-AI/OpenSpec). Install the CLI and use the
generated Codex workflows to propose, implement, and archive a change:

```bash
npm install -g @fission-ai/openspec@1.11.0
openspec list
```

In Codex, start with `$openspec-propose`, continue with
`$openspec-apply-change`, and finish with `$openspec-archive-change`. Existing
product requirements under [`spec/`](spec/README.md) are source material for
the proposal. Run `openspec validate --all --strict --no-interactive` and
`cargo fmt --check` before sending a change.

## License

Oxiprep is licensed under the [Apache License 2.0](LICENSE).

[cadrum](https://crates.io/crates/cadrum) is MIT. OpenCASCADE Technology (OCCT), which cadrum links statically, is [LGPL-2.1 with the Open CASCADE exception](https://dev.opencascade.org/resources/licensing). That applies to OCCT itself—prebuilt or built from source—not to this repository’s Apache-2.0 code. Binaries that include OCCT must also meet the LGPL-2.1 terms.
