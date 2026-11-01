# ZeroSets

Experimental implicit-modeling kernel and nTop-style notebook UI.

This repository contains the standalone kernel/UI work split out from the
[ZeroSet](https://github.com/BaiSongt/ZeroSet) book repository.

## Layout

- `kernel/` — Rust workspace: core field graph and meshing, wgpu adaptive
  meshing, the block/notebook layer, CLI, wasm and napi bindings.
- `bindings/web/` — Vite + TypeScript + three.js Notebook prototype. It talks
  to the kernel through the wasm binding and includes the nTop-style ribbon,
  node graph, responsive panels, i18n, and light/dark themes.
- `bindings/node/` — napi-rs Node binding and smoke/demo scripts.

## Quick start

Build and test the Rust workspace:

```powershell
cd kernel
cargo test -p zeroset-core --lib
cargo test -p zeroset-gpu --lib -- --test-threads=1
cargo test -p zeroset-blocks
cargo test -p zeroset-wasm
cargo test -p zeroset-napi
```

Build the web app (the checked-in `pkg/` output is generated locally):

```powershell
cd kernel
cargo build -p zeroset-wasm --target wasm32-unknown-unknown --release
wasm-bindgen target\wasm32-unknown-unknown\release\zeroset_wasm.wasm `
  --target web --out-dir ..\bindings\web\pkg --out-name zeroset_wasm

cd ..\bindings\web
pnpm install
pnpm build
pnpm dev
```

The dev server serves the Notebook prototype at `http://127.0.0.1:5173/`.
The UI uses localStorage keys `zeroset.theme`, `zeroset.language`, and
`zeroset.panel-layout` for persisted preferences and panel widths.

## Documentation

- [Kernel, meshing, and UI notes](kernel/README.md)
- [nTop UI / kernel research](kernel/docs/ntop-ui-and-kernel-notes.md)
- [Notebook UI screenshot](bindings/web/notebook.png)
- [Light theme screenshot](bindings/web/ui-i18n-light.png)
- [English dark theme screenshot](bindings/web/ui-i18n-en-dark.png)

## Status

The kernel exposes analytic SDF/CSG fields, Surface Nets and Dual Contouring
extractors, adaptive mixed-level meshing, QEM simplification, block/notebook
evaluation, packed ZSM2 meshes, and wasm/napi host bindings. The notebook UI
covers block graph editing, dynamic List/Custom Field ports, Custom Field
expression templates and parameter presets, structured diagnostics with
click-to-source, Chinese/English UI, and light/dark themes.

Rust workspace license metadata is `MIT OR Apache-2.0`.
