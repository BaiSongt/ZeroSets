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

### V0.5 命名变量与参数绑定

1. 在 Inspector 的数值参数旁点击 `+`，可由当前值创建一个命名变量；也可从 Ribbon 的「变量」浮层中新建。
2. 在另一个数值参数的「变量绑定」下拉框里选择同一个变量，即可共同驱动多个 Block。只允许相同量纲的绑定；长度和角度单位可自动换算。
3. 修改变量会更新所有绑定值，并高亮受影响的下游 Block。删除变量会解除绑定、保留最后的数值；支持 Undo/Redo。
4. JSON 快照保留可选的 `variables` 和 `variable_refs` 元数据，WASM 仍接收原版 v1 Number 参数，无需修改 Rust 内核。

本地变量/Graph 测试：在 `bindings/web` 下运行 `pnpm test:unit`（不使用 GitHub CI）。

### V0.6 变量公式与依赖

1. 在「变量」面板中先创建 `baseRadius = 12 mm`、`wallThickness = 2 mm` 两个普通变量，再创建 `outerRadius`（单位选毫米）。
2. 在 `outerRadius` 卡片的 `ƒx` 输入框中填写 `baseRadius + wallThickness`，按回车或离开输入框应用，结果为 `14 mm`；也可写 `baseRadius + 2 mm` 或 `baseRadius * 1.5`。
3. 将 Block 的数值参数绑定到 `outerRadius`。修改 `baseRadius` 后，公式与已绑定 Block 同步更新，并高亮受影响的下游依赖。
4. 支持 `+ - * /`、括号、负号、科学计数法、中文变量名，以及 `mm`、`cm`、`m`、`in`、`inch`、`rad`、`deg` 单位后缀；加减要求同量纲，乘除只允许无量纲缩放或同量纲相除。
5. 系统会拒绝循环引用、未定义变量、除零和不兼容单位。重命名变量会同步修改公式引用；有其他公式依赖的变量必须先解除依赖才能删除。清空公式可恢复手动数值。

公式以可选的 `expression` 字段保存在 v1 JSON 中，计算值仍按旧版 Number 参数传给 WASM；变量脚本**不执行任意 JavaScript**。

### Graph 可编辑工作区（V0.7）

1. 切到 **Graph**，在画布空白处右键、按 `A` 或点击顶部 `+`，打开 Block 快速搜索；输入名称后按 Enter，节点会落在对应画布位置，不会自动改变缩放与平移。
2. 从一个节点的**输出端口**拖到另一个节点的**输入端口**建立连接；也可反向从输入拖到输出以重连。连接前预览可用端口，并阻止类型不匹配及循环依赖。
3. 将输出端口拖到空白处，可直接选择一个兼容的新 Block 并自动连上；将已连接的输入端口拖到空白处则会断开连接。双击或右键已有边也能断开。
4. 选中节点后，右侧 Inspector 可设置名称、重载、数值/变量绑定、启用状态；双击节点或点击节点内 `⚙` 进入属性。在 Graph **展开**模式下，`⚙` 会打开可关闭的浮动属性抽屉，不会压缩画布。
5. 鼠标中键拖动画布、滚轮缩放，支持多选、Delete、复制与 Undo/Redo。Graph 节点坐标保存在 JSON 的可选 `graph_layout` 字段中，旧版文件仍可加载；Graph 与 Notebook 始终共用同一份计算快照。

Graph 交互规则测试：在 `bindings/web` 下运行 `pnpm test:graph`，或运行 `pnpm test:unit` 执行全部用例。

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
