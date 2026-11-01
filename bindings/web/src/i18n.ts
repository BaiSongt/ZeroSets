export type Language = "zh" | "en";

const LANGUAGE_STORAGE_KEY = "zeroset.language";
const LANGUAGE_EVENT = "zeroset:languagechange";

const en: Record<string, string> = {
  "app.title": "ZeroSet Notebook",
  "brand.notebook": "Notebook",
  "doc.notebook": "Notebook",
  "doc.mesh": "Mesh",
  "titlebar.session": "LOCAL SESSION",
  "action.language": "Switch language",
  "action.theme": "Switch light/dark theme",
  "theme.light": "Light",
  "theme.dark": "Dark",
  "ribbon.file": "File",
  "ribbon.edit": "Edit",
  "ribbon.build": "Build",
  "ribbon.view": "View",
  "action.new": "New",
  "action.template": "Template",
  "action.save": "Save",
  "action.load": "Load",
  "action.undo": "Undo",
  "action.redo": "Redo",
  "action.auto": "Auto",
  "action.features": "Features",
  "action.build": "Build Mesh",
  "action.fit": "Fit",
  "action.arrange": "Arrange",
  "action.library": "Library",
  "action.variables": "Variables",
  "action.inspector": "Inspector",
  "action.addBlock": "Add Block",
  "action.expandGraph": "Expand",
  "action.restoreGraph": "Restore",
  "panel.library": "Library",
  "panel.blocks": "Blocks",
  "panel.workspace": "Workspace",
  "panel.notebook": "Notebook",
  "panel.inspector": "Inspector",
  "panel.properties": "Properties",
  "panel.graph": "Graph",
  "panel.variables": "Variables",
  "variables.help": "One variable can drive many numeric inputs.",
  "variables.add": "+ Add",
  "variables.empty": "No named variables yet. Create one here or from a numeric input.",
  "variables.remove": "Delete variable",
  "variables.affected": "{count} affected Blocks",
  "variables.binding": "Variable binding",
  "variables.unbound": "— Local value —",
  "variables.createForParam": "+ Create from value",
  "variables.drivenBy": "Driven by {name}",
  "variables.invalidName": "Use letters, digits and underscores; do not start with a digit.",
  "variables.duplicateName": "Variable name already exists: {name}",
  "variables.invalidValue": "Variable value must be finite.",
  "variables.invalidSnapshot": "Invalid variables or bindings in notebook JSON.",
  "variables.formula.label": "Formula",
  "variables.formula.placeholder": "baseRadius + 2 mm",
  "variables.formula.computed": "Calculated by a formula; clear it to edit the value.",
  "variables.formula.syntax": "Formula syntax error. Check operators, parentheses and unit suffixes.",
  "variables.formula.unknown": "Unknown variable: {detail}.",
  "variables.formula.cycle": "Circular variable reference: {detail}.",
  "variables.formula.dimension": "Incompatible units or unsupported unit multiplication.",
  "variables.formula.divideByZero": "Formula divides by zero.",
  "variables.formula.overflow": "Formula result is outside the kernel's valid numeric range: {detail}.",
  "variables.formula.tooLong": "Formula is too long or nested too deeply.",
  "variables.formula.inUse": "Delete blocked; formula variables still depend on this variable: {names}.",
  "notebook.empty": "No blocks yet. Add one from the library.",
  "notebook.sources": "Inputs",
  "notebook.params": "Parameters",
  "notebook.openGraph": "View in Graph",
  "notebook.noSource": "No upstream blocks",
  "notebook.chooseSource": "— Unconnected —",
  "notebook.invalidSource": "invalid source",
  "notebook.addPort": "+ Add input",
  "palette.addConnect": "Add and connect {count} selected block(s)",
  "toast.connectCycle": "Connection rejected: circular dependency",
  "viewport.output": "Output",
  "viewport.outputAuto": "Auto",
  "viewport.outputNone": "No Mesh",
  "viewport.previewThis": "Preview this Mesh",
  "viewport.previewPinned": "Unpin preview",
  "search.blocks": "Search blocks",
  "panel.add": "Click to add to notebook",
  "panel.drag": "Drag to reorder",
  "viewport.perspective": "Perspective",
  "viewport.controls": "Orbit: drag · Pan: middle / right · Zoom: wheel",
  "status.autoUpdate": "Auto-update",
  "status.resizable": "Resizable panels",
  "status.wasm": "wasm {version}",
  "status.loading": "wasm loading…",
  "status.empty": "empty notebook · add blocks to build",
  "status.manual": "manual update · press Build Mesh",
  "status.dirty": "dirty…",
  "status.building": "building…",
  "status.noMesh": "add a Mesh block to build",
  "status.stats": "{verts} verts · {tris} tris · {ms} ms · wasm {version}",
  "status.initialize": "wasm {version} · {count} block definitions",
  "status.buildFailed": "build failed: {error}",
  "toast.buildFailed": "Build failed: {error}",
  "graph.empty": "Add a block from the left.",
  "properties.select": "Select a block to edit its parameters.",
  "properties.unknown": "Unknown block definition: {id}",
  "properties.name": "Name",
  "properties.overload": "Overload",
  "properties.preset": "Preset",
  "properties.customExpression": "— custom expression —",
  "properties.none": "— none —",
  "properties.drop": "drop",
  "properties.enabled": "Enabled",
  "properties.delete": "Delete block",
  "properties.addField": "＋ Add field input",
  "properties.addList": "＋ Add List input",
  "batch.selected": "{count} nodes selected",
  "batch.duplicate": "Duplicate",
  "batch.alignLeft": "Align left",
  "batch.alignTop": "Align top",
  "batch.distributeH": "Distribute H",
  "batch.distributeV": "Distribute V",
  "batch.enable": "Enable",
  "batch.disable": "Disable",
  "batch.delete": "Delete",
  "diagnostics.error": "error",
  "diagnostics.jump": "jump to source →",
  "diagnostics.line": "line {line}, column {column}",
  "palette.noMatch": "No blocks match this filter.",
  "graph.disabled": "disabled",
  "graph.addField": "＋ add field",
  "graph.connectHint": "Drag ports to connect · Right-click to add · Double-click a wire to remove",
  "graph.hintCompact": "Drag ports · Right-click to add · Middle-drag to pan",
  "graph.editProperties": "Edit selected node properties",
  "graph.quickAddTitle": "Add Block",
  "graph.quickSearch": "Search blocks…",
  "graph.quickHint": "Enter: add at cursor · Esc: close",
  "graph.quickEmpty": "No matching Blocks",
  "graph.disconnect": "Double-click / right-click to disconnect",
  "graph.rewire": "Drag to output to reconnect; drag to blank to disconnect",
  "graph.invalidDrop": "Cannot connect these ports",
  "ports.rename": "Rename port",
  "ports.renameAria": "Rename {name}",
  "ports.moveUp": "Move {name} up",
  "ports.moveDown": "Move {name} down",
  "ports.remove": "Remove {name}",
  "properties.selectedCount": "{count} selected",
  "properties.linkHint": "{type} input (link in a source)",
  "properties.dropHint": "Drop another block's output here",
  "diagnostics.input": "input: {name}",
  "toast.invalidPort": "Invalid port name: {name}",
  "toast.connectType": "Cannot connect {source} to {target}",
  "toast.saved": "Saved zeroset-notebook.json",
  "toast.portExists": "Port name already exists: {name}",
  "toast.loadFailed": "Load failed: {error}",
};

