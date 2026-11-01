# nTop 界面与分辨率机制调研

> 目标：对标 nTop 的交互模型，但先做内核。本文只记录会影响内核/GPU 设计的事实，不做像素级仿制。

## 资料来源

- [Main Window](https://docs.ntop.com/Product-Documentation/ntop/_user_interface/_main_window)
- [Ribbon](https://docs.ntop.com/Product-Documentation/ntop/_user_interface/_main_window/ribbon)
- [Resolution](https://docs.ntop.com/Product-Documentation/ntop/_user_interface/_main_window/resolution)
- [Notebook Anatomy](https://docs.ntop.com/Product-Documentation/ntop/_using-blocks/_notebook_anatomy)
- [Block Anatomy](https://docs.ntop.com/Product-Documentation/ntop/_using-blocks/_block_anatomy)
- [What is adaptive resolution?](https://docs.ntop.com/help-articles/knowledge-base/user-interface/what-is-adaptive-resolution)
- [nTop 5.0 - New Implicit Modeling Kernel](https://docs.ntop.com/help-articles/knowledge-base/backwards-compatibility-notices/ntop-5-0-new-implicit-modeling-kernel)
- [Why isn't GPU Acceleration working?](https://docs.ntop.com/help-articles/access-and-installation/setup-and-install-troubleshooting/why-isn-t-the-gpu-acceleration-working)

## 1. 主窗口的五个区域

| 区域 | nTop 的做法 | 对内核/前端的含义 |
|---|---|---|
| Menu Bar | 文件、视图、窗口、帮助、账户 | 与内核无关，前端壳层 |
| Ribbon | 按工作流分组的 Block 图标，带 tooltip、Toolkit、Beta 页 | 需要 **Block 注册表**：分类、图标、描述、输入输出类型 |
| Notebook | 左侧工作区，Block 串联成工作流；有搜索、Section、变量、Outline、错误日志、Auto-Update | 需要 **实例图 + 稳定 ID + 缓存/脏传播 + 逐块状态** |
| Viewport | 3D 交互、View Cube、显示设置 | 需要 **GPU 视口渲染器**，与求值内核解耦 |
| Status Bar | 3D Resolution、Block 进度、Log Panel | 分辨率是“显示策略”，不是几何输出精度 |

Notebook 还分左右侧栏：

- 左：Outline / Imports。Outline 提供块级导航、变量引用追踪、搜索与实例计数。
- 右：Information / Display。Information 展示 Block 描述、输入、输出、可转换类型、版本；Display 控制颜色、透明度。

Block 卡片本身包含：折叠、输出类型色条、Block 图标、类型重载、名称、实例名、Rebuild 按钮、Details、可见性、输入图标/名称/单位/校验状态。

## 2. Block 模型给内核的约束

nTop 的 Block 不是“函数列表”，而是带类型和状态的一等对象：

1. **输入/输出有类型**（Implicit Body、Mesh、Field、Scalar、Vector、Transform 等），颜色编码；
2. **支持 overload**：同名 Block 可有不同输入组合；
3. **单位是输入的一部分**，不是注释；
4. **实例有独立身份**：可重命名、可被变量引用、可单独 rebuild；
5. **每个实例有状态**：unbuilt / built / warning / error，错误可定位到具体输入；
6. **Auto-Update 与 Manual Run 并存**：上游变化是否触发下游重算，是策略而不是硬编码；
7. **缓存是几何结果的一部分**：修改变量只重算受影响子图，不能全图重跑。

这意味着我们的 `FieldGraph` 只是“数学 DAG”，还需要一层“Block 实例图”：

```text
BlockRegistry
  BlockDef { id, name, category, inputs, outputs, overloads, version }
        │
Notebook
  BlockInstance { instance_id, def_id, input_bindings, params, display, status }
        │
Evaluator
  cache: instance_id -> Value
  dirty propagation + manual/auto policy
        │
FieldGraph (arithmetic DAG) / Mesh / Scalar
```

## 3. Resolution：显示分辨率与几何精度分离

nTop 的 Resolution 位于 Status Bar 右下角，有五档：

- Low
- Medium
- High
- Highest
- Adaptive

关键事实：

- 分辨率是 **显示设置**，不改变导出网格/几何的输出质量；
- 文件保存后会继承上次的分辨率；
- 分辨率越高，渲染时间越长，但不改变文件大小；
- 需要更高画质时可用 **Precise Render**（View → Precise Render / Ctrl+H），生成一张当前最高渲染的快照；
- **Adaptive** 会根据当前视角动态调整局部隐式分辨率，最高约比 Highest 每轴高 20 倍；
- Adaptive 需要至少 4GB VRAM；GPU Acceleration 是另一个开关，Adaptive 不依赖它，但 GPU 关闭时有多 implicit 卡 UI 的已知问题；
- 开启 Adaptive 后首次渲染更慢，旋转/移动可能掉帧；Surface Plot 和交互式实时渲染在 beta 期不支持。

这给出一个明确的分层：

```text
几何精度（输出）     ≠     显示分辨率（视口）
- 网格 tolerance            - Low/Medium/High/Highest
- SDF 求值精度              - Adaptive（视角相关）
- 单位/容差                 - Precise Render（离屏快照）
```

内核不应该把“视口分辨率”写进几何节点。视口分辨率属于 **渲染策略**，可以随相机、屏幕空间误差、GPU 内存预算变化；几何输出仍由 tolerance/units 决定。

## 4. nTop 5.0 新隐式内核的公开信息

nTop 5.0（2024-06-24）换成新隐式内核，公开的收益：

- 大规模布尔：数百次布尔（穿孔、纹理）从小时级降到秒级；
- 渲染、提面、切片速度提升；
- 精度提升：更锐利的网格边、更薄的 TPMS 壁、机加工件微米级公差；
- 支撑 nTop Core 开发者库，让合作方直接处理 implicit body，避免提面瓶颈。

对我们的启示：

- **DAG 求值缓存 + 布尔批处理**比单点 SDF 优化更重要；
- **提面/切片/渲染共用同一求值内核**，不要各写一套；
- **精度是内核卖点**，需要显式的精度/容差模型，而不是“f32 一把梭”；
- **对外 API 要能嵌进别人的应用**（C ABI / napi / wasm），不能只做桌面 UI。

## 5. GPU Adaptive Resolution 的技术拆解

nTop 的 Adaptive 是“基于视角动态调整局部隐式分辨率”，最高每轴约 20x Highest。若用稠密体素，20x 每轴意味着 8000x 内存，不可行；因此它更可能是：

1. **屏幕空间求值/光线步进**：每个像素沿射线求 SDF，分辨率由屏幕像素和步进质量决定；
2. **分块 LOD**：视口按屏幕空间误差选择 tile 的采样级别；
3. **渐进式细化**：先低分辨率出图，再对可见区域/边缘逐步加密；
4. **GPU 内存预算**：Adaptive 有独立的 GPU Memory Usage 设置，说明存在 tile/缓存池。

我们的内核对应实现：

- `GpuSampler::sample_tiles`：一次 dispatch 批量求值多个 tile；
- `sample_adaptive`：以 bounds 中心为参考帧，八叉树式细分，只保留零集附近 tile；
- `FieldGraph::rebased`：把模型搬到局部坐标，避免大坐标 f32 精度损失；
- `FieldGraph::compacted` + `Notebook::compact`：按缓存输出收集可达节点，回收改参数后遗留的孤儿子图并重映射字段句柄；
- 已把 raymarch、TAA 和 tile LOD 接到视口，并把 adaptive tile 接到无裂缝窄带提面；

## 6. 精度策略

nTop 5.0 明确把 precision 作为内核升级点。我们的路线：

| 层级 | 当前 | 下一步 |
|---|---|---|
| 坐标 | `f32`，`rebased` 局部帧 | 相机相对坐标、tile 局部原点 |
| 求值 | CPU/GPU `f32` | CPU `f64` 参考、GPU double-single 关键路径 |
| 提面 | Surface Nets，面法向 | Dual Contouring + QEF、锐边保留 |
| 容差 | 固定 padding/band | 世界单位 + 尺度化容差、误差预算 |
| 验证 | CPU/GPU 对拍 | f64 golden、Hausdorff、薄壁/最小特征测试 |

## 7. 我们不准备照搬的部分

- Ribbon 的高密度图标和固定分类：前期用命令面板/搜索即可，避免 UI 先于内核。
- 云端/账户/协作：与内核实验无关。
- 每个 Block 一套独立求值逻辑：必须共享同一个 `FieldGraph`/求值器，否则缓存和精度会分裂。

## 8. 近期实现顺序

1. ✅ **Block 注册表 + 实例图 + Web Notebook UI**：类型、单位、overload、状态、缓存/脏传播、版本化 JSON 快照；wasm UI 原型已接 nTop 风格标题栏 + 分组 Ribbon（File/Edit/Build/View）、Block 库搜索（`/` 聚焦）、Block 库/Properties/视口三处可拖拽分栏（宽度持久化，双击复位）、1120/900/860/720px 自适应断点（窄屏 Library/Inspector 抽屉）、底部状态栏、Block 库/Properties/视口、自动布局节点画布、缩放/平移/Fit、Shift 多选/框选/成组拖动/批量操作/对齐分布/连线断开/Arrange、Features 内核特征边 overlay（阈值来自 feature_angle）、**自由端口 List block**（任意端口名/连线、动态 `＋ add field` / `Remove`、端口重命名/↑↓ 排序、快照往返）、Custom Field（def/函数体 let/vec/$var/and-or-not/lazy if/编译期 for、可选 field 输入 a–d 以 $a–$d 绑定、UI 任意 field 端口以 $name 绑定、可选 Number 输入 scale/offset 以 $scale/$offset 绑定、可选 Vector/Boolean 输入 translation/flag 以 $translation/$flag 绑定、行/列+caret 错误诊断（Rust `Diagnostic` 暴露 line/column/snippet，wasm `notebook_diagnostics` 返回 JSON（含 `source`）、UI 渲染结构化错误卡并可点击跳转源 block 与 caret、`custom_templates()` 提供 10 个表达式预设及参数预设（`scale`/`offset`/`translation`/`flag`）并接入下拉）、多行编辑）、Mesh crease/simplify 参数、单位下拉、SVG 贝塞尔连线、输出口拖线、Auto/Manual 更新、Undo/Redo、中英文切换（`i18n.ts` 词典 + 动态状态翻译）、全局明暗主题（`theme.ts` + `themes.css` 语义变量 + three.js 场景同步，localStorage 持久化）；
2. ✅ **headless GPU raymarch**：相机相对、渐进分辨率、逐像素屏幕空间 epsilon；
3. ✅ **wgpu surface 视口**：拖拽 0.5x、空闲 1.0x、motion-vector 重投影 TAA、16×16 tile 质量 LOD；
4. ✅ **GPU 解析梯度**：逐节点发射 `zs_grad_*`，bracket 上对 CPU 误差 < 1e-2；
5. ✅ **自适应窄带提面**：adaptive tile → 全局稀疏 Surface Nets 或 QEF 顶点（`mesh_adaptive_dc`），统一 leaf cell 时无裂缝；truncated grid 可 refine 后走 SN/DC，f64 refined SN/DC 也已接；`mesh_adaptive_mixed` 直接产出 closed mixed-level SN/DC（truncated sphere: 1,610 verts / 3,216 tris；SN volume 4.121312 / Hausdorff 0.008492，DC volume 4.141012 / Hausdorff 0.006338；sharp box volume 7.999996 / Hausdorff 0.072168；thin wall volume 0.3999996 / Hausdorff 0.032004；torus volume 2.283193 / Hausdorff 0.024744；capsule 644 verts / 1,284 tris / volume 0.544772；cylinder 1,150 / 2,296 / volume 1.169305；三孔板 uniform 4,520 / 9,048、closed、volume 1.599245 / Hausdorff 0.072168，truncated raw mixed 仍有 nonmanifold=158，现自动回退 refined 得到 4,520 / 9,048、closed、volume 1.574660；refined SN/DC 逐三角形按梯度统一 winding，三孔板 refined SN volume 0.219447→1.535693、DC 0.283532→1.575056；open boundary 804 verts / 1,218 tris / 60 boundary edges；裁剪 bounds 781 verts / 1,516 tris / 44 boundary edges，cap 后 782 verts / 1,560 tris / closed）；feature-aware mixed QEF + offset-aware mass-point blend + simple boundary cap 已接；
6. ✅ **Dual Contouring + QEF**：uniform grid 上解析梯度 + cell clamp + feature-edge/feature-corner constraint + feature-adaptive regularization，尖锐盒 max error 到 2.4e-7；`with_crease_normals` 可拆分锐边法向；Mesh block 已提供 `dense` / `dual_contour` overload；f32/f64 adaptive refined DC 已接，mixed-level DC 原型已接；
7. ⏳ 屏幕空间误差驱动的 Adaptive Resolution（把 tile 采样和 raymarch 接起来）；
8. ✅ **f64 CPU 参考求值/梯度/提面 + Hausdorff**：`evaluate_f64` / 解析 `gradient_f64_analytic` / `sample_grid_f64`、f64 世界 SN/DC + 局部 SN/DC，`Mesh::hausdorff_distance` BVH 采样；f64 自适应提面待补；
9. ⏳ napi/wasm 暴露同一套 `BlockRegistry` 与 `ResolutionPolicy`；napi 已先接 Promise 异步 meshing（`meshBinaryAsync`，tokio blocking pool）；
10. ✅ **FieldGraph 节点回收**：`FieldGraph::compacted(roots)` 返回旧→新 id 映射，`Notebook::compact()` 从缓存输出收集可达根、重映射字段句柄并回收孤儿子图。