const zh: Record<string, string> = {
  "app.title": "ZeroSet 笔记本",
  "brand.notebook": "笔记本",
  "doc.notebook": "笔记本",
  "doc.mesh": "网格",
  "titlebar.session": "本地会话",
  "action.language": "切换语言",
  "action.theme": "切换明暗主题",
  "theme.light": "浅色",
  "theme.dark": "深色",
  "ribbon.file": "文件",
  "ribbon.edit": "编辑",
  "ribbon.build": "构建",
  "ribbon.view": "视图",
  "action.new": "新建",
  "action.template": "模板",
  "action.save": "保存",
  "action.load": "加载",
  "action.undo": "撤销",
  "action.redo": "重做",
  "action.auto": "自动",
  "action.features": "特征",
  "action.build": "构建网格",
  "action.fit": "适配",
  "action.arrange": "排列",
  "action.library": "库",
  "action.variables": "变量",
  "action.inspector": "属性",
  "action.addBlock": "添加 Block",
  "action.expandGraph": "展开",
  "action.restoreGraph": "还原",
  "panel.library": "库",
  "panel.blocks": "图块",
  "panel.workspace": "工作区",
  "panel.notebook": "笔记本",
  "panel.inspector": "检查器",
  "panel.properties": "属性",
  "panel.graph": "依赖图",
  "panel.variables": "命名变量",
  "variables.help": "一个变量可以驱动多个数值参数。",
  "variables.add": "+ 新建",
  "variables.empty": "还没有命名变量，可在这里或数值参数旁创建。",
  "variables.remove": "删除变量",
  "variables.affected": "影响 {count} 个 Block",
  "variables.binding": "变量绑定",
  "variables.unbound": "— 本地数值 —",
  "variables.createForParam": "+ 从当前参数创建",
  "variables.drivenBy": "由 {name} 驱动",
  "variables.invalidName": "名称只能使用字母、数字和下划线，不能以数字开头。",
  "variables.duplicateName": "变量名称已存在：{name}",
  "variables.invalidValue": "变量值必须为有限数字。",
  "variables.invalidSnapshot": "Notebook JSON 中的变量或绑定不合法。",
  "variables.formula.label": "变量公式",
  "variables.formula.placeholder": "baseRadius + 2 mm",
  "variables.formula.computed": "当前值由公式计算；清空公式后可以直接修改数值。",
  "variables.formula.syntax": "公式语法错误，请检查运算符、括号和单位后缀。",
  "variables.formula.unknown": "公式引用了不存在的变量：{detail}。",
  "variables.formula.cycle": "公式存在循环依赖：{detail}。",
  "variables.formula.dimension": "公式中的单位不匹配，或暂不支持单位相乘。",
  "variables.formula.divideByZero": "公式中存在除零。",
  "variables.formula.overflow": "公式结果超出内核的数值范围：{detail}。",
  "variables.formula.tooLong": "公式过长或嵌套层数过多。",
  "variables.formula.inUse": "无法删除：其他公式变量仍依赖此变量：{names}。",
  "notebook.empty": "当前没有 Block。可以从 Block 库添加建模步骤。",
  "notebook.sources": "上游输入",
  "notebook.params": "参数",
  "notebook.openGraph": "在 Graph 查看",
  "notebook.noSource": "无上游连接",
  "notebook.chooseSource": "— 未连接 —",
  "notebook.invalidSource": "无效来源",
  "notebook.addPort": "+ 添加输入端口",
  "palette.addConnect": "添加并自动连接 {count} 个选中 Block",
  "toast.connectCycle": "无法连接：这会形成循环依赖",
  "viewport.output": "输出",
  "viewport.outputAuto": "自动",
  "viewport.outputNone": "无 Mesh",
  "viewport.previewThis": "预览此网格",
  "viewport.previewPinned": "取消固定预览",
  "search.blocks": "搜索图块",
  "panel.add": "点击添加到笔记本",
  "panel.drag": "拖拽调整顺序",
  "viewport.perspective": "透视",
  "viewport.controls": "旋转：拖动 · 平移：中键 / 右键 · 缩放：滚轮",
  "status.autoUpdate": "自动更新",
  "status.resizable": "可调整面板",
  "status.wasm": "wasm {version}",
  "status.loading": "wasm 加载中…",
  "status.empty": "空笔记本 · 添加图块以构建",
  "status.manual": "手动更新 · 点击“构建网格”",
  "status.dirty": "待更新…",
  "status.building": "构建中…",
  "status.noMesh": "添加 Mesh 图块以构建",
  "status.stats": "{verts} 顶点 · {tris} 三角形 · {ms} ms · wasm {version}",
  "status.initialize": "wasm {version} · {count} 个图块定义",
  "status.buildFailed": "构建失败：{error}",
  "toast.buildFailed": "构建失败：{error}",
  "graph.empty": "从左侧添加图块。",
  "properties.select": "选择一个图块以编辑参数。",
  "properties.unknown": "未知图块定义：{id}",
  "properties.name": "名称",
  "properties.overload": "重载",
  "properties.preset": "预设",
  "properties.customExpression": "— 自定义表达式 —",
  "properties.none": "— 无 —",
  "properties.drop": "拖入",
  "properties.enabled": "启用",
  "properties.delete": "删除图块",
  "properties.addField": "＋ 添加字段输入",
  "properties.addList": "＋ 添加 List 输入",
  "batch.selected": "已选 {count} 个节点",
  "batch.duplicate": "复制",
  "batch.alignLeft": "左对齐",
  "batch.alignTop": "顶部对齐",
  "batch.distributeH": "水平分布",
  "batch.distributeV": "垂直分布",
  "batch.enable": "启用",
  "batch.disable": "禁用",
  "batch.delete": "删除",
  "diagnostics.error": "错误",
  "diagnostics.jump": "跳转到源 →",
  "diagnostics.line": "第 {line} 行，第 {column} 列",
  "palette.noMatch": "没有匹配的图块。",
  "graph.disabled": "已禁用",
  "graph.addField": "＋ 添加字段",
  "graph.connectHint": "拖动端口连接 · 右键新建节点 · 双击连线删除",
  "graph.hintCompact": "拖动端口连线 · 右键新建 · 中键平移",
  "graph.editProperties": "编辑所选节点属性",
  "graph.quickAddTitle": "新建 Block",
  "graph.quickSearch": "搜索 Block…",
  "graph.quickHint": "Enter：在光标处新建 · Esc：关闭",
  "graph.quickEmpty": "没有匹配的 Block",
  "graph.disconnect": "双击或右键可断开连接",
  "graph.rewire": "拖至输出端口重新连接，拖至空白处断开",
  "graph.invalidDrop": "无法连接这些端口",
  "ports.rename": "重命名端口",
  "ports.renameAria": "重命名 {name}",
  "ports.moveUp": "上移 {name}",
  "ports.moveDown": "下移 {name}",
  "ports.remove": "移除 {name}",
  "properties.selectedCount": "已选 {count} 个",
  "properties.linkHint": "{type} 输入（从源连线）",
  "properties.dropHint": "把另一个图块的输出拖到这里",
  "diagnostics.input": "输入：{name}",
  "toast.invalidPort": "无效端口名：{name}",
  "toast.connectType": "无法把 {source} 连接到 {target}",
  "toast.saved": "已保存 zeroset-notebook.json",
  "toast.portExists": "端口名已存在：{name}",
  "toast.loadFailed": "加载失败：{error}",
};

const dictionaries: Record<Language, Record<string, string>> = { en, zh };

const fieldZh: Record<string, string> = {
  expression: "表达式",
  scale: "缩放",
  offset: "偏移",
  translation: "平移",
  flag: "开关",
  field: "字段",
  mesh: "网格",
  resolution: "分辨率",
  crease: "折痕角",
  simplify: "简化比例",
  simplify_angle: "简化角",
  feature_relax: "特征松弛",
  feature_angle: "特征角",
  cap: "封口",
  amount: "数量",
  angle: "角度",
  radius: "半径",
  center: "中心",
  half: "半尺寸",
  rounding: "圆角",
};

const categoryZh: Record<string, string> = {
  Primitive: "基础",
  primitive: "基础",
  Boolean: "布尔",
  boolean: "布尔",
  Transform: "变换",
  transform: "变换",
  Utility: "工具",
  utility: "工具",
  Mesh: "网格",
  mesh: "网格",
  Input: "输入",
  input: "输入",
};

const descriptionZh: Record<string, string> = {
  sphere: "解析球体；重载可指定半径或直径。",
  box: "由半尺寸定义的圆角盒。",
  cylinder: "带任意轴线的封口圆柱。",
  capsule: "两端点之间的胶囊。",
  torus: "绕任意法线的圆环。",
  union: "硬并集；接受两个或三个字段。",
  list: "收集可选字段输入为列表，供列表感知图块使用。",
  intersect: "两个字段的硬交集。",
  subtract: "硬差集：a 减 b。",
  smooth_union: "多项式平滑并集。",
  smooth_intersect: "多项式平滑交集。",
  smooth_subtract: "多项式平滑差集。",
  offset: "正距离膨胀实体，负距离侵蚀。",
  transform: "平移、轴角旋转与等比缩放。",
  custom:
    "编译小型字段表达式；可选字段绑定为 $a-$d / $name，数值绑定为 $scale/$offset，向量 $translation，布尔 $flag。",
  mesh: "在密集网格分辨率上做 Surface Nets 或 Dual Contouring 提取。",
  number: "可连接进 Number 输入的类型化数值字面量。",
  vector: "可连接进 Vector 输入的类型化向量字面量。",
};

const defaultNameIds: Record<string, string> = {
  Mesh: "mesh",
  "Mesh from Field": "mesh",
  "Field List": "list",
  "Custom Field": "custom",
  Number: "number",
  Vector: "vector",
};

const blockZh: Record<string, string> = {
  sphere: "球",
  box: "盒",
  cylinder: "圆柱",
  capsule: "胶囊",
  torus: "圆环",
  union: "合并",
  intersect: "相交",
  subtract: "减去",
  smooth_union: "平滑合并",
  smooth_intersect: "平滑相交",
  smooth_subtract: "平滑减去",
  offset: "偏移",
  transform: "变换",
  list: "字段列表",
  custom: "自定义字段",
  mesh: "生成网格",
  number: "数值",
  vector: "向量",
};

const templateZh: Record<string, string> = {
  Sphere: "球",
  "Scaled sphere": "缩放球",
  "Offset shell": "偏移壳",
  "Translated sphere": "平移球",
  "Smooth union": "平滑合并",
  Subtract: "减去",
  Intersection: "相交",
  Torus: "圆环",
  "Rotated box": "旋转盒",
  "Boolean switch": "布尔切换",
};

const unitZh: Record<string, string> = {
  unitless: "无单位",
  millimeter: "毫米",
  centimeter: "厘米",
  meter: "米",
  inch: "英寸",
  radian: "弧度",
  degree: "度",
};

let current: Language = "en";

function detectLanguage(): Language {
  try {
    const saved = localStorage.getItem(LANGUAGE_STORAGE_KEY);
    if (saved === "zh" || saved === "en") return saved;
  } catch {
    // localStorage can be unavailable in privacy-restricted contexts.
  }
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

export function currentLanguage(): Language {
  return current;
}

export function languageLabel(): string {
  return current === "zh" ? "EN" : "中文";
}

export function t(key: string, vars?: Record<string, string | number>): string {
  const template = dictionaries[current][key] ?? en[key] ?? key;
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in vars ? String(vars[name]) : match,
  );
}

export function fieldLabel(name: string): string {
  return current === "zh" ? (fieldZh[name] ?? name) : name;
}

export function categoryLabel(name: string): string {
  if (current !== "zh") return name;
  return categoryZh[name] ?? categoryZh[name.charAt(0).toUpperCase() + name.slice(1)] ?? name;
}

export function blockDescription(id: string, fallback: string): string {
  return current === "zh" ? (descriptionZh[id] ?? fallback) : fallback;
}

export function defaultBlockId(name: string): string | undefined {
  return defaultNameIds[name];
}

export function blockLabel(id: string, fallback: string): string {
  return current === "zh" ? (blockZh[id] ?? fallback) : fallback;
}

export function templateLabel(name: string): string {
  return current === "zh" ? (templateZh[name] ?? name) : name;
}

export function unitLabel(unit: string): string {
  return current === "zh" ? (unitZh[unit] ?? unit) : unit;
}

export function applyTranslations(root: ParentNode = document): void {
  document.documentElement.lang = current === "zh" ? "zh-CN" : "en";
  document.title = t("app.title");
  for (const element of root.querySelectorAll<HTMLElement>("[data-i18n]")) {
    const key = element.dataset.i18n;
    if (key) element.textContent = t(key);
  }
  for (const element of root.querySelectorAll<HTMLElement>("[data-i18n-title]")) {
    const key = element.dataset.i18nTitle;
    if (key) element.title = t(key);
  }
  for (const element of root.querySelectorAll<HTMLElement>("[data-i18n-placeholder]")) {
    const key = element.dataset.i18nPlaceholder;
    if (key) element.setAttribute("placeholder", t(key));
  }
}

export function setLanguage(language: Language, persist = true): Language {
  current = language;
  if (persist) {
    try {
      localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
    } catch {
      // Persisting the preference is best-effort.
    }
  }
  applyTranslations();
  window.dispatchEvent(new CustomEvent<Language>(LANGUAGE_EVENT, { detail: language }));
  return language;
}

export function initLanguage(): Language {
  current = detectLanguage();
  applyTranslations();
  return current;
}

export function toggleLanguage(): Language {
  return setLanguage(current === "zh" ? "en" : "zh");
}

export function onLanguageChange(listener: (language: Language) => void): () => void {
  const handler = (event: Event): void => {
    listener((event as CustomEvent<Language>).detail);
  };
  window.addEventListener(LANGUAGE_EVENT, handler);
  return () => window.removeEventListener(LANGUAGE_EVENT, handler);
}
