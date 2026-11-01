import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import wasmUrl from "../pkg/zeroset_wasm_bg.wasm?url";
import init, {
  block_catalog,
  custom_templates,
  notebook_diagnostics,
  notebook_eval,
  notebook_template,
  version,
} from "../pkg/zeroset_wasm.js";
import {
  applyTranslations,
  blockDescription,
  blockLabel,
  categoryLabel,
  currentLanguage,
  defaultBlockId,
  fieldLabel,
  initLanguage,
  languageLabel as languageToggleLabel,
  onLanguageChange,
  t,
  templateLabel,
  toggleLanguage,
  unitLabel,
} from "./i18n";
import {
  currentTheme,
  initTheme,
  onThemeChange,
  toggleTheme,
  type Theme,
} from "./theme";
import { acceptsOutput, createsCycle, planAutoConnections } from "./workflow";
import { bezierWire, exportGraphLayout, graphPointAt, importGraphLayout, planGraphConnection, type GraphPoint, type GraphPort } from "./graph-editor";
import {
  dependentVariableIds,
  FormulaError,
  renameFormulaReference,
  resolveVariableExpressions,
  type VariableResolution,
} from "./expressions";
import {
  affectedBlockIds,
  convertQuantity,
  materializeBoundNumbers,
  unitFamily,
  validVariableName,
  type NamedVariable,
} from "./variables";

type ValueType = "field" | "mesh" | "number" | "vector" | "boolean" | "text" | "list" | "any";
type UnitKind = "any" | "length" | "angle" | "unitless";
type Unit =
  | "unitless"
  | "millimeter"
  | "centimeter"
  | "meter"
  | "inch"
  | "radian"
  | "degree";

type NumberParam = { Number: { value: number; unit: Unit } };
type VectorParam = { Vector: [[number, number, number], Unit] };
type BooleanParam = { Boolean: boolean };
type TextParam = { Text: string };
type ParamValue = NumberParam | VectorParam | BooleanParam | TextParam;

type InputDef = {
  name: string;
  value_type: ValueType;
  required: boolean;
  unit: UnitKind;
  default: ParamValue | null;
};

type Overload = { name: string; inputs: InputDef[]; output: ValueType };

type BlockDef = {
  id: string;
  name: string;
  category: string;
  description: string;
  overloads: Overload[];
};

type CustomTemplate = {
  name: string;
  expression: string;
  params?: [string, ParamValue][];
};

type Instance = {
  id: number;
  def_id: string;
  name: string;
  overload: string | null;
  params: Record<string, ParamValue>;
  inputs: Record<string, number>;
  ports?: string[];
  variable_refs?: Record<string, number>;
  enabled: boolean;
  manual: boolean;
};

type Snapshot = {
  version: number;
  next_id: number;
  update_policy?: "auto" | "manual";
  variables?: NamedVariable[];
  next_variable_id?: number;
  graph_layout?: Record<string, GraphPoint>;
  instances: Instance[];
};

type DiagnosticEntry = {
  severity: string;
  message: string;
  input: string | null;
  line: number | null;
  column: number | null;
  snippet: string | null;
  source?: number | null;
};

const viewport = document.querySelector<HTMLElement>("#viewport");
const previewTargetSelect = document.querySelector<HTMLSelectElement>("#preview-target");
const palette = document.querySelector<HTMLElement>("#palette");
const paletteList = document.querySelector<HTMLElement>("#palette-list");
const paletteCount = document.querySelector<HTMLElement>("#palette-count");
const blockSearch = document.querySelector<HTMLInputElement>("#block-search");
const togglePaletteButton = document.querySelector<HTMLButtonElement>("#toggle-palette");
const toggleVariablesButton = document.querySelector<HTMLButtonElement>("#toggle-variables");
const variablesPanel = document.querySelector<HTMLElement>("#variables-panel");
const variablesList = document.querySelector<HTMLElement>("#variables-list");
const variablesCount = document.querySelector<HTMLElement>("#variables-count");
const variableAddButton = document.querySelector<HTMLButtonElement>("#variable-add");
const togglePropertiesButton = document.querySelector<HTMLButtonElement>("#toggle-properties");
const properties = document.querySelector<HTMLElement>("#properties");
const propertiesBadge = document.querySelector<HTMLElement>("#properties-badge");
const notebook = document.querySelector<HTMLElement>("#notebook");
const notebookBody = document.querySelector<HTMLElement>("#notebook-body");
const notebookList = document.querySelector<HTMLElement>("#notebook-list");
const notebookCount = document.querySelector<HTMLElement>("#notebook-count");
const workspaceTitle = document.querySelector<HTMLElement>("#workspace-title");
const showNotebookButton = document.querySelector<HTMLButtonElement>("#show-notebook");
const showGraphButton = document.querySelector<HTMLButtonElement>("#show-graph");
const graphExpandButton = document.querySelector<HTMLButtonElement>("#graph-expand");
const graphExpandLabel = document.querySelector<HTMLElement>("#graph-expand-label");
const notebookAddButton = document.querySelector<HTMLButtonElement>("#notebook-add");
const graphPropertiesButton = document.querySelector<HTMLButtonElement>("#graph-properties-toggle");
const graphQuickAdd = document.querySelector<HTMLElement>("#graph-quick-add");
const graphQuickSearch = document.querySelector<HTMLInputElement>("#graph-quick-search");
const graphQuickResults = document.querySelector<HTMLElement>("#graph-quick-results");
const graphWorld = document.querySelector<HTMLElement>("#graph-world");
const wires = document.querySelector<SVGSVGElement>("#wires");
const marquee = document.querySelector<HTMLElement>("#marquee");
const zoomLabel = document.querySelector<HTMLElement>("#zoom-label");
const zoomIn = document.querySelector<HTMLButtonElement>("#zoom-in");
const zoomOut = document.querySelector<HTMLButtonElement>("#zoom-out");
const fitButton = document.querySelector<HTMLButtonElement>("#fit");
const arrangeButton = document.querySelector<HTMLButtonElement>("#arrange");
const instanceList = document.querySelector<HTMLElement>("#instance-list");
const propertyBody = document.querySelector<HTMLElement>("#property-body");
const statusLabel = document.querySelector<HTMLElement>("#status");
const buildButton = document.querySelector<HTMLButtonElement>("#build");
const undoButton = document.querySelector<HTMLButtonElement>("#undo");
const redoButton = document.querySelector<HTMLButtonElement>("#redo");
const templateButton = document.querySelector<HTMLButtonElement>("#template");
const newButton = document.querySelector<HTMLButtonElement>("#new");
const saveButton = document.querySelector<HTMLButtonElement>("#save");
const loadButton = document.querySelector<HTMLButtonElement>("#load-button");
const loadFile = document.querySelector<HTMLInputElement>("#load-file");
const autoToggle = document.querySelector<HTMLInputElement>("#auto");
const featureToggle = document.querySelector<HTMLInputElement>("#show-features");
const toast = document.querySelector<HTMLElement>("#toast");
const diagnostics = document.querySelector<HTMLElement>("#diagnostics");
const languageButton = document.querySelector<HTMLButtonElement>("#toggle-language");
const languageLabel = document.querySelector<HTMLElement>("#language-label");
const themeButton = document.querySelector<HTMLButtonElement>("#toggle-theme");
const themeLabel = document.querySelector<HTMLElement>("#theme-label");

if (
  !viewport ||
  !previewTargetSelect ||
  !toggleVariablesButton ||
  !variablesPanel ||
  !variablesList ||
  !variablesCount ||
  !variableAddButton ||
  !paletteList ||
  !notebook ||
  !notebookBody ||
  !notebookList ||
  !notebookCount ||
  !workspaceTitle ||
  !showNotebookButton ||
  !showGraphButton ||
  !graphExpandButton ||
  !graphExpandLabel ||
  !notebookAddButton ||
  !graphPropertiesButton ||
  !graphQuickAdd ||
  !graphQuickSearch ||
  !graphQuickResults ||
  !graphWorld ||
  !wires ||
  !marquee ||
  !zoomLabel ||
  !zoomIn ||
  !zoomOut ||
  !fitButton ||
  !arrangeButton ||
  !instanceList ||
  !propertyBody ||
  !statusLabel ||
  !buildButton ||
  !undoButton ||
  !redoButton ||
  !templateButton ||
  !newButton ||
  !saveButton ||
  !loadButton ||
  !loadFile ||
  !autoToggle ||
  !featureToggle ||
  !toast ||
  !diagnostics
) {
  throw new Error("Notebook UI markup is incomplete");
}

if (!languageButton || !languageLabel || !themeButton || !themeLabel) {
  throw new Error("Language/theme controls are missing");
}

let catalog: BlockDef[] = [];
let customTemplates: CustomTemplate[] = [];
let snapshot: Snapshot = { version: 1, next_id: 1, instances: [], update_policy: "auto" };
let selectedId: number | null = null;
let selectedIds = new Set<number>();
let previewTargetId: number | null = null;
let impactedVariableId: number | null = null;
let impactedIds = new Set<number>();
let formulaDependencies = new Map<number, Set<number>>();
let errorId: number | null = null;
let autoUpdate = true;
let suppressClickUntil = 0;
type WorkspaceMode = "notebook" | "graph";
const WORKSPACE_MODE_KEY = "zeroset.workspace-mode";
let workspaceMode: WorkspaceMode = "notebook";
let graphExpanded = false;
let graphPropertiesOpen = false;
let graphAddPosition: GraphPoint | null = null;
let graphAddSourceId: number | null = null;
let activeGraphWire: { from: GraphPort; element: HTMLElement; pointer: GraphPoint } | null = null;
let cancelGraphWireDrag: (() => void) | null = null;
let zoom = 1;
let panX = 0;
let panY = 0;
let panStart: { x: number; y: number; panX: number; panY: number } | null = null;
const nodePositions = new Map<number, { x: number; y: number }>();

type HistoryEntry = {
  snapshot: Snapshot;
  positions: Array<[number, { x: number; y: number }]>;
  selectedId: number | null;
  selectedIds: number[];
};

const history: HistoryEntry[] = [];
let historyIndex = -1;
const HISTORY_LIMIT = 80;
let buildTimer: number | undefined;
let notebookHistoryTimer: number | undefined;
let buildToken = 0;
let framed = false;

const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.setClearColor(0x090d14);
viewport.appendChild(renderer.domElement);

const scene = new THREE.Scene();
scene.fog = new THREE.Fog(0x090d14, 140, 320);

const camera = new THREE.PerspectiveCamera(42, 1, 0.1, 1000);
camera.position.set(58, 42, 62);

const controls = new OrbitControls(camera, renderer.domElement);
controls.enableDamping = true;
controls.dampingFactor = 0.08;
controls.target.set(0, 0, 0);

const hemiLight = new THREE.HemisphereLight(0xcfe0ff, 0x1a2230, 1.15);
scene.add(hemiLight);
const keyLight = new THREE.DirectionalLight(0xffffff, 2.3);
keyLight.position.set(40, 80, 30);
scene.add(keyLight);
const rimLight = new THREE.DirectionalLight(0x6ea8fe, 0.9);
rimLight.position.set(-50, -20, -40);
scene.add(rimLight);

let grid = new THREE.GridHelper(160, 32, 0x243044, 0x161d2a);
grid.position.y = -34;
scene.add(grid);

const material = new THREE.MeshStandardMaterial({
  color: 0x8fb7ff,
  metalness: 0.16,
  roughness: 0.4,
});

let currentMesh: THREE.Mesh | null = null;
let currentGeometry: THREE.BufferGeometry | null = null;
let currentFeatures: THREE.LineSegments | null = null;
const featureMaterial = new THREE.LineBasicMaterial({
  color: 0xffb86b,
  transparent: true,
  opacity: 0.9,
});

function applyViewportTheme(theme: Theme): void {
  const light = theme === "light";
  const clear = light ? 0xf3f6f9 : 0x090d14;
  const gridCenter = light ? 0xc2cedb : 0x243044;
  const gridLine = light ? 0xdbe3ea : 0x161d2a;
  scene.background = new THREE.Color(clear);
  scene.fog = new THREE.Fog(clear, 140, 320);
  renderer.setClearColor(clear);
  hemiLight.color.setHex(light ? 0xffffff : 0xcfe0ff);
  hemiLight.groundColor.setHex(light ? 0xd5dde7 : 0x1a2230);
  hemiLight.intensity = light ? 1.05 : 1.15;
  keyLight.intensity = light ? 1.8 : 2.3;
  rimLight.color.setHex(light ? 0x8facd8 : 0x6ea8fe);
  material.color.setHex(light ? 0x7aa8e8 : 0x8fb7ff);
  featureMaterial.color.setHex(light ? 0xd27a2a : 0xffb86b);
  scene.remove(grid);
  grid.geometry.dispose();
  (grid.material as THREE.Material).dispose();
  grid = new THREE.GridHelper(160, 32, gridCenter, gridLine);
  grid.position.y = -34;
  scene.add(grid);
  renderer.render(scene, camera);
}

applyViewportTheme(currentTheme());

function resize(): void {
  const width = Math.max(1, viewport!.clientWidth);
  const height = Math.max(1, viewport!.clientHeight);
  renderer.setSize(width, height, false);
  camera.aspect = width / height;
  camera.updateProjectionMatrix();
}

function animate(): void {
  requestAnimationFrame(animate);
  controls.update();
  renderer.render(scene, camera);
}

function showToast(message: string): void {
  toast!.textContent = message;
  toast!.classList.remove("hidden");
  window.setTimeout(() => toast!.classList.add("hidden"), 4200);
}

function hideToast(): void {
  toast!.classList.add("hidden");
}

type PanelKind = "palette" | "graph" | "properties";

const PANEL_LAYOUT_KEY = "zeroset.panel-layout";
const PANEL_VARS: Record<PanelKind, string> = {
  palette: "--palette-w",
  graph: "--graph-w",
  properties: "--properties-w",
};
let layoutFrame = 0;

function panelElement(kind: PanelKind): HTMLElement | null {
  if (kind === "palette") return palette;
  if (kind === "graph") return notebook;
  return properties;
}

function setPanelWidth(kind: PanelKind, width: number): void {
  document.documentElement.style.setProperty(PANEL_VARS[kind], `${Math.round(width)}px`);
}

function refreshLayout(): void {
  if (layoutFrame) return;
  layoutFrame = window.requestAnimationFrame(() => {
    layoutFrame = 0;
    clampPanelLayout();
    resize();
    updateCanvasSize();
    scheduleWires();
  });
}

function clampPanelLayout(): void {
  if (graphExpanded) return;
  const width = document.documentElement.clientWidth;
  const propertiesWidth = properties?.getBoundingClientRect().width ?? 0;
  const graphWidth = notebook?.getBoundingClientRect().width ?? 0;
  if (graphWidth <= 0) return;
  // Block 库是浮层，不应占用分栏可用宽度。
  const maxGraph = Math.max(260, width - propertiesWidth - 8 - 280);
  if (graphWidth > maxGraph) {
    setPanelWidth("graph", maxGraph);
  }
}

function restorePanelLayout(): void {
  try {
    const raw = localStorage.getItem(PANEL_LAYOUT_KEY);
    if (!raw) return;
    const saved = JSON.parse(raw) as Partial<Record<PanelKind, number>>;
    for (const kind of Object.keys(PANEL_VARS) as PanelKind[]) {
      const width = saved[kind];
      if (typeof width === "number" && Number.isFinite(width) && width > 0) {
        setPanelWidth(kind, width);
      }
    }
    refreshLayout();
  } catch (error) {
    console.warn("panel layout could not be restored", error);
  }
}

function savePanelLayout(): void {
  const saved: Partial<Record<PanelKind, number>> = {};
  for (const kind of Object.keys(PANEL_VARS) as PanelKind[]) {
    const width = panelElement(kind)?.getBoundingClientRect().width ?? 0;
    if (width > 0) saved[kind] = Math.round(width);
  }
  try {
    localStorage.setItem(PANEL_LAYOUT_KEY, JSON.stringify(saved));
  } catch (error) {
    console.warn("panel layout could not be saved", error);
  }
}

function bindPanelResizers(): void {
  for (const resizer of document.querySelectorAll<HTMLElement>(".resizer[data-resize]")) {
    const kind = resizer.dataset.resize as PanelKind;
    if (!PANEL_VARS[kind]) continue;
    resizer.addEventListener("pointerdown", (event) => {
      if (event.button !== 0) return;
      const target = panelElement(kind);
      if (!target) return;
      event.preventDefault();
      const startX = event.clientX;
      const startWidth = target.getBoundingClientRect().width;
      resizer.classList.add("active");
      document.body.classList.add("resizing");
      const limits =
        kind === "palette"
          ? { min: 180, max: 360 }
          : kind === "graph"
            ? { min: 300, max: 720 }
            : { min: 250, max: 520 };
      const move = (moveEvent: PointerEvent) => {
        const delta = moveEvent.clientX - startX;
        const direction = kind === "properties" ? -1 : 1;
        const width = Math.max(
          limits.min,
          Math.min(limits.max, startWidth + delta * direction),
        );
        setPanelWidth(kind, width);
        refreshLayout();
      };
      const finish = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", finish);
        window.removeEventListener("pointercancel", finish);
        resizer.classList.remove("active");
        document.body.classList.remove("resizing");
        savePanelLayout();
        refreshLayout();
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", finish);
      window.addEventListener("pointercancel", finish);
    });
    resizer.addEventListener("dblclick", () => {
      document.documentElement.style.removeProperty(PANEL_VARS[kind]);
      savePanelLayout();
      refreshLayout();
    });
  }
}

let statusState: { key: string; vars?: Record<string, string | number> } | null = null;

function setStatus(message: string): void {
  statusState = null;
  statusLabel!.textContent = message;
}

function setLocalizedStatus(
  key: string,
  vars?: Record<string, string | number>,
): void {
  statusState = { key, vars };
  statusLabel!.textContent = t(key, vars);
}

function syncLanguageThemeControls(): void {
  languageLabel!.textContent = languageToggleLabel();
  languageButton!.title = t("action.language");
  const light = currentTheme() === "light";
  themeLabel!.textContent = light ? t("theme.dark") : t("theme.light");
  themeButton!.title = t("action.theme");
}

function diagnosticCaret(entry: DiagnosticEntry): string | null {
  if (entry.line === null || entry.column === null || entry.snippet === null) return null;
  const bar = entry.snippet.indexOf("|");
  const prefix = bar >= 0 ? bar + 2 : `${entry.line} | `.length;
  return `${entry.snippet}\n${" ".repeat(prefix + Math.max(0, entry.column - 1))}^`;
}

function readDiagnostics(instanceId: number): DiagnosticEntry[] {
  try {
    const json = notebook_diagnostics(kernelSnapshotJson(), BigInt(instanceId));
    const entries = JSON.parse(json) as DiagnosticEntry[];
    return entries.filter((entry) => entry.severity === "error");
  } catch (error) {
    console.warn("structured diagnostics unavailable", error);
    return [];
  }
}

function diagnosticOffset(value: string, line: number, column: number): number {
  const lines = value.split("\n");
  const target = Math.max(1, Math.min(line, lines.length));
  let offset = 0;
  for (let index = 0; index < target - 1; index += 1) {
    offset += (lines[index]?.length ?? 0) + 1;
  }
  return Math.min(value.length, offset + Math.max(0, column - 1));
}

function revealDiagnostic(entry: DiagnosticEntry): void {
  if (entry.source === null || entry.source === undefined) return;
  if (!instanceById(entry.source)) return;
  selectedId = entry.source;
  selectedIds = new Set([entry.source]);
  renderInstances();
  renderProperties();
  const textarea = propertyBody!.querySelector<HTMLTextAreaElement>("textarea.text-area");
  if (!textarea) return;
  textarea.focus();
  textarea.scrollIntoView({ block: "nearest" });
  if (entry.line !== null && entry.column !== null) {
    const offset = diagnosticOffset(textarea.value, entry.line, entry.column);
    textarea.setSelectionRange(offset, offset);
  }
}

function setDiagnostics(message: string | null, entries: DiagnosticEntry[] = []): void {
  if (!message) {
    diagnostics!.replaceChildren();
    diagnostics!.classList.add("hidden");
    return;
  }
  diagnostics!.replaceChildren();
  if (entries.length === 0) {
    diagnostics!.textContent = message;
  } else {
    for (const entry of entries) {
      const item = document.createElement("div");
      item.className = "diagnostic-entry";

      const head = document.createElement("div");
      head.className = "diagnostic-head";
      const badge = document.createElement("span");
      badge.className = `diagnostic-badge ${entry.severity}`;
      badge.textContent = entry.severity === "error" ? t("diagnostics.error") : entry.severity;
      head.append(badge);
      if (entry.line !== null && entry.column !== null) {
        const location = document.createElement("span");
        location.className = "diagnostic-location";
        location.textContent = t("diagnostics.line", { line: entry.line, column: entry.column });
        head.append(location);
      }
      if (entry.source !== null && entry.source !== undefined) {
        const jump = document.createElement("span");
        jump.className = "diagnostic-jump";
        jump.textContent = t("diagnostics.jump");
        head.append(jump);
      }
      item.append(head);

      const summary = document.createElement("div");
      summary.className = "diagnostic-message";
      summary.textContent = entry.message
        .split("\n")[0]
        .replace(/\s+at line \d+, column \d+$/, "");
      item.append(summary);

      if (entry.input) {
        const input = document.createElement("div");
        input.className = "diagnostic-input";
        input.textContent = t("diagnostics.input", { name: entry.input });
        item.append(input);
      }

      const caret = diagnosticCaret(entry);
      if (caret) {
        const snippet = document.createElement("pre");
        snippet.className = "diagnostic-snippet";
        snippet.textContent = caret;
        item.append(snippet);
      }
      if (entry.source !== null && entry.source !== undefined) {
        item.classList.add("diagnostic-clickable");
        item.addEventListener("click", () => revealDiagnostic(entry));
      }
      diagnostics!.append(item);
    }
  }
  diagnostics!.classList.remove("hidden");
}

function blockDef(id: string): BlockDef | undefined {
  return catalog.find((def) => def.id === id);
}

function instanceById(id: number): Instance | undefined {
  return snapshot.instances.find((instance) => instance.id === id);
}

function selectedInstance(): Instance | undefined {
  return selectedId === null ? undefined : instanceById(selectedId);
}

function activeOverload(instance: Instance): Overload | undefined {
  const def = blockDef(instance.def_id);
  if (!def) return undefined;
  return instance.overload
    ? def.overloads.find((overload) => overload.name === instance.overload)
    : def.overloads[0];
}

function outputType(instance: Instance): ValueType | undefined {
  return activeOverload(instance)?.output;
}

function isFreePortBlock(instance: Instance): boolean {
  return instance.def_id === "list" || instance.def_id === "custom";
}

/**
 * List and Custom Field blocks accept free-form field ports. `inputs` stores
 * connected ports, `ports` keeps empty ones so a port can be added before
 * wiring it. Other blocks keep their fixed overload ports.
 */
function freePortNames(instance: Instance): string[] {
  const overload = activeOverload(instance);
  if (!overload || !isFreePortBlock(instance)) return [];
  const fixed = new Set(overload.inputs.map((input) => input.name));
  const names: string[] = [];
  const seen = new Set<string>();
  const push = (name: string): void => {
    if (fixed.has(name) || seen.has(name)) return;
    seen.add(name);
    names.push(name);
  };
  for (const name of instance.ports ?? []) push(name);
  for (const name of Object.keys(instance.inputs)) push(name);
  return names;
}

function inputsFor(instance: Instance): InputDef[] {
  const overload = activeOverload(instance);
  if (!overload) return [];
  if (!isFreePortBlock(instance)) return overload.inputs;
  const extras = freePortNames(instance).map((name) => ({
    name,
    value_type: "field" as const,
    required: false,
    unit: "any" as const,
    default: null,
  }));
  return instance.def_id === "list" ? extras : [...overload.inputs, ...extras];
}

function nextFreePortName(instance: Instance): string {
  const used = new Set(inputsFor(instance).map((input) => input.name));
  let index = 1;
  while (used.has(`in${index}`)) index += 1;
  return `in${index}`;
}

function renameFreePort(instance: Instance, oldName: string, rawName: string): boolean {
  const newName = rawName.trim();
  if (newName === oldName) return true;
  const overload = activeOverload(instance);
  if (!overload) return false;
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(newName)) {
    showToast(t("toast.invalidPort", { name: newName || "<empty>" }));
    return false;
  }
  if (
    overload.inputs.some((input) => input.name === newName) ||
    freePortNames(instance).some((name) => name !== oldName && name === newName)
  ) {
    showToast(t("toast.portExists", { name: newName }));
    return false;
  }
  const order = freePortNames(instance);
  const source = instance.inputs[oldName];
  delete instance.inputs[oldName];
  if (source !== undefined) instance.inputs[newName] = source;
  instance.ports = order.map((name) => (name === oldName ? newName : name));
  commitHistory();
  renderAll();
  scheduleWires();
  scheduleBuild();
  return true;
}

function moveFreePort(instance: Instance, name: string, delta: number): void {
  const order = freePortNames(instance);
  const index = order.indexOf(name);
  const target = index + delta;
  if (index < 0 || target < 0 || target >= order.length) return;
  [order[index], order[target]] = [order[target]!, order[index]!];
  instance.ports = order;
  commitHistory();
  renderAll();
  scheduleWires();
  scheduleBuild();
}

function renderPortTools(
  parent: HTMLElement,
  instance: Instance,
  input: InputDef,
): void {
  const row = document.createElement("div");
  row.className = "port-tools";
  const rename = document.createElement("input");
  rename.type = "text";
  rename.value = input.name;
  rename.title = t("ports.rename");
  rename.setAttribute("aria-label", t("ports.renameAria", { name: input.name }));
  const commitRename = (): void => {
    if (!renameFreePort(instance, input.name, rename.value)) rename.value = input.name;
  };
  rename.addEventListener("change", commitRename);
  rename.addEventListener("keydown", (event) => {
    if (event.key === "Enter") rename.blur();
    if (event.key === "Escape") {
      rename.value = input.name;
      rename.blur();
    }
  });
  const move = (label: string, title: string, delta: number): HTMLButtonElement => {
    const button = document.createElement("button");
    button.className = "ghost small";
    button.textContent = label;
    button.title = title;
    button.addEventListener("click", () => moveFreePort(instance, input.name, delta));
    return button;
  };
  const remove = document.createElement("button");
  remove.className = "ghost small";
  remove.textContent = "✕";
  remove.title = t("ports.remove", { name: input.name });
  remove.setAttribute("aria-label", t("ports.remove", { name: input.name }));
  remove.addEventListener("click", () => {
    delete instance.inputs[input.name];
    instance.ports = (instance.ports ?? []).filter((name) => name !== input.name);
    commitHistory();
    renderAll();
    scheduleWires();
    scheduleBuild();
  });
  row.append(
    rename,
    move("↑", t("ports.moveUp", { name: input.name }), -1),
    move("↓", t("ports.moveDown", { name: input.name }), 1),
    remove,
  );
  parent.append(row);
}

function unitsFor(kind: UnitKind): Unit[] {
  switch (kind) {
    case "length":
      return ["millimeter", "centimeter", "meter", "inch"];
    case "angle":
      return ["radian", "degree"];
    default:
      return ["unitless"];
  }
}

function defaultParam(input: InputDef): ParamValue | null {
  if (input.default) return structuredClone(input.default);
  const unit = unitsFor(input.unit)[0] ?? "unitless";
  switch (input.value_type) {
    case "number":
      return { Number: { value: 0, unit } };
    case "vector":
      return { Vector: [[0, 0, 0], unit] };
    case "boolean":
      return { Boolean: false };
    case "text":
      return { Text: "" };
    default:
      return null;
  }
}

function defaultsFor(overload: Overload): Record<string, ParamValue> {
  const params: Record<string, ParamValue> = {};
  for (const input of overload.inputs) {
    const value = defaultParam(input);
    if (value && input.value_type !== "field" && input.value_type !== "mesh") {
      params[input.name] = value;
    }
  }
  return params;
}

function namedVariables(): NamedVariable[] {
  return snapshot.variables ?? [];
}

function variableById(id: number): NamedVariable | undefined {
  return namedVariables().find((variable) => variable.id === id);
}

function boundVariable(instance: Instance, name: string): NamedVariable | undefined {
  const id = instance.variable_refs?.[name];
  return id === undefined ? undefined : variableById(id);
}

function numberParameter(instance: Instance, input: InputDef): { value: number; unit: Unit } {
  const param = instance.params[input.name] ?? defaultParam(input);
  return param && "Number" in param ? param.Number : { value: 0, unit: "unitless" };
}

function variableMatchesInput(variable: NamedVariable, input: InputDef, unit: Unit): boolean {
  const kind = unitFamily(variable.unit);
  return (input.unit === "any" || input.unit === kind) && kind === unitFamily(unit);
}

function formulaErrorMessage(error: unknown): string {
  if (error instanceof FormulaError) {
    return t(`variables.formula.${error.code}`, { detail: error.detail });
  }
  return String(error);
}

// 先在副本上计算全部公式和参数边界，成功后再原子地提交变量对象。
function evaluateVariableDraft(draft: NamedVariable[]): { resolved: NamedVariable[]; evaluation: VariableResolution } {
  const evaluation = resolveVariableExpressions(draft);
  const resolved = draft.map((variable) => ({ ...variable, value: evaluation.values.get(variable.id)! }));
  const byId = new Map(resolved.map((variable) => [variable.id, variable]));
  for (const instance of snapshot.instances) {
    for (const [name, id] of Object.entries(instance.variable_refs ?? {})) {
      const variable = byId.get(id);
      const input = inputsFor(instance).find((candidate) => candidate.name === name);
      if (!variable || !input || input.value_type !== "number") {
        throw new Error(t("variables.invalidSnapshot"));
      }
      const unit = numberParameter(instance, input).unit;
      if (!variableMatchesInput(variable, input, unit)) throw new FormulaError("dimension", variable.name, id);
      if (convertQuantity(variable.value, variable.unit, unit) === null) {
        throw new FormulaError("overflow", variable.name, id);
      }
    }
  }
  return { resolved, evaluation };
}

function applyVariableDraft(draft: NamedVariable[]): void {
  const { resolved, evaluation } = evaluateVariableDraft(draft);
  const existing = new Map(namedVariables().map((variable) => [variable.id, variable]));
  snapshot.variables = resolved.map((variable) => {
    const current = existing.get(variable.id);
    return current ? Object.assign(current, variable) : variable;
  });
  formulaDependencies = evaluation.dependencies;
  synchronizeVariableParams();
}

function refreshDerivedVariableFields(): void {
  for (const variable of namedVariables()) {
    const input = variablesList!.querySelector<HTMLInputElement>(
      `.variable-card[data-variable-id="${variable.id}"] .variable-value-row input`,
    );
    if (input && document.activeElement !== input) input.value = String(variable.value);
  }
}

function highlightVariableCards(): void {
  const affected = impactedVariableId === null
    ? new Set<number>()
    : dependentVariableIds(formulaDependencies, impactedVariableId);
  for (const card of variablesList!.querySelectorAll<HTMLElement>(".variable-card")) {
    card.classList.toggle("variable-impact", affected.has(Number(card.dataset.variableId)));
  }
}

// 参数值始终物化为 Number，旧版 WASM 与旧版 Notebook 仍可理解快照中的数值。
function synchronizeVariableParams(): void {
  const evaluation = resolveVariableExpressions(namedVariables());
  formulaDependencies = evaluation.dependencies;
  for (const variable of namedVariables()) {
    variable.value = evaluation.values.get(variable.id)!;
  }
  for (const instance of snapshot.instances) {
    for (const [name, variableId] of Object.entries(instance.variable_refs ?? {})) {
      const variable = variableById(variableId);
      const input = inputsFor(instance).find((candidate) => candidate.name === name);
      if (!variable || !input || input.value_type !== "number") {
        throw new Error(t("variables.invalidSnapshot"));
      }
      const param = numberParameter(instance, input);
      if (!variableMatchesInput(variable, input, param.unit)) {
        throw new Error(t("variables.invalidSnapshot"));
      }
      if (!instance.params[name]) instance.params[name] = { Number: param };
    }
  }
  if (!materializeBoundNumbers(snapshot.instances, namedVariables())) {
    throw new Error(t("variables.invalidValue"));
  }
}

// 发送给 Rust 的数据严格保持 NotebookSnapshot v1 结构，不携带 UI 变量元数据。
function kernelSnapshotJson(): string {
  synchronizeVariableParams();
  return JSON.stringify({
    version: snapshot.version,
    next_id: snapshot.next_id,
    update_policy: snapshot.update_policy,
    instances: snapshot.instances.map((instance) => ({
      id: instance.id,
      def_id: instance.def_id,
      name: instance.name,
      overload: instance.overload,
      params: instance.params,
      inputs: instance.inputs,
      enabled: instance.enabled,
      manual: instance.manual,
    })),
  });
}

function validateVariableSnapshot(value: Snapshot): void {
  if (value.next_variable_id !== undefined &&
      (!Number.isSafeInteger(value.next_variable_id) || value.next_variable_id < 1)) {
    throw new Error(t("variables.invalidSnapshot"));
  }
  if (value.variables === undefined) {
    if (value.instances.some((instance) => Object.keys(instance.variable_refs ?? {}).length > 0)) {
      throw new Error(t("variables.invalidSnapshot"));
    }
    return;
  }
  if (!Array.isArray(value.variables)) throw new Error(t("variables.invalidSnapshot"));
  const ids = new Set<number>();
  const names = new Set<string>();
  const validUnits = new Set<Unit>(["unitless", "millimeter", "centimeter", "meter", "inch", "radian", "degree"]);
  for (const variable of value.variables) {
    if (!Number.isSafeInteger(variable.id) || variable.id < 1 ||
        !validVariableName(variable.name) || names.has(variable.name) || ids.has(variable.id) ||
        !Number.isFinite(variable.value) || !validUnits.has(variable.unit) ||
        (variable.expression !== undefined && typeof variable.expression !== "string")) {
      throw new Error(t("variables.invalidSnapshot"));
    }
    ids.add(variable.id);
    names.add(variable.name);
    if (convertQuantity(variable.value, variable.unit, variable.unit) === null) {
      throw new Error(t("variables.invalidSnapshot"));
    }
  }
  let computed: VariableResolution;
  try {
    computed = resolveVariableExpressions(value.variables);
  } catch (error) {
    throw new Error(`${t("variables.invalidSnapshot")} ${formulaErrorMessage(error)}`);
  }
  for (const instance of value.instances) {
    for (const [name, id] of Object.entries(instance.variable_refs ?? {})) {
      const variable = value.variables.find((candidate) => candidate.id === id);
      const input = inputsFor(instance).find((candidate) => candidate.name === name);
      if (!variable || !input || input.value_type !== "number" ||
          !variableMatchesInput(variable, input, numberParameter(instance, input).unit) ||
          convertQuantity(computed.values.get(variable.id)!, variable.unit, numberParameter(instance, input).unit) === null) {
        throw new Error(t("variables.invalidSnapshot"));
      }
    }
  }
}

function uniqueVariableName(base: string): string {
  const prefix = validVariableName(base) ? base : "variable";
  let result = prefix;
  let suffix = 1;
  while (namedVariables().some((variable) => variable.name === result)) {
    result = `${prefix}${suffix++}`;
  }
  return result;
}

function showVariablesPanel(open: boolean): void {
  document.body.classList.toggle("show-variables", open);
  toggleVariablesButton!.classList.toggle("active", open);
  if (open) {
    document.body.classList.remove("show-palette", "show-properties");
    togglePaletteButton?.classList.remove("active");
    togglePropertiesButton?.classList.remove("active");
  } else if (impactedVariableId !== null) {
    impactedVariableId = null;
    renderInstances();
    highlightVariableCards();
  }
}

function selectVariableImpact(id: number | null): void {
  if (impactedVariableId === id) return;
  impactedVariableId = id;
  renderInstances();
  highlightVariableCards();
}

function addNamedVariable(from?: { instance: Instance; input: InputDef }): void {
  const current = from
    ? numberParameter(from.instance, from.input)
    : { value: 1, unit: "millimeter" as Unit };
  if (convertQuantity(current.value, current.unit, current.unit) === null) {
    showToast(t("variables.invalidValue"));
    return;
  }
  flushNotebookHistory();
  const id = Math.max(snapshot.next_variable_id ?? 1, 1, ...namedVariables().map((variable) => variable.id + 1));
  const variable: NamedVariable = {
    id,
    name: uniqueVariableName(from?.input.name ?? "variable"),
    value: current.value,
    unit: current.unit,
  };
  snapshot.next_variable_id = id + 1;
  snapshot.variables = [...namedVariables(), variable];
  if (from) {
    from.instance.variable_refs = { ...from.instance.variable_refs, [from.input.name]: id };
  }
  synchronizeVariableParams();
  impactedVariableId = id;
  commitHistory();
  renderAll();
  if (from) scheduleBuild();
  showVariablesPanel(true);
  variablesList!.querySelector<HTMLInputElement>(`.variable-card[data-variable-id="${id}"] .variable-name-row input`)?.focus();
}

function bindNumberVariable(instance: Instance, input: InputDef, variableId: number | null): void {
  flushNotebookHistory();
  if (variableId === null) {
    const refs = { ...instance.variable_refs };
    delete refs[input.name];
    instance.variable_refs = refs;
    impactedVariableId = null;
  } else {
    const variable = variableById(variableId);
    if (!variable || !variableMatchesInput(variable, input, numberParameter(instance, input).unit) ||
        convertQuantity(variable.value, variable.unit, numberParameter(instance, input).unit) === null) {
      showToast(t("variables.invalidSnapshot"));
      return;
    }
    instance.variable_refs = { ...instance.variable_refs, [input.name]: variableId };
    impactedVariableId = variableId;
  }
  synchronizeVariableParams();
  commitHistory();
  renderAll();
  scheduleBuild();
}

function removeNamedVariable(id: number): void {
  const derived = dependentVariableIds(formulaDependencies, id);
  const users = namedVariables().filter((variable) => variable.id !== id && derived.has(variable.id));
  if (users.length > 0) {
    showToast(t("variables.formula.inUse", { names: users.map((variable) => variable.name).join(", ") }));
    return;
  }
  flushNotebookHistory();
  synchronizeVariableParams();
  snapshot.variables = namedVariables().filter((variable) => variable.id !== id);
  for (const instance of snapshot.instances) {
    const refs = { ...instance.variable_refs };
    for (const [name, ref] of Object.entries(refs)) {
      if (ref === id) delete refs[name];
    }
    if (instance.variable_refs) instance.variable_refs = refs;
  }
  synchronizeVariableParams();
  if (impactedVariableId === id) impactedVariableId = null;
  commitHistory();
  renderAll();
  scheduleBuild();
}

function layoutNodes(): void {
  const ids = new Set(snapshot.instances.map((instance) => instance.id));
  const depths = new Map<number, number>();
  const visiting = new Set<number>();

  const depthOf = (id: number): number => {
    const known = depths.get(id);
    if (known !== undefined) return known;
    if (visiting.has(id)) return 0;
    visiting.add(id);
    const instance = instanceById(id);
    let depth = 0;
    if (instance) {
      for (const source of Object.values(instance.inputs)) {
        if (ids.has(source)) depth = Math.max(depth, depthOf(source) + 1);
      }
    }
    visiting.delete(id);
    depths.set(id, depth);
    return depth;
  };

  const rows = new Map<number, number>();
  for (const instance of snapshot.instances) {
    const depth = depthOf(instance.id);
    const row = rows.get(depth) ?? 0;
    rows.set(depth, row + 1);
    if (!nodePositions.has(instance.id)) {
      nodePositions.set(instance.id, { x: 24 + row * 190, y: 24 + depth * 154 });
    }
  }
  for (const id of [...nodePositions.keys()]) {
    if (!ids.has(id)) nodePositions.delete(id);
  }
}

function updateCanvasSize(): void {
  let width = 0;
  let height = 0;
  for (const position of nodePositions.values()) {
    width = Math.max(width, position.x + 190);
    height = Math.max(height, position.y + 150);
  }
  const visibleWidth = (notebookBody!.clientWidth - panX) / zoom;
  const visibleHeight = (notebookBody!.clientHeight - panY) / zoom;
  graphWorld!.style.width = `${Math.max(width, visibleWidth + 100, 1)}px`;
  graphWorld!.style.height = `${Math.max(height, visibleHeight + 100, 1)}px`;
}

function applyView(): void {
  graphWorld!.style.transform = `translate(${panX}px, ${panY}px) scale(${zoom})`;
  zoomLabel!.textContent = `${Math.round(zoom * 100)}%`;
  updateCanvasSize();
  scheduleWires();
}

function captureHistory(): HistoryEntry {
  return {
    snapshot: structuredClone(snapshot),
    positions: [...nodePositions].map(([id, position]) => [id, { ...position }]),
    selectedId,
    selectedIds: [...selectedIds],
  };
}

function updateHistoryButtons(): void {
  undoButton!.disabled = historyIndex <= 0;
  redoButton!.disabled = historyIndex < 0 || historyIndex >= history.length - 1;
}

function resetHistory(): void {
  history.length = 0;
  history.push(captureHistory());
  historyIndex = 0;
  updateHistoryButtons();
}

function commitHistory(): void {
  history.splice(historyIndex + 1);
  history.push(captureHistory());
  if (history.length > HISTORY_LIMIT) history.shift();
  historyIndex = history.length - 1;
  updateHistoryButtons();
}

function flushNotebookHistory(): void {
  if (notebookHistoryTimer === undefined) return;
  window.clearTimeout(notebookHistoryTimer);
  notebookHistoryTimer = undefined;
  commitHistory();
}

function scheduleNotebookHistory(): void {
  window.clearTimeout(notebookHistoryTimer);
  notebookHistoryTimer = window.setTimeout(flushNotebookHistory, 500);
}

function restoreHistory(entry: HistoryEntry): void {
  snapshot = structuredClone(entry.snapshot);
  synchronizeVariableParams();
  nodePositions.clear();
  for (const [id, position] of entry.positions) {
    nodePositions.set(id, { ...position });
  }
  selectedId = entry.selectedId;
  selectedIds = new Set(entry.selectedIds);
  errorId = null;
  syncPolicy();
  renderAll();
  scheduleBuild();
  updateHistoryButtons();
}

function undo(): void {
  flushNotebookHistory();
  if (historyIndex <= 0) return;
  historyIndex -= 1;
  restoreHistory(history[historyIndex]);
}

function redo(): void {
  flushNotebookHistory();
  if (historyIndex < 0 || historyIndex >= history.length - 1) return;
  historyIndex += 1;
  restoreHistory(history[historyIndex]);
}

function clampZoom(value: number): number {
  return Math.min(2.5, Math.max(0.25, value));
}

function setZoomAt(screenX: number, screenY: number, next: number): void {
  const clamped = clampZoom(next);
  const ratio = clamped / zoom;
  panX = screenX - (screenX - panX) * ratio;
  panY = screenY - (screenY - panY) * ratio;
  zoom = clamped;
  applyView();
}

function zoomAtCenter(factor: number): void {
  setZoomAt(
    notebookBody!.clientWidth / 2,
    notebookBody!.clientHeight / 2,
    zoom * factor,
  );
}

function fitCanvas(): void {
  const viewportWidth = notebookBody!.clientWidth;
  const viewportHeight = notebookBody!.clientHeight;
  if (nodePositions.size === 0) {
    zoom = 1;
    panX = 0;
    panY = 0;
    applyView();
    return;
  }

  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const position of nodePositions.values()) {
    minX = Math.min(minX, position.x);
    minY = Math.min(minY, position.y);
    maxX = Math.max(maxX, position.x + 190);
    maxY = Math.max(maxY, position.y + 150);
  }
  const width = Math.max(maxX - minX, 1);
  const height = Math.max(maxY - minY, 1);
  const fit = Math.min(
    (viewportWidth - 72) / width,
    (viewportHeight - 72) / height,
    1.6,
  );
  zoom = clampZoom(fit);
  panX = (viewportWidth - width * zoom) / 2 - minX * zoom;
  panY = (viewportHeight - height * zoom) / 2 - minY * zoom;
  applyView();
}

function startNodeDrag(event: PointerEvent, instance: Instance): void {
  if (event.button !== 0) return;
  if (event.shiftKey) return;
  const position = nodePositions.get(instance.id);
  if (!position) return;
  event.preventDefault();
  const previousTargetId = meshTarget()?.id ?? null;

  if (!selectedIds.has(instance.id)) {
    selectedIds.clear();
    selectedIds.add(instance.id);
  }
  selectedId = instance.id;
  if (!selectedIds.has(instance.id)) selectedIds.add(instance.id);
  for (const node of instanceList!.querySelectorAll<HTMLElement>(".instance")) {
    node.classList.toggle("selected", selectedIds.has(Number(node.dataset.id)));
  }
  renderNotebookList();
  renderPreviewTargets();
  renderPalette();
  renderProperties();
  updatePreviewAfterSelection(previousTargetId);

  const entries = [...selectedIds]
    .map((id) => {
      const nodePosition = nodePositions.get(id);
      const element = instanceList!.querySelector<HTMLElement>(
        `.instance[data-id="${id}"]`,
      );
      if (!nodePosition || !element) return null;
      return {
        position: { x: nodePosition.x, y: nodePosition.y },
        originX: nodePosition.x,
        originY: nodePosition.y,
        element,
      };
    })
    .filter((entry): entry is NonNullable<typeof entry> => entry !== null);
  if (entries.length === 0) return;

  const header = event.currentTarget as HTMLElement;
  const startX = event.clientX;
  const startY = event.clientY;
  let moved = false;

  const onMove = (move: PointerEvent): void => {
    const dx = (move.clientX - startX) / zoom;
    const dy = (move.clientY - startY) / zoom;
    if (!moved && Math.hypot(dx, dy) * zoom < 3) return;
    moved = true;
    for (const entry of entries) {
      entry.element.classList.add("dragging");
      entry.position.x = Math.max(4, entry.originX + dx);
      entry.position.y = Math.max(4, entry.originY + dy);
      entry.element.style.left = `${entry.position.x}px`;
      entry.element.style.top = `${entry.position.y}px`;
    }
    updateCanvasSize();
    scheduleWires();
  };

  const onUp = (): void => {
    header.removeEventListener("pointermove", onMove);
    header.removeEventListener("pointerup", onUp);
    header.removeEventListener("pointercancel", onUp);
    for (const entry of entries) entry.element.classList.remove("dragging");
    if (moved) {
      suppressClickUntil = performance.now() + 120;
      commitHistory();
      updateCanvasSize();
      scheduleWires();
    }
  };

  header.setPointerCapture(event.pointerId);
  header.addEventListener("pointermove", onMove);
  header.addEventListener("pointerup", onUp);
  header.addEventListener("pointercancel", onUp);
}

function startMarquee(event: PointerEvent): void {
  if (event.button !== 0) return;
  const body = notebookBody!;
  const bodyRect = body.getBoundingClientRect();
  const startX = event.clientX - bodyRect.left;
  const startY = event.clientY - bodyRect.top;
  let lastX = startX;
  let lastY = startY;
  let moved = false;
  const additive = event.shiftKey;

  marquee!.classList.remove("hidden");
  marquee!.style.left = `${startX}px`;
  marquee!.style.top = `${startY}px`;
  marquee!.style.width = "0px";
  marquee!.style.height = "0px";
  notebook!.setPointerCapture(event.pointerId);

  const onMove = (move: PointerEvent): void => {
    lastX = move.clientX - bodyRect.left;
    lastY = move.clientY - bodyRect.top;
    if (!moved && Math.hypot(lastX - startX, lastY - startY) < 3) return;
    moved = true;
    const left = Math.min(startX, lastX);
    const top = Math.min(startY, lastY);
    marquee!.style.left = `${left}px`;
    marquee!.style.top = `${top}px`;
    marquee!.style.width = `${Math.abs(lastX - startX)}px`;
    marquee!.style.height = `${Math.abs(lastY - startY)}px`;
  };

  const onUp = (): void => {
    const previousTargetId = meshTarget()?.id ?? null;
    notebook!.removeEventListener("pointermove", onMove);
    notebook!.removeEventListener("pointerup", onUp);
    notebook!.removeEventListener("pointercancel", onUp);
    marquee!.classList.add("hidden");
    if (!moved) {
      if (!additive) {
        selectedIds.clear();
        selectedId = null;
        renderInstances();
        renderProperties();
        updatePreviewAfterSelection(previousTargetId);
      }
      return;
    }

    const left = Math.min(startX, lastX);
    const right = Math.max(startX, lastX);
    const top = Math.min(startY, lastY);
    const bottom = Math.max(startY, lastY);
    const hits = new Set<number>();
    for (const instance of snapshot.instances) {
      const element = instanceList!.querySelector<HTMLElement>(
        `.instance[data-id="${instance.id}"]`,
      );
      if (!element) continue;
      const rect = element.getBoundingClientRect();
      const nodeLeft = rect.left - bodyRect.left;
      const nodeRight = rect.right - bodyRect.left;
      const nodeTop = rect.top - bodyRect.top;
      const nodeBottom = rect.bottom - bodyRect.top;
      if (nodeRight >= left && nodeLeft <= right && nodeBottom >= top && nodeTop <= bottom) {
        hits.add(instance.id);
      }
    }
    if (!additive) selectedIds.clear();
    for (const id of hits) selectedIds.add(id);
    selectedId = [...selectedIds][0] ?? null;
    renderInstances();
    renderProperties();
    updatePreviewAfterSelection(previousTargetId);
  };

  notebook!.addEventListener("pointermove", onMove);
  notebook!.addEventListener("pointerup", onUp);
  notebook!.addEventListener("pointercancel", onUp);
}

function autoConnectionsFor(def: BlockDef): Record<string, number> {
  const sources = [...selectedIds]
    .map((id) => instanceById(id))
    .filter((instance): instance is Instance => instance !== undefined)
    .map((instance) => ({ id: instance.id, output: outputType(instance) }))
    .filter((source): source is { id: number; output: ValueType } => source.output !== undefined);
  return planAutoConnections(def.overloads[0]?.inputs ?? [], sources);
}

function closeGraphQuickAdd(): void {
  graphQuickAdd!.classList.add("hidden");
  graphAddPosition = null;
  graphAddSourceId = null;
}

function graphOffersSource(def: BlockDef, sourceId: number | null): boolean {
  if (sourceId === null) return true;
  const source = instanceById(sourceId);
  const type = source ? outputType(source) : undefined;
  if (!type) return false;
  if (type === "field" && (def.id === "list" || def.id === "custom")) return true;
  return (def.overloads[0]?.inputs ?? []).some((input) =>
    ["field", "mesh", "list", "any"].includes(input.value_type) && acceptsOutput(input.value_type, type));
}

function renderGraphQuickResults(): void {
  const list = graphQuickResults!;
  list.replaceChildren();
  const query = graphQuickSearch!.value.trim().toLocaleLowerCase();
  const matches = catalog.filter((def) => graphOffersSource(def, graphAddSourceId) &&
    `${blockLabel(def.id, def.name)} ${def.name} ${def.id} ${categoryLabel(def.category)}`.toLocaleLowerCase().includes(query));
  if (matches.length === 0) {
    const hint = document.createElement("p");
    hint.className = "graph-quick-empty";
    hint.textContent = t("graph.quickEmpty");
    list.append(hint);
    return;
  }
  for (const [index, def] of matches.slice(0, 60).entries()) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "graph-quick-option";
    if (index === 0) button.classList.add("active");
    button.setAttribute("role", "option");
    button.setAttribute("aria-selected", String(index === 0));
    const name = document.createElement("span");
    name.textContent = blockLabel(def.id, def.name);
    const output = document.createElement("span");
    output.textContent = def.overloads[0]?.output ?? "";
    button.append(name, output);
    button.addEventListener("click", () => {
      const position = graphAddPosition ?? { x: 24, y: 24 };
      const sourceId = graphAddSourceId;
      closeGraphQuickAdd();
      addBlock(def, false, position, sourceId);
    });
    list.append(button);
  }
}

function openGraphQuickAdd(clientX: number, clientY: number, sourceId: number | null = null): void {
  if (workspaceMode !== "graph") return;
  const rect = notebookBody!.getBoundingClientRect();
  graphAddPosition = graphPointAt(clientX, clientY, rect.left, rect.top, panX, panY, zoom);
  graphAddSourceId = sourceId;
  const x = Math.max(6, Math.min(clientX - rect.left, rect.width - 280));
  const y = Math.max(6, Math.min(clientY - rect.top, rect.height - 278));
  graphQuickAdd!.style.left = `${x}px`;
  graphQuickAdd!.style.top = `${y}px`;
  graphQuickAdd!.classList.remove("hidden");
  graphQuickSearch!.value = "";
  renderGraphQuickResults();
  graphQuickSearch!.focus();
}

function setGraphPropertiesOpen(open: boolean): void {
  graphPropertiesOpen = open;
  const visible = open && workspaceMode === "graph" && graphExpanded;
  document.body.classList.toggle("graph-properties-open", visible);
  graphPropertiesButton!.classList.toggle("active", visible);
  graphPropertiesButton!.setAttribute("aria-pressed", String(visible));
}

function inspectGraphNode(id: number): void {
  const previousTargetId = meshTarget()?.id ?? null;
  selectedId = id;
  selectedIds = new Set([id]);
  renderInstances();
  renderProperties();
  updatePreviewAfterSelection(previousTargetId);
  if (graphExpanded) setGraphPropertiesOpen(true);
  else if (window.innerWidth <= 900) {
    document.body.classList.add("show-properties");
    togglePropertiesButton?.classList.add("active");
  }
}

function addBlock(def: BlockDef, connectSelected = false, graphPosition?: GraphPoint, sourceId?: number | null): void {
  const overload = def.overloads[0];
  if (!overload) return;
  const source = sourceId === undefined || sourceId === null ? undefined : instanceById(sourceId);
  const sourceType = source ? outputType(source) : undefined;
  const inputs = source && sourceType
    ? planAutoConnections(overload.inputs, [{ id: source.id, output: sourceType }])
    : connectSelected ? autoConnectionsFor(def) : {};
  if (source && sourceType === "field" && Object.keys(inputs).length === 0 &&
      (def.id === "list" || def.id === "custom")) inputs.in1 = source.id;
  const id = snapshot.next_id++;
  snapshot.instances.push({
    id,
    def_id: def.id,
    name: `${def.name} ${id}`,
    overload: overload.name,
    params: defaultsFor(overload),
    inputs,
    enabled: true,
    manual: false,
  });
  selectedId = id;
  selectedIds = new Set([id]);
  if (graphPosition) {
    nodePositions.set(id, { x: Math.max(4, graphPosition.x - 75), y: Math.max(4, graphPosition.y - 20) });
  }
  layoutNodes();
  commitHistory();
  renderAll();
  scheduleBuild();
  if (workspaceMode === "notebook") notebookList!.scrollTop = notebookList!.scrollHeight;
  else if (!graphPosition) fitCanvas();
  else scheduleWires();
}

function deleteInstance(id: number): void {
  snapshot.instances = snapshot.instances.filter((instance) => instance.id !== id);
  for (const instance of snapshot.instances) {
    for (const [name, source] of Object.entries(instance.inputs)) {
      if (source === id) delete instance.inputs[name];
    }
  }
  selectedIds.delete(id);
  if (selectedId === id || selectedId === null || !selectedIds.has(selectedId)) {
    selectedId = [...selectedIds][0] ?? snapshot.instances[0]?.id ?? null;
  }
  layoutNodes();
  commitHistory();
  renderAll();
  scheduleBuild();
}

function deleteSelected(): void {
  if (selectedIds.size === 0) return;
  const removed = new Set(selectedIds);
  snapshot.instances = snapshot.instances.filter((instance) => !removed.has(instance.id));
  for (const instance of snapshot.instances) {
    for (const [name, source] of Object.entries(instance.inputs)) {
      if (removed.has(source)) delete instance.inputs[name];
    }
  }
  selectedIds.clear();
  selectedId = snapshot.instances[0]?.id ?? null;
  if (selectedId !== null) selectedIds.add(selectedId);
  layoutNodes();
  commitHistory();
  renderAll();
  scheduleBuild();
}

function setSelectedEnabled(enabled: boolean): void {
  if (selectedIds.size === 0) return;
  for (const instance of snapshot.instances) {
    if (selectedIds.has(instance.id)) instance.enabled = enabled;
  }
  commitHistory();
  renderAll();
  scheduleBuild();
}

function duplicateSelected(): void {
  const originals = snapshot.instances.filter((instance) => selectedIds.has(instance.id));
  if (originals.length === 0) return;
  const mapping = new Map<number, number>();
  for (const instance of originals) {
    mapping.set(instance.id, snapshot.next_id++);
  }
  const copies: Instance[] = originals.map((instance) => {
    const inputs: Record<string, number> = {};
    for (const [name, source] of Object.entries(instance.inputs)) {
      inputs[name] = mapping.get(source) ?? source;
    }
    return {
      ...structuredClone(instance),
      id: mapping.get(instance.id)!,
      name: `${instance.name} copy`,
      params: structuredClone(instance.params),
      inputs,
    };
  });
  snapshot.instances.push(...copies);
  selectedIds = new Set(copies.map((instance) => instance.id));
  selectedId = copies[copies.length - 1]?.id ?? null;
  layoutNodes();
  commitHistory();
  renderAll();
  scheduleBuild();
}

function alignSelected(mode: "left" | "top" | "distribute-x" | "distribute-y"): void {
  const nodes = snapshot.instances
    .filter((instance) => selectedIds.has(instance.id))
    .map((instance) => ({ id: instance.id, position: nodePositions.get(instance.id) }))
    .filter((entry): entry is { id: number; position: { x: number; y: number } } =>
      Boolean(entry.position),
    );
  if (nodes.length < 2) return;

  if (mode === "left") {
    const left = Math.min(...nodes.map((entry) => entry.position.x));
    for (const entry of nodes) entry.position.x = left;
  } else if (mode === "top") {
    const top = Math.min(...nodes.map((entry) => entry.position.y));
    for (const entry of nodes) entry.position.y = top;
  } else if (nodes.length >= 3) {
    const axis = mode === "distribute-x" ? "x" : "y";
    const sorted = [...nodes].sort((a, b) => a.position[axis] - b.position[axis]);
    const first = sorted[0]!.position[axis];
    const last = sorted[sorted.length - 1]!.position[axis];
    const step = (last - first) / (sorted.length - 1);
    sorted.forEach((entry, index) => {
      entry.position[axis] = first + step * index;
    });
  } else {
    return;
  }

  commitHistory();
  renderAll();
}

function connectInput(targetId: number, inputName: string, sourceId: number): boolean {
  const target = instanceById(targetId);
  const source = instanceById(sourceId);
  if (!target || !source) return false;
  let input = inputsFor(target).find((candidate) => candidate.name === inputName);
  const createdFreePort = !input && isFreePortBlock(target);
  if (!input && isFreePortBlock(target)) {
    input = { name: inputName, value_type: "field", required: false, unit: "any", default: null };
  }
  if (!input) return false;
  const sourceType = outputType(source);
  if (!sourceType) return false;
  if (!acceptsOutput(input.value_type, sourceType)) {
    showToast(t("toast.connectType", { source: sourceType, target: input.value_type }));
    return false;
  }
  if (createsCycle(snapshot.instances, targetId, sourceId)) {
    showToast(t("toast.connectCycle"));
    return false;
  }
  if (createdFreePort) target.ports = [...(target.ports ?? []), inputName];
  target.inputs[inputName] = sourceId;
  commitHistory();
  renderAll();
  scheduleBuild();
  return true;
}

function disconnectInput(targetId: number, inputName: string): void {
  const target = instanceById(targetId);
  if (!target || target.inputs[inputName] === undefined) return;
  delete target.inputs[inputName];
  commitHistory();
  renderAll();
  scheduleBuild();
}

function linkCandidates(target: Instance, input: InputDef): Instance[] {
  return snapshot.instances.filter((candidate) => {
    if (candidate.id === target.id || createsCycle(snapshot.instances, target.id, candidate.id)) {
      return false;
    }
    const candidateType = outputType(candidate);
    return candidateType !== undefined && acceptsOutput(input.value_type, candidateType);
  });
}

function renderPreviewTargets(): void {
  const select = previewTargetSelect!;
  const meshBlocks = snapshot.instances.filter((instance) => outputType(instance) === "mesh");
  if (previewTargetId !== null && !meshBlocks.some((instance) => instance.id === previewTargetId)) {
    previewTargetId = null;
  }
  select.replaceChildren();
  const automatic = document.createElement("option");
  automatic.value = "";
  const autoTarget = autoMeshTarget();
  automatic.textContent = `${t("viewport.outputAuto")} · ${autoTarget ? displayName(autoTarget) : t("viewport.outputNone")}`;
  select.append(automatic);
  for (const instance of meshBlocks) {
    const option = document.createElement("option");
    option.value = String(instance.id);
    option.textContent = `${displayName(instance)}  #${instance.id}`;
    select.append(option);
  }
  select.value = previewTargetId === null ? "" : String(previewTargetId);
  select.disabled = meshBlocks.length === 0;
  select.title = select.selectedOptions[0]?.textContent ?? "";
}

function updatePreviewAfterSelection(previousId: number | null): void {
  if (previewTargetId !== null) return;
  if ((meshTarget()?.id ?? null) !== previousId) {
    framed = false;
    build();
  }
}

function displayName(instance: Instance): string {
  const def = blockDef(instance.def_id);
  if (!def) return instance.name;
  const defaultId = defaultBlockId(instance.name);
  if (defaultId) return blockLabel(defaultId, instance.name);
  if (instance.name === def.name) return blockLabel(def.id, def.name);
  if (instance.name === `${def.name} ${instance.id}`) {
    return `${blockLabel(def.id, def.name)} ${instance.id}`;
  }
  return instance.name;
}

// Notebook 与 Graph 只改变展示方式，始终读写同一份内核快照。
function syncWorkspaceModeControls(): void {
  const graphMode = workspaceMode === "graph";
  document.body.classList.toggle("workspace-graph", graphMode);
  document.body.classList.toggle("graph-expanded", graphMode && graphExpanded);
  showNotebookButton!.classList.toggle("active", !graphMode);
  showNotebookButton!.setAttribute("aria-pressed", String(!graphMode));
  showGraphButton!.classList.toggle("active", graphMode);
  showGraphButton!.setAttribute("aria-pressed", String(graphMode));
  graphExpandButton!.classList.toggle("hidden", !graphMode);
  graphPropertiesButton!.classList.toggle("hidden", !graphMode || !graphExpanded);
  graphExpandButton!.setAttribute("aria-pressed", String(graphMode && graphExpanded));
  const expandKey = graphExpanded ? "action.restoreGraph" : "action.expandGraph";
  graphExpandButton!.title = t(expandKey);
  graphExpandLabel!.textContent = t(expandKey);
  workspaceTitle!.textContent = t(graphMode ? "panel.graph" : "panel.notebook");
  setGraphPropertiesOpen(graphPropertiesOpen);
}

function setWorkspaceMode(nextMode: WorkspaceMode): void {
  cancelGraphWireDrag?.();
  workspaceMode = nextMode;
  closeGraphQuickAdd();
  if (nextMode === "notebook") {
    graphExpanded = false;
    graphPropertiesOpen = false;
  }
  syncWorkspaceModeControls();
  try {
    localStorage.setItem(WORKSPACE_MODE_KEY, nextMode);
  } catch (error) {
    console.warn("无法保存工作区模式", error);
  }
  requestAnimationFrame(() => {
    refreshLayout();
    if (nextMode === "graph") fitCanvas();
    scheduleWires();
  });
}

function setGraphExpanded(expanded: boolean): void {
  if (workspaceMode !== "graph") return;
  cancelGraphWireDrag?.();
  graphExpanded = expanded;
  if (!expanded) graphPropertiesOpen = false;
  if (expanded) {
    document.body.classList.remove("show-properties");
    togglePropertiesButton?.classList.remove("active");
  }
  syncWorkspaceModeControls();
  requestAnimationFrame(() => {
    refreshLayout();
    fitCanvas();
    scheduleWires();
  });
}

function renderVariables(): void {
  const list = variablesList!;
  list.replaceChildren();
  variablesCount!.textContent = String(namedVariables().length);
  if (namedVariables().length === 0) {
    const empty = document.createElement("p");
    empty.className = "variable-empty";
    empty.textContent = t("variables.empty");
    list.append(empty);
    return;
  }

  for (const variable of namedVariables()) {
    const card = document.createElement("section");
    card.className = "variable-card";
    card.dataset.variableId = String(variable.id);
    const nameRow = document.createElement("div");
    nameRow.className = "variable-name-row";
    const name = document.createElement("input");
    name.type = "text";
    name.value = variable.name;
    name.setAttribute("aria-label", t("variables.binding"));
    name.addEventListener("focus", () => selectVariableImpact(variable.id));
    name.addEventListener("change", () => {
      const next = name.value.trim();
      if (!validVariableName(next)) {
        showToast(t("variables.invalidName"));
        name.value = variable.name;
        return;
      }
      if (namedVariables().some((entry) => entry.id !== variable.id && entry.name === next)) {
        showToast(t("variables.duplicateName", { name: next }));
        name.value = variable.name;
        return;
      }
      if (next === variable.name) return;
      flushNotebookHistory();
      try {
        applyVariableDraft(namedVariables().map((entry) => ({
          ...entry,
          name: entry.id === variable.id ? next : entry.name,
          expression: entry.expression
            ? renameFormulaReference(entry.expression, variable.name, next)
            : undefined,
        })));
      } catch (error) {
        showToast(formulaErrorMessage(error));
        name.value = variable.name;
        return;
      }
      commitHistory();
      renderAll();
    });
    const id = document.createElement("span");
    id.className = "variable-id";
    id.textContent = `#${variable.id}`;
    nameRow.append(name, id);

    const valueRow = document.createElement("div");
    valueRow.className = "variable-value-row";
    const value = document.createElement("input");
    value.type = "number";
    value.step = "any";
    value.value = String(variable.value);
    value.disabled = Boolean(variable.expression?.trim());
    if (value.disabled) value.title = t("variables.formula.computed");
    value.setAttribute("aria-label", variable.name);
    value.addEventListener("focus", () => selectVariableImpact(variable.id));
    value.addEventListener("input", () => {
      if (value.value.trim() === "") return;
      const next = Number(value.value);
      try {
        applyVariableDraft(namedVariables().map((entry) => entry.id === variable.id
          ? { ...entry, value: next }
          : { ...entry }));
      } catch {
        return;
      }
      const impactChanged = impactedVariableId !== variable.id;
      selectVariableImpact(variable.id);
      if (!impactChanged) renderNotebookList();
      renderProperties();
      refreshDerivedVariableFields();
      scheduleBuild();
      scheduleNotebookHistory();
    });
    value.addEventListener("change", () => {
      if (value.value.trim() === "" || Number(value.value) !== variable.value) {
        showToast(t("variables.invalidValue"));
        value.value = String(variable.value);
      }
      flushNotebookHistory();
    });
    const unit = document.createElement("select");
    for (const optionUnit of unitsFor(unitFamily(variable.unit))) {
      const option = document.createElement("option");
      option.value = optionUnit;
      option.textContent = unitLabel(optionUnit);
      unit.append(option);
    }
    unit.value = variable.unit;
    unit.addEventListener("change", () => {
      const nextUnit = unit.value as Unit;
      const converted = convertQuantity(variable.value, variable.unit, nextUnit);
      if (converted === null) {
        unit.value = variable.unit;
        showToast(t("variables.invalidValue"));
        return;
      }
      flushNotebookHistory();
      try {
        applyVariableDraft(namedVariables().map((entry) => entry.id === variable.id
          ? { ...entry, value: converted, unit: nextUnit }
          : { ...entry }));
      } catch (error) {
        unit.value = variable.unit;
        showToast(formulaErrorMessage(error));
        return;
      }
      impactedVariableId = variable.id;
      commitHistory();
      renderAll();
      scheduleBuild();
    });
    valueRow.append(value, unit);

    const formulaRow = document.createElement("div");
    formulaRow.className = "variable-formula-row";
    const fx = document.createElement("span");
    fx.className = "variable-fx";
    fx.textContent = "ƒx";
    fx.title = t("variables.formula.label");
    const formulaInput = document.createElement("input");
    formulaInput.type = "text";
    formulaInput.className = "variable-formula-input";
    formulaInput.spellcheck = false;
    formulaInput.placeholder = t("variables.formula.placeholder");
    formulaInput.value = variable.expression ?? "";
    formulaInput.setAttribute("aria-label", t("variables.formula.label"));
    formulaInput.addEventListener("focus", () => selectVariableImpact(variable.id));
    const formulaError = document.createElement("p");
    formulaError.className = "variable-formula-error";
    formulaError.setAttribute("role", "status");
    formulaInput.addEventListener("input", () => {
      formulaInput.classList.remove("invalid");
      formulaError.textContent = "";
    });
    formulaInput.addEventListener("change", () => {
      const next = formulaInput.value.trim();
      if (next === (variable.expression ?? "")) return;
      flushNotebookHistory();
      try {
        applyVariableDraft(namedVariables().map((entry) => entry.id === variable.id
          ? { ...entry, expression: next || undefined }
          : { ...entry }));
      } catch (error) {
        formulaInput.classList.add("invalid");
        formulaError.textContent = formulaErrorMessage(error);
        return;
      }
      impactedVariableId = variable.id;
      commitHistory();
      renderAll();
      scheduleBuild();
    });
    formulaRow.append(fx, formulaInput);

    const foot = document.createElement("div");
    foot.className = "variable-foot";
    const affected = document.createElement("span");
    affected.textContent = t("variables.affected", {
      count: affectedBlockIds(snapshot.instances, dependentVariableIds(formulaDependencies, variable.id)).size,
    });
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "✕";
    remove.title = t("variables.remove");
    remove.setAttribute("aria-label", remove.title);
    remove.addEventListener("click", () => removeNamedVariable(variable.id));
    foot.append(affected, remove);
    card.append(nameRow, valueRow, formulaRow, formulaError, foot);
    list.append(card);
  }
  highlightVariableCards();
}

function renderNotebookList(): void {
  const list = notebookList!;
  const scrollTop = list.scrollTop;
  const focused = document.activeElement as HTMLElement | null;
  const focusedId = focused?.closest<HTMLElement>(".notebook-item")?.dataset.id;
  const focusedParam = focused instanceof HTMLInputElement ? focused.dataset.notebookParam : undefined;
  const focusedLink = focused instanceof HTMLSelectElement ? focused.dataset.notebookInput : undefined;
  const focusedHead = focused?.classList.contains("notebook-item-head") ?? false;
  list.replaceChildren();
  notebookCount!.textContent = String(snapshot.instances.length);
  if (snapshot.instances.length === 0) {
    const empty = document.createElement("p");
    empty.className = "notebook-empty";
    empty.textContent = t("notebook.empty");
    list.append(empty);
    return;
  }

  for (const [index, instance] of snapshot.instances.entries()) {
    const item = document.createElement("section");
    item.className = "notebook-item";
    item.setAttribute("role", "listitem");
    item.dataset.id = String(instance.id);
    item.classList.toggle("selected", selectedId === instance.id);
    item.classList.toggle("error", errorId === instance.id);
    item.classList.toggle("disabled", !instance.enabled);
    item.classList.toggle("variable-impact", impactedIds.has(instance.id));

    const head = document.createElement("button");
    head.type = "button";
    head.className = "notebook-item-head";
    head.setAttribute("aria-expanded", String(selectedId === instance.id));
    const indexLabel = document.createElement("span");
    indexLabel.className = "notebook-index";
    indexLabel.textContent = String(index + 1).padStart(2, "0");
    const symbol = document.createElement("span");
    symbol.className = "notebook-symbol";
    const kind = outputType(instance);
    symbol.textContent = kind === "mesh" ? "M" : kind === "field" ? "F" : "#";
    const name = document.createElement("span");
    name.className = "notebook-block-name";
    name.textContent = displayName(instance);
    name.title = displayName(instance);
    const output = document.createElement("span");
    output.className = "notebook-output";
    output.textContent = outputType(instance) ?? "?";
    const state = document.createElement("span");
    state.className = "notebook-state";
    state.setAttribute("aria-label", instance.enabled ? "Enabled" : "Disabled");
    head.append(indexLabel, symbol, name, output, state);
    head.addEventListener("click", () => {
      const previousId = meshTarget()?.id ?? null;
      selectedId = instance.id;
      selectedIds = new Set([instance.id]);
      renderInstances();
      renderProperties();
      updatePreviewAfterSelection(previousId);
    });
    item.append(head);

    if (selectedId === instance.id) {
      const detail = document.createElement("div");
      detail.className = "notebook-detail";
      const linkInputs = inputsFor(instance).filter(
        (input) => ["field", "mesh", "list", "any"].includes(input.value_type),
      );
      if (linkInputs.length > 0 || isFreePortBlock(instance)) {
        const sourcesHeading = document.createElement("p");
        sourcesHeading.className = "notebook-detail-heading";
        sourcesHeading.textContent = t("notebook.sources");
        detail.append(sourcesHeading);
        for (const input of linkInputs) {
          const row = document.createElement("div");
          row.className = "notebook-source-row";
          const label = document.createElement("label");
          label.textContent = fieldLabel(input.name);
          const select = document.createElement("select");
          select.id = `notebook-link-${instance.id}-${input.name}`;
          select.dataset.notebookInput = input.name;
          label.htmlFor = select.id;
          const none = document.createElement("option");
          none.value = "";
          none.textContent = t("notebook.chooseSource");
          select.append(none);
          const candidates = linkCandidates(instance, input);
          for (const candidate of candidates) {
            const option = document.createElement("option");
            option.value = String(candidate.id);
            option.textContent = `${displayName(candidate)}  #${candidate.id}`;
            select.append(option);
          }
          const linkedId = instance.inputs[input.name];
          if (linkedId !== undefined && !candidates.some((candidate) => candidate.id === linkedId)) {
            const invalid = document.createElement("option");
            invalid.value = String(linkedId);
            invalid.textContent = `#${linkedId} (${t("notebook.invalidSource")})`;
            select.append(invalid);
          }
          select.value = linkedId === undefined ? "" : String(linkedId);
          select.disabled = candidates.length === 0 && linkedId === undefined;
          select.addEventListener("change", () => {
            if (select.value === "") disconnectInput(instance.id, input.name);
            else if (!connectInput(instance.id, input.name, Number(select.value))) {
              select.value = linkedId === undefined ? "" : String(linkedId);
            }
          });
          row.append(label, select);
          detail.append(row);
        }
        if (isFreePortBlock(instance)) {
          const addPort = document.createElement("button");
          addPort.className = "notebook-detail-action";
          addPort.type = "button";
          addPort.textContent = t("notebook.addPort");
          addPort.addEventListener("click", () => {
            instance.ports = [...(instance.ports ?? []), nextFreePortName(instance)];
            commitHistory();
            renderAll();
          });
          detail.append(addPort);
        }
      }

      const numbers = inputsFor(instance)
        .filter((input) => input.value_type === "number" && instance.inputs[input.name] === undefined)
        .slice(0, 2);
      if (numbers.length > 0) {
        const paramsHeading = document.createElement("p");
        paramsHeading.className = "notebook-detail-heading";
        paramsHeading.textContent = t("notebook.params");
        detail.append(paramsHeading);
        for (const input of numbers) {
          const current = instance.params[input.name] ?? defaultParam(input);
          const param = current && "Number" in current
            ? current.Number
            : { value: 0, unit: "unitless" as Unit };
          const bound = boundVariable(instance, input.name);
          const row = document.createElement("div");
          row.className = "notebook-quick-row";
          const label = document.createElement("label");
          label.textContent = `${fieldLabel(input.name)} (${unitLabel(param.unit)})`;
          const value = document.createElement("input");
          value.type = "number";
          value.step = "any";
          value.value = String(param.value);
          value.disabled = Boolean(bound);
          if (bound) value.title = t("variables.drivenBy", { name: bound.name });
          value.dataset.notebookParam = input.name;
          value.dataset.instanceId = String(instance.id);
          value.addEventListener("input", () => {
            const nextValue = Number(value.value);
            if (!Number.isFinite(nextValue) || value.value === "") return;
            instance.params[input.name] = { Number: { value: nextValue, unit: param.unit } };
            renderProperties();
            scheduleBuild();
            scheduleNotebookHistory();
          });
          value.addEventListener("change", flushNotebookHistory);
          row.append(label);
          if (bound) {
            const jump = document.createElement("button");
            jump.type = "button";
            jump.className = "notebook-variable-tag";
            jump.textContent = `↳ ${bound.name}`;
            jump.title = t("variables.drivenBy", { name: bound.name });
            jump.addEventListener("click", () => {
              showVariablesPanel(true);
              variablesList!.querySelector<HTMLInputElement>(
                `.variable-card[data-variable-id="${bound.id}"] .variable-value-row input`,
              )?.focus();
            });
            row.append(jump);
          }
          row.append(value);
          detail.append(row);
        }
      }

      const openGraph = document.createElement("button");
      openGraph.type = "button";
      openGraph.className = "notebook-detail-action";
      openGraph.textContent = t("notebook.openGraph");
      openGraph.addEventListener("click", () => {
        setWorkspaceMode("graph");
        setGraphExpanded(true);
      });
      const actions = document.createElement("div");
      actions.className = "notebook-detail-actions";
      actions.append(openGraph);
      if (outputType(instance) === "mesh") {
        const preview = document.createElement("button");
        preview.className = "notebook-detail-action";
        preview.type = "button";
        preview.textContent = t(previewTargetId === instance.id ? "viewport.previewPinned" : "viewport.previewThis");
        preview.addEventListener("click", () => {
          previewTargetId = previewTargetId === instance.id ? null : instance.id;
          framed = false;
          renderInstances();
          build();
        });
        actions.append(preview);
      }
      detail.append(actions);
      item.append(detail);
    }
    list.append(item);
  }
  list.scrollTop = scrollTop;
  // WASM 自动构建会刷新状态；保留正在编辑的快捷参数焦点。
  if (focusedId !== undefined) {
    const item = list.querySelector<HTMLElement>(`.notebook-item[data-id="${focusedId}"]`);
    if (focusedParam !== undefined) {
      const input = [...(item?.querySelectorAll<HTMLInputElement>("input[data-notebook-param]") ?? [])]
        .find((candidate) => candidate.dataset.notebookParam === focusedParam);
      input?.focus({ preventScroll: true });
    } else if (focusedLink !== undefined) {
      const select = [...(item?.querySelectorAll<HTMLSelectElement>("select[data-notebook-input]") ?? [])]
        .find((candidate) => candidate.dataset.notebookInput === focusedLink);
      select?.focus({ preventScroll: true });
    } else if (focusedHead) {
      item?.querySelector<HTMLButtonElement>(".notebook-item-head")?.focus({ preventScroll: true });
    }
  }
}

function renderPalette(): void {
  paletteList!.replaceChildren();
  const query = blockSearch?.value.trim().toLowerCase() ?? "";
  const groups = new Map<string, BlockDef[]>();
  let shown = 0;
  for (const def of catalog) {
    if (
      query &&
      !`${def.name} ${def.description} ${def.category}`.toLowerCase().includes(query)
    ) {
      continue;
    }
    const list = groups.get(def.category) ?? [];
    list.push(def);
    groups.set(def.category, list);
    shown += 1;
  }
  if (paletteCount) {
    paletteCount.textContent = query ? `${shown} / ${catalog.length}` : String(catalog.length);
  }
  if (groups.size === 0) {
    const hint = document.createElement("p");
    hint.className = "empty-note";
    hint.textContent = t("palette.noMatch");
    paletteList!.append(hint);
    return;
  }
  for (const [category, defs] of groups) {
    const heading = document.createElement("div");
    heading.className = "category";
    heading.textContent = categoryLabel(category);
    paletteList!.append(heading);
    for (const def of defs) {
      const entry = document.createElement("div");
      entry.className = "block-entry";
      const button = document.createElement("button");
      button.className = "block-button";
      button.type = "button";
      button.title = blockDescription(def.id, def.description);
      const name = document.createElement("span");
      name.textContent = blockLabel(def.id, def.name);
      const type = document.createElement("span");
      type.className = "type";
      type.textContent = def.overloads[0]?.output ?? "";
      button.append(name, type);
      button.addEventListener("click", () => addBlock(def));
      entry.append(button);
      const connections = autoConnectionsFor(def);
      const count = Object.keys(connections).length;
      if (count > 0) {
        const connect = document.createElement("button");
        connect.className = "block-connect-button";
        connect.type = "button";
        connect.textContent = "↳";
        connect.title = t("palette.addConnect", { count });
        connect.setAttribute("aria-label", connect.title);
        connect.addEventListener("click", () => {
          addBlock(def, true);
          if (document.body.classList.contains("show-palette")) togglePaletteButton?.click();
        });
        entry.append(connect);
      }
      paletteList!.append(entry);
    }
  }
}

function graphPortFromElement(element: HTMLElement): GraphPort | null {
  const nodeId = Number(element.dataset.instanceId);
  if (!Number.isSafeInteger(nodeId) || nodeId <= 0) return null;
  if (element.dataset.port === "out") {
    const source = instanceById(nodeId);
    const valueType = source ? outputType(source) : undefined;
    return valueType ? { kind: "output", nodeId, valueType } : null;
  }
  if (element.classList.contains("input-port") && element.dataset.input) {
    return { kind: "input", nodeId, name: element.dataset.input, valueType: element.dataset.valueType ?? "field" };
  }
  return null;
}

function graphPortAt(clientX: number, clientY: number): { element: HTMLElement; port: GraphPort } | null {
  for (const hit of document.elementsFromPoint(clientX, clientY)) {
    const element = hit.closest<HTMLElement>(".input-port[data-instance-id], .port[data-port='out']")
      ?? hit.closest<HTMLElement>(".input-row")?.querySelector<HTMLElement>(".input-port[data-instance-id]");
    if (!element || !graphWorld!.contains(element)) continue;
    const port = graphPortFromElement(element);
    if (port) return { element, port };
  }
  return null;
}

function graphPortAnchor(element: HTMLElement, port: GraphPort): GraphPoint {
  const rect = element.getBoundingClientRect();
  const world = graphWorld!.getBoundingClientRect();
  return {
    x: ((port.kind === "output" ? rect.right : rect.left) - world.left) / zoom,
    y: (rect.top + rect.height / 2 - world.top) / zoom,
  };
}

function clearGraphWire(): void {
  activeGraphWire = null;
  document.body.classList.remove("graph-connecting");
  instanceList!.querySelectorAll(".input-port.over, .input-port.incompatible, .input-port.can-connect, .port.can-connect")
    .forEach((element) => element.classList.remove("over", "incompatible", "can-connect"));
  scheduleWires();
}

function startGraphPortDrag(event: PointerEvent, element: HTMLElement, from: GraphPort): void {
  if (workspaceMode !== "graph" || event.button !== 0) return;
  event.preventDefault();
  event.stopPropagation();
  closeGraphQuickAdd();
  const rect = notebookBody!.getBoundingClientRect();
  const startX = event.clientX;
  const startY = event.clientY;
  let moved = false;
  activeGraphWire = {
    from, element,
    pointer: graphPointAt(event.clientX, event.clientY, rect.left, rect.top, panX, panY, zoom),
  };
  document.body.classList.add("graph-connecting");

  for (const candidate of instanceList!.querySelectorAll<HTMLElement>(".input-port[data-instance-id], .port[data-port='out']")) {
    const port = graphPortFromElement(candidate);
    if (port && candidate !== element && planGraphConnection(snapshot.instances, from, port).ok) {
      candidate.classList.add("can-connect");
    }
  }

  const move = (pointer: PointerEvent): void => {
    if (pointer.pointerId !== event.pointerId || !activeGraphWire) return;
    if (!moved && Math.hypot(pointer.clientX - startX, pointer.clientY - startY) < 3) return;
    moved = true;
    activeGraphWire.pointer = graphPointAt(pointer.clientX, pointer.clientY, rect.left, rect.top, panX, panY, zoom);
    instanceList!.querySelectorAll(".input-port.over, .input-port.incompatible, .port.over, .port.incompatible")
      .forEach((candidate) => candidate.classList.remove("over", "incompatible"));
    const hit = graphPortAt(pointer.clientX, pointer.clientY);
    if (hit && hit.element !== element) {
      const plan = planGraphConnection(snapshot.instances, from, hit.port);
      hit.element.classList.add(plan.ok ? "over" : "incompatible");
    }
    scheduleWires();
  };
  const cleanup = (): void => {
    cancelGraphWireDrag = null;
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
    if (element.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId);
    clearGraphWire();
  };
  const up = (pointer: PointerEvent): void => {
    if (pointer.pointerId !== event.pointerId) return;
    const hit = moved ? graphPortAt(pointer.clientX, pointer.clientY) : null;
    cleanup();
    if (!moved) return;
    suppressClickUntil = performance.now() + 140;
    if (hit) {
      const plan = planGraphConnection(snapshot.instances, from, hit.port);
      if (plan.ok) {
        connectInput(plan.connection.targetId, plan.connection.inputName, plan.connection.sourceId);
      } else if (plan.reason === "cycle") {
        showToast(t("toast.connectCycle"));
      } else if (plan.reason === "type") {
        const source = from.kind === "output" ? from : hit.port;
        const target = from.kind === "input" ? from : hit.port;
        showToast(t("toast.connectType", { source: source.valueType, target: target.valueType }));
      } else {
        showToast(t("graph.invalidDrop"));
      }
    } else if (from.kind === "input") {
      disconnectInput(from.nodeId, from.name);
    } else {
      openGraphQuickAdd(pointer.clientX, pointer.clientY, from.nodeId);
    }
  };
  const cancel = (pointer: PointerEvent): void => {
    if (pointer.pointerId === event.pointerId) cleanup();
  };
  element.setPointerCapture(event.pointerId);
  cancelGraphWireDrag = cleanup;
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", cancel);
}

function renderInstances(): void {
  if (impactedVariableId !== null && !variableById(impactedVariableId)) impactedVariableId = null;
  impactedIds = impactedVariableId === null
    ? new Set<number>()
    : affectedBlockIds(snapshot.instances, dependentVariableIds(formulaDependencies, impactedVariableId));
  renderPalette();
  renderNotebookList();
  renderPreviewTargets();
  layoutNodes();
  updateCanvasSize();
  instanceList!.replaceChildren();
  if (snapshot.instances.length === 0) {
    const hint = document.createElement("p");
    hint.className = "hint empty-hint";
    hint.textContent = t("graph.empty");
    instanceList!.append(hint);
    scheduleWires();
    return;
  }

  for (const instance of snapshot.instances) {
    const def = blockDef(instance.def_id);
    const card = document.createElement("div");
    card.className = "instance";
    card.dataset.id = String(instance.id);
    const position = nodePositions.get(instance.id) ?? { x: 24, y: 24 };
    card.style.left = `${position.x}px`;
    card.style.top = `${position.y}px`;
    if (selectedIds.has(instance.id)) card.classList.add("selected");
    if (instance.id === errorId) card.classList.add("error");
    if (impactedIds.has(instance.id)) card.classList.add("variable-impact");

    const row = document.createElement("div");
    row.className = "row";
    row.addEventListener("pointerdown", (event) => {
      if ((event.target as HTMLElement).closest(".port, button")) return;
      startNodeDrag(event, instance);
    });
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = displayName(instance);
    const output = document.createElement("span");
    output.className = "meta port";
    output.textContent = outputType(instance) ?? "?";
    output.title = t("graph.connectHint");
    output.dataset.instanceId = String(instance.id);
    output.dataset.port = "out";
    output.addEventListener("pointerdown", (event) => {
      const port = graphPortFromElement(output);
      if (port) startGraphPortDrag(event, output, port);
    });
    const settings = document.createElement("button");
    settings.type = "button";
    settings.className = "node-settings";
    settings.textContent = "⚙";
    settings.title = t("graph.editProperties");
    settings.setAttribute("aria-label", settings.title);
    settings.addEventListener("click", (event) => {
      event.stopPropagation();
      inspectGraphNode(instance.id);
    });
    row.append(name, settings, output);

    const meta = document.createElement("div");
    meta.className = "meta";
    meta.textContent = `#${instance.id} · ${def?.id ?? instance.def_id}${
      instance.enabled ? "" : ` · ${t("graph.disabled")}`
    }`;
    card.append(row, meta);

    const overload = activeOverload(instance);
    if (overload) {
      for (const input of inputsFor(instance)) {
        if (!["field", "mesh", "list", "any"].includes(input.value_type)) continue;
        const inputRow = document.createElement("div");
        inputRow.className = "input-row";
        const port = document.createElement("span");
        port.className = "input-port";
        port.dataset.instanceId = String(instance.id);
        port.dataset.input = input.name;
        port.dataset.valueType = input.value_type;
        const source = instance.inputs[input.name]
          ? instanceById(instance.inputs[input.name])
          : undefined;
        if (source) port.classList.add("connected");
        port.title = source ? t("graph.rewire") : `${input.name} · ${input.value_type}`;
        port.addEventListener("pointerdown", (event) => {
          const graphPort = graphPortFromElement(port);
          if (graphPort) startGraphPortDrag(event, port, graphPort);
        });
        port.addEventListener("contextmenu", (event) => {
          event.preventDefault();
          event.stopPropagation();
          disconnectInput(instance.id, input.name);
        });
        const label = document.createElement("span");
        label.textContent = source ? `${input.name} ← ${displayName(source)}` : `${input.name} —`;
        inputRow.append(port, label);
        card.append(inputRow);
      }
      if (isFreePortBlock(instance)) {
        const addRow = document.createElement("div");
        addRow.className = "input-row add-port-row";
        const addPort = document.createElement("span");
        addPort.className = "input-port add-port";
        addPort.title = t("graph.addField");
        addPort.dataset.instanceId = String(instance.id);
        addPort.dataset.input = nextFreePortName(instance);
        addPort.dataset.valueType = "field";
        addPort.addEventListener("pointerdown", (event) => {
          const graphPort = graphPortFromElement(addPort);
          if (graphPort) startGraphPortDrag(event, addPort, graphPort);
        });
        const label = document.createElement("span");
        label.textContent = t("graph.addField");
        addRow.append(addPort, label);
        card.append(addRow);
      }
    }

    card.addEventListener("click", (event) => {
      if (performance.now() < suppressClickUntil) return;
      const previousTargetId = meshTarget()?.id ?? null;
      if (event.shiftKey) {
        if (selectedIds.has(instance.id)) {
          selectedIds.delete(instance.id);
          if (selectedId === instance.id) {
            selectedId = [...selectedIds][0] ?? null;
          }
        } else {
          selectedIds.add(instance.id);
          selectedId = instance.id;
        }
      } else {
        if (!selectedIds.has(instance.id)) {
          selectedIds.clear();
          selectedIds.add(instance.id);
        }
        selectedId = instance.id;
      }
      renderInstances();
      renderProperties();
      updatePreviewAfterSelection(previousTargetId);
    });
    card.addEventListener("dblclick", (event) => {
      if ((event.target as HTMLElement).closest(".port, .input-port, .node-settings")) return;
      inspectGraphNode(instance.id);
    });
    instanceList!.append(card);
  }
  updateCanvasSize();
  scheduleWires();
}

function makeLabel(text: string, required: boolean): HTMLLabelElement {
  const label = document.createElement("label");
  label.className = "field-label";
  label.textContent = text;
  if (required) {
    const mark = document.createElement("span");
    mark.className = "req";
    mark.textContent = " *";
    label.append(mark);
  }
  return label;
}

function makeUnitSelect(
  kind: UnitKind,
  value: Unit,
  onChange: (unit: Unit) => void,
): HTMLSelectElement {
  const select = document.createElement("select");
  for (const unit of unitsFor(kind)) {
    const option = document.createElement("option");
    option.value = unit;
    option.textContent = unitLabel(unit);
    select.append(option);
  }
  select.value = unitsFor(kind).includes(value) ? value : unitsFor(kind)[0];
  select.addEventListener("change", () => onChange(select.value as Unit));
  return select;
}

function renderParam(
  parent: HTMLElement,
  instance: Instance,
  input: InputDef,
): void {
  parent.append(makeLabel(fieldLabel(input.name), input.required));
  const current = instance.params[input.name] ?? defaultParam(input);

  if (input.value_type === "number") {
    const param = current && "Number" in current ? current.Number : { value: 0, unit: "unitless" as Unit };
    const bound = boundVariable(instance, input.name);
    const wrapper = document.createElement("div");
    wrapper.className = "with-unit";
    const value = document.createElement("input");
    value.type = "number";
    value.step = "any";
    value.value = String(param.value);
    value.disabled = Boolean(bound);
    const unit = makeUnitSelect(input.unit, param.unit, (nextUnit) => {
      const valueNow = Number(value.value) || 0;
      instance.params[input.name] = { Number: { value: valueNow, unit: nextUnit } };
      commitHistory();
      scheduleBuild();
    });
    unit.disabled = Boolean(bound);
    value.addEventListener("input", () => {
      instance.params[input.name] = {
        Number: { value: Number(value.value) || 0, unit: unit.value as Unit },
      };
      renderNotebookList();
      scheduleBuild();
    });
    value.addEventListener("change", commitHistory);
    wrapper.append(value, unit);
    parent.append(wrapper);

    const bindingRow = document.createElement("div");
    bindingRow.className = "param-variable";
    const binding = document.createElement("select");
    binding.setAttribute("aria-label", t("variables.binding"));
    const local = document.createElement("option");
    local.value = "";
    local.textContent = t("variables.unbound");
    binding.append(local);
    for (const variable of namedVariables()) {
      if (!variableMatchesInput(variable, input, param.unit) ||
          convertQuantity(variable.value, variable.unit, param.unit) === null) continue;
      const option = document.createElement("option");
      option.value = String(variable.id);
      option.textContent = variable.name;
      binding.append(option);
    }
    binding.value = bound ? String(bound.id) : "";
    binding.addEventListener("change", () => {
      bindNumberVariable(instance, input, binding.value === "" ? null : Number(binding.value));
    });
    const create = document.createElement("button");
    create.type = "button";
    create.textContent = "+";
    create.title = t("variables.createForParam");
    create.setAttribute("aria-label", create.title);
    create.disabled = Boolean(bound);
    create.addEventListener("click", () => addNamedVariable({ instance, input }));
    bindingRow.append(binding, create);
    parent.append(bindingRow);
    if (bound) {
      const hint = document.createElement("p");
      hint.className = "param-variable-hint";
      hint.textContent = t("variables.drivenBy", { name: bound.name });
      parent.append(hint);
    }
    return;
  }

  if (input.value_type === "vector") {
    const param =
      current && "Vector" in current ? current.Vector : ([[0, 0, 0], "unitless"] as [number[], Unit]);
    const wrapper = document.createElement("div");
    wrapper.className = "vector";
    const values: HTMLInputElement[] = [];
    for (let axis = 0; axis < 3; axis += 1) {
      const value = document.createElement("input");
      value.type = "number";
      value.step = "any";
      value.value = String(param[0][axis] ?? 0);
      values.push(value);
      wrapper.append(value);
    }
    const unit = makeUnitSelect(input.unit, param[1], (nextUnit) => {
      const vector = values.map((field) => Number(field.value) || 0) as [number, number, number];
      instance.params[input.name] = { Vector: [vector, nextUnit] };
      commitHistory();
      scheduleBuild();
    });
    const update = (): void => {
      const vector = values.map((field) => Number(field.value) || 0) as [number, number, number];
      instance.params[input.name] = { Vector: [vector, unit.value as Unit] };
      scheduleBuild();
    };
    for (const field of values) {
      field.addEventListener("input", update);
      field.addEventListener("change", commitHistory);
    }
    wrapper.append(unit);
    parent.append(wrapper);
    return;
  }

  if (input.value_type === "boolean") {
    const param = current && "Boolean" in current ? current.Boolean : false;
    const wrapper = document.createElement("div");
    wrapper.className = "inline";
    const value = document.createElement("input");
    value.type = "checkbox";
    value.checked = param;
    value.addEventListener("change", () => {
      instance.params[input.name] = { Boolean: value.checked };
      commitHistory();
      scheduleBuild();
    });
    wrapper.append(value);
    parent.append(wrapper);
    return;
  }

  if (input.value_type === "text") {
    const param = current && "Text" in current ? current.Text : "";
    let preset: HTMLSelectElement | null = null;
    if (instance.def_id === "custom" && input.name === "expression" && customTemplates.length > 0) {
      parent.append(makeLabel(t("properties.preset"), false));
      preset = document.createElement("select");
      preset.className = "preset-select";
      const blank = document.createElement("option");
      blank.value = "";
      blank.textContent = t("properties.customExpression");
      preset.append(blank);
      for (const template of customTemplates) {
        const option = document.createElement("option");
        option.value = template.name;
        option.textContent = templateLabel(template.name);
        preset.append(option);
      }
      preset.value =
        customTemplates.find((template) => template.expression === param)?.name ?? "";
      preset.addEventListener("change", () => {
        const template = customTemplates.find((candidate) => candidate.name === preset!.value);
        if (!template) return;
        value.value = template.expression;
        instance.params[input.name] = { Text: template.expression };
        const presetParams = new Set(["scale", "offset", "translation", "flag"]);
        for (const paramInput of inputsFor(instance)) {
          if (!presetParams.has(paramInput.name)) continue;
          const fallback = defaultParam(paramInput);
          if (fallback) instance.params[paramInput.name] = fallback;
        }
        for (const [name, param] of template.params ?? []) {
          instance.params[name] = structuredClone(param);
        }
        synchronizeVariableParams();
        commitHistory();
        renderProperties();
        scheduleBuild();
      });
      parent.append(preset);
    }
    const value = document.createElement("textarea");
    value.className = "text-area";
    value.rows = 5;
    value.spellcheck = false;
    value.value = param;
    value.addEventListener("input", () => {
      instance.params[input.name] = { Text: value.value };
      if (preset) preset.value = "";
      scheduleBuild();
    });
    value.addEventListener("change", commitHistory);
    parent.append(value);
    return;
  }

  const hint = document.createElement("p");
  hint.className = "hint";
  hint.textContent = t("properties.linkHint", { type: input.value_type });
  parent.append(hint);
}

function renderLink(parent: HTMLElement, instance: Instance, input: InputDef): void {
  parent.append(makeLabel(fieldLabel(input.name), input.required));
  const select = document.createElement("select");
  const none = document.createElement("option");
  none.value = "";
  none.textContent = t("properties.none");
  select.append(none);

  for (const candidate of linkCandidates(instance, input)) {
    const option = document.createElement("option");
    option.value = String(candidate.id);
    option.textContent = `${displayName(candidate)}  (#${candidate.id})`;
    select.append(option);
  }

  const linkedId = instance.inputs[input.name];
  if (linkedId !== undefined && ![...select.options].some((option) => option.value === String(linkedId))) {
    const invalid = document.createElement("option");
    invalid.value = String(linkedId);
    invalid.textContent = `#${linkedId} (${t("notebook.invalidSource")})`;
    select.append(invalid);
  }

  select.value = instance.inputs[input.name]?.toString() ?? "";
  select.addEventListener("change", () => {
    if (select.value === "") {
      disconnectInput(instance.id, input.name);
    } else {
      connectInput(instance.id, input.name, Number(select.value));
    }
  });

  const drop = document.createElement("div");
  drop.className = "drop-target";
  drop.textContent = t("properties.drop");
  drop.title = t("properties.dropHint");
  drop.addEventListener("dragover", (event) => {
    event.preventDefault();
    drop.classList.add("over");
  });
  drop.addEventListener("dragleave", () => drop.classList.remove("over"));
  drop.addEventListener("drop", (event) => {
    event.preventDefault();
    drop.classList.remove("over");
    const raw = event.dataTransfer?.getData("text/plain") ?? "";
    connectInput(instance.id, input.name, Number(raw));
  });

  const row = document.createElement("div");
  row.className = "link-row";
  row.dataset.instanceId = String(instance.id);
  row.dataset.input = input.name;
  row.append(select, drop);
  parent.append(row);
}

function renderProperties(): void {
  propertyBody!.replaceChildren();
  if (propertiesBadge) {
    propertiesBadge.textContent =
      selectedIds.size > 1
        ? t("properties.selectedCount", { count: selectedIds.size })
        : selectedId === null
          ? "—"
          : `#${selectedId}`;
  }
  if (selectedIds.size > 1) {
    const summary = document.createElement("div");
    summary.className = "selection-summary";
    const title = document.createElement("strong");
    title.textContent = t("batch.selected", { count: selectedIds.size });
    const actions = document.createElement("div");
    actions.className = "batch-actions";
    const addAction = (label: string, action: () => void, danger = false): void => {
      const button = document.createElement("button");
      button.textContent = label;
      if (danger) button.className = "danger";
      button.addEventListener("click", action);
      actions.append(button);
    };
    addAction(t("batch.duplicate"), duplicateSelected);
    addAction(t("batch.alignLeft"), () => alignSelected("left"));
    addAction(t("batch.alignTop"), () => alignSelected("top"));
    if (selectedIds.size >= 3) {
      addAction(t("batch.distributeH"), () => alignSelected("distribute-x"));
      addAction(t("batch.distributeV"), () => alignSelected("distribute-y"));
    }
    addAction(t("batch.enable"), () => setSelectedEnabled(true));
    addAction(t("batch.disable"), () => setSelectedEnabled(false));
    addAction(t("batch.delete"), deleteSelected, true);
    summary.append(title, actions);
    propertyBody!.append(summary);
  }
  const instance = selectedInstance();
  if (!instance) {
    const hint = document.createElement("p");
    hint.className = "hint";
    hint.textContent = t("properties.select");
    propertyBody!.append(hint);
    return;
  }

  const def = blockDef(instance.def_id);
  if (!def) {
    const hint = document.createElement("p");
    hint.className = "hint";
    hint.textContent = t("properties.unknown", { id: instance.def_id });
    propertyBody!.append(hint);
    return;
  }

  propertyBody!.append(makeLabel(t("properties.name"), false));
  const name = document.createElement("input");
  name.type = "text";
  name.value = instance.name;
  name.addEventListener("input", () => {
    instance.name = name.value;
    renderInstances();
  });
  name.addEventListener("change", commitHistory);
  propertyBody!.append(name);

  if (def.overloads.length > 1) {
    propertyBody!.append(makeLabel(t("properties.overload"), false));
    const select = document.createElement("select");
    for (const overload of def.overloads) {
      const option = document.createElement("option");
      option.value = overload.name;
      option.textContent = overload.name;
      select.append(option);
    }
    select.value = instance.overload ?? def.overloads[0]?.name ?? "";
    select.addEventListener("change", () => {
      const overload = def.overloads.find((candidate) => candidate.name === select.value);
      if (!overload) return;
      instance.overload = overload.name;
      instance.params = defaultsFor(overload);
      for (const inputName of Object.keys(instance.inputs)) {
        if (
          !isFreePortBlock(instance) &&
          !overload.inputs.some((input) => input.name === inputName)
        ) {
          delete instance.inputs[inputName];
        }
      }
      const refs = { ...instance.variable_refs };
      for (const [name, id] of Object.entries(refs)) {
        const input = inputsFor(instance).find((candidate) => candidate.name === name);
        const variable = variableById(id);
        if (!input || input.value_type !== "number" || !variable ||
            !variableMatchesInput(variable, input, numberParameter(instance, input).unit) ||
            convertQuantity(variable.value, variable.unit, numberParameter(instance, input).unit) === null) {
          delete refs[name];
        }
      }
      if (instance.variable_refs) instance.variable_refs = refs;
      synchronizeVariableParams();
      renderAll();
      commitHistory();
      scheduleBuild();
    });
    propertyBody!.append(select);
  }

  const description = document.createElement("p");
  description.className = "hint";
  description.textContent = blockDescription(def.id, def.description);
  propertyBody!.append(description);

  const overload = activeOverload(instance);
  if (!overload) {
    propertyBody!.append(description);
    return;
  }

  const overloadInputs = new Set(overload.inputs.map((input) => input.name));
  for (const input of inputsFor(instance)) {
    if (
      input.value_type === "field" ||
      input.value_type === "mesh" ||
      input.value_type === "list"
    ) {
      renderLink(propertyBody!, instance, input);
      if (isFreePortBlock(instance) && !overloadInputs.has(input.name)) {
        renderPortTools(propertyBody!, instance, input);
      }
    } else {
      renderParam(propertyBody!, instance, input);
    }
  }

  if (isFreePortBlock(instance)) {
    const addPort = document.createElement("button");
    addPort.className = "ghost";
    addPort.textContent =
      instance.def_id === "custom" ? t("properties.addField") : t("properties.addList");
    addPort.addEventListener("click", () => {
      const name = nextFreePortName(instance);
      instance.ports = [...(instance.ports ?? []), name];
      commitHistory();
      renderAll();
    });
    propertyBody!.append(addPort);
  }

  const divider = document.createElement("div");
  divider.className = "divider";
  propertyBody!.append(divider);

  const enabledRow = document.createElement("div");
  enabledRow.className = "inline";
  const enabled = document.createElement("input");
  enabled.type = "checkbox";
  enabled.checked = instance.enabled;
  enabled.addEventListener("change", () => {
    instance.enabled = enabled.checked;
    commitHistory();
    renderInstances();
    scheduleBuild();
  });
  const enabledLabel = document.createElement("span");
  enabledLabel.textContent = t("properties.enabled");
  enabledRow.append(enabled, enabledLabel);
  propertyBody!.append(enabledRow);

  const remove = document.createElement("button");
  remove.className = "danger";
  remove.textContent = t("properties.delete");
  remove.addEventListener("click", () => deleteInstance(instance.id));
  propertyBody!.append(remove);
}

function renderWires(): void {
  const world = graphWorld!;
  const svg = wires!;
  const width = Math.max(world.clientWidth, 1);
  const height = Math.max(world.clientHeight, 1);
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.setAttribute("width", String(width));
  svg.setAttribute("height", String(height));
  svg.replaceChildren();
  const worldRect = world.getBoundingClientRect();

  for (const instance of snapshot.instances) {
    const overload = activeOverload(instance);
    if (!overload) continue;
    for (const input of inputsFor(instance)) {
      if (
        input.value_type !== "field" &&
        input.value_type !== "mesh" &&
        input.value_type !== "list" &&
        input.value_type !== "any"
      ) {
        continue;
      }
      const sourceId = instance.inputs[input.name];
      if (sourceId === undefined) continue;
      const output = world.querySelector<HTMLElement>(
        `[data-port="out"][data-instance-id="${sourceId}"]`,
      );
      const port = world.querySelector<HTMLElement>(
        `[data-instance-id="${instance.id}"][data-input="${CSS.escape(input.name)}"]`,
      );
      if (!output || !port) continue;

      const from = output.getBoundingClientRect();
      const to = port.getBoundingClientRect();
      const x1 = (from.right - worldRect.left) / zoom;
      const y1 = (from.top + from.height / 2 - worldRect.top) / zoom;
      const x2 = (to.left - worldRect.left) / zoom;
      const y2 = (to.top + to.height / 2 - worldRect.top) / zoom;
      const curve = bezierWire({ x: x1, y: y1 }, { x: x2, y: y2 });
      const hit = document.createElementNS("http://www.w3.org/2000/svg", "path");
      hit.setAttribute("d", curve);
      hit.setAttribute("class", "wire-hit");
      const disconnect = (event: Event): void => {
        event.preventDefault();
        event.stopPropagation();
        disconnectInput(instance.id, input.name);
      };
      hit.addEventListener("dblclick", disconnect);
      hit.addEventListener("contextmenu", disconnect);
      const title = document.createElementNS("http://www.w3.org/2000/svg", "title");
      title.textContent = t("graph.disconnect");
      hit.append(title);
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute("d", curve);
      path.setAttribute("class", input.value_type === "mesh" ? "wire mesh" : "wire");
      svg.append(hit, path);
    }
  }
  if (activeGraphWire?.element.isConnected) {
    const anchor = graphPortAnchor(activeGraphWire.element, activeGraphWire.from);
    const from = activeGraphWire.from.kind === "output" ? anchor : activeGraphWire.pointer;
    const to = activeGraphWire.from.kind === "output" ? activeGraphWire.pointer : anchor;
    const preview = document.createElementNS("http://www.w3.org/2000/svg", "path");
    preview.setAttribute("d", bezierWire(from, to));
    preview.setAttribute("class", "wire-preview");
    svg.append(preview);
  }
}

let wireFrame: number | undefined;

function scheduleWires(): void {
  if (wireFrame !== undefined) return;
  wireFrame = requestAnimationFrame(() => {
    wireFrame = undefined;
    renderWires();
  });
}

function renderAll(): void {
  renderInstances();
  renderVariables();
  renderProperties();
}

function decodeMesh(bytes: Uint8Array): {
  geometry: THREE.BufferGeometry;
  vertices: number;
  triangles: number;
  featureEdges: Uint32Array;
} {
  const copy = bytes.slice();
  const view = new DataView(copy.buffer);
  const magic = String.fromCharCode(copy[0] ?? 0, copy[1] ?? 0, copy[2] ?? 0, copy[3] ?? 0);
  if (magic !== "ZSM2") throw new Error(`unexpected mesh magic: ${magic}`);

  const vertices = view.getUint32(4, true);
  const triangles = view.getUint32(8, true);
  const featureCount = view.getUint32(12, true);
  const positionOffset = 16;
  const normalOffset = positionOffset + vertices * 12;
  const indexOffset = normalOffset + vertices * 12;
  const featureOffset = indexOffset + triangles * 12;

  const positions = new Float32Array(copy.buffer, positionOffset, vertices * 3);
  const normals = new Float32Array(copy.buffer, normalOffset, vertices * 3);
  const indices = new Uint32Array(copy.buffer, indexOffset, triangles * 3);
  const featureEdges = new Uint32Array(copy.buffer, featureOffset, featureCount * 2);

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.BufferAttribute(normals, 3));
  geometry.setIndex(new THREE.BufferAttribute(indices, 1));
  geometry.computeBoundingBox();
  geometry.computeBoundingSphere();
  return { geometry, vertices, triangles, featureEdges };
}

function buildFeatureGeometry(
  geometry: THREE.BufferGeometry,
  edges: Uint32Array,
): THREE.BufferGeometry {
  const position = geometry.getAttribute("position");
  const result = new THREE.BufferGeometry();
  if (!position || edges.length === 0) return result;
  const lines = new Float32Array(edges.length * 3);
  for (let index = 0; index < edges.length; index += 1) {
    const vertex = edges[index]!;
    lines[index * 3] = position.getX(vertex);
    lines[index * 3 + 1] = position.getY(vertex);
    lines[index * 3 + 2] = position.getZ(vertex);
  }
  result.setAttribute("position", new THREE.BufferAttribute(lines, 3));
  return result;
}

function autoMeshTarget(): Instance | undefined {
  const selected = selectedInstance();
  if (selected && outputType(selected) === "mesh") return selected;
  return [...snapshot.instances].reverse().find((instance) => outputType(instance) === "mesh");
}

function meshTarget(): Instance | undefined {
  if (previewTargetId !== null) {
    const pinned = instanceById(previewTargetId);
    if (pinned && outputType(pinned) === "mesh") return pinned;
  }
  return autoMeshTarget();
}

function clearMeshPreview(): void {
  ++buildToken;
  window.clearTimeout(buildTimer);
  if (currentMesh) scene.remove(currentMesh);
  if (currentFeatures) {
    scene.remove(currentFeatures);
    currentFeatures.geometry.dispose();
  }
  currentGeometry?.dispose();
  currentMesh = null;
  currentFeatures = null;
  currentGeometry = null;
  framed = false;
  errorId = null;
  setDiagnostics(null);
}

function build(): void {
  const target = meshTarget();
  if (!target) {
    clearMeshPreview();
    setLocalizedStatus("status.noMesh");
    return;
  }

  const token = ++buildToken;
  setLocalizedStatus("status.building");
  requestAnimationFrame(() => {
    if (token !== buildToken) return;
    const started = performance.now();
    try {
      const bytes = notebook_eval(kernelSnapshotJson(), BigInt(target.id));
      const { geometry, vertices, triangles, featureEdges } = decodeMesh(bytes);
      if (currentMesh) scene.remove(currentMesh);
      if (currentFeatures) {
        scene.remove(currentFeatures);
        currentFeatures.geometry.dispose();
      }
      currentGeometry?.dispose();
      currentGeometry = geometry;
      currentMesh = new THREE.Mesh(geometry, material);
      scene.add(currentMesh);
      currentFeatures = new THREE.LineSegments(
        buildFeatureGeometry(geometry, featureEdges),
        featureMaterial,
      );
      currentFeatures.visible = featureToggle!.checked;
      scene.add(currentFeatures);
      scene.updateMatrixWorld(true);

      const box = geometry.boundingBox;
      if (box) {
        const center = box.getCenter(new THREE.Vector3());
        currentMesh.position.copy(center).multiplyScalar(-1);
        currentFeatures.position.copy(currentMesh.position);
        if (!framed) {
          const size = box.getSize(new THREE.Vector3());
          const sphereRadius = Math.max(size.length() * 0.5, 1);
          const verticalFov = THREE.MathUtils.degToRad(camera.fov);
          const fitVertical = sphereRadius / Math.max(Math.sin(verticalFov / 2), 0.01);
          const fitHorizontal = fitVertical / Math.max(Math.min(1, camera.aspect), 0.1);
          const distance = Math.max(fitVertical, fitHorizontal) * 1.05;
          const direction = new THREE.Vector3(1, 0.65, 1.1).normalize();
          camera.position.copy(direction.multiplyScalar(distance));
          camera.near = Math.max(distance / 500, 0.01);
          camera.far = distance * 20;
          camera.updateProjectionMatrix();
          controls.target.set(0, 0, 0);
          controls.update();
          framed = true;
        }
      }

      errorId = null;
      setDiagnostics(null);
      hideToast();
      const elapsed = performance.now() - started;
      setLocalizedStatus("status.stats", {
        verts: vertices.toLocaleString(),
        tris: triangles.toLocaleString(),
        ms: elapsed.toFixed(1),
        version: version(),
      });
    } catch (error) {
      console.error(error);
      errorId = target.id;
      const message = String(error);
      setDiagnostics(message, readDiagnostics(target.id));
      setLocalizedStatus("status.buildFailed", { error: message.split("\n")[0] ?? message });
      showToast(t("toast.buildFailed", { error: message }));
    }
    renderInstances();
  });
}

function scheduleBuild(): void {
  if (!autoUpdate) {
    setLocalizedStatus("status.manual");
    return;
  }
  setLocalizedStatus("status.dirty");
  window.clearTimeout(buildTimer);
  buildTimer = window.setTimeout(build, 260);
}

function syncPolicy(): void {
  autoUpdate = snapshot.update_policy !== "manual";
  autoToggle!.checked = autoUpdate;
}

function saveJson(): void {
  flushNotebookHistory();
  synchronizeVariableParams();
  const blob = new Blob([JSON.stringify({ ...snapshot, graph_layout: exportGraphLayout(nodePositions) }, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = "zeroset-notebook.json";
  anchor.click();
  URL.revokeObjectURL(url);
  showToast(t("toast.saved"));
}

async function loadJson(file: File): Promise<void> {
  try {
    const parsed = JSON.parse(await file.text()) as Snapshot;
    if (parsed.version !== 1 || !Array.isArray(parsed.instances)) {
      throw new Error("expected a version 1 Notebook snapshot");
    }
    validateVariableSnapshot(parsed);
    const savedPositions = importGraphLayout(parsed.graph_layout, new Set(parsed.instances.map((instance) => instance.id)));
    flushNotebookHistory();
    snapshot = parsed;
    synchronizeVariableParams();
    impactedVariableId = null;
    previewTargetId = null;
    selectedId = snapshot.instances[0]?.id ?? null;
    selectedIds = new Set(selectedId === null ? [] : [selectedId]);
    framed = false;
    errorId = null;
    nodePositions.clear();
    for (const [id, position] of savedPositions) nodePositions.set(id, position);
    syncPolicy();
    renderAll();
    fitCanvas();
    resetHistory();
    build();
  } catch (error) {
    showToast(t("toast.loadFailed", { error: String(error) }));
  }
}

async function main(): Promise<void> {
  initTheme();
  initLanguage();
  applyViewportTheme(currentTheme());
  syncLanguageThemeControls();
  onThemeChange((theme) => {
    applyViewportTheme(theme);
    syncLanguageThemeControls();
  });
  onLanguageChange(() => {
    syncLanguageThemeControls();
    if (statusState) statusLabel!.textContent = t(statusState.key, statusState.vars);
    renderAll();
    if (!graphQuickAdd!.classList.contains("hidden")) renderGraphQuickResults();
    syncWorkspaceModeControls();
  });
  languageButton!.addEventListener("click", () => toggleLanguage());
  themeButton!.addEventListener("click", () => toggleTheme());

  await init(wasmUrl);
  catalog = JSON.parse(block_catalog()) as BlockDef[];
  customTemplates = JSON.parse(custom_templates()) as CustomTemplate[];
  snapshot = JSON.parse(notebook_template()) as Snapshot;
  synchronizeVariableParams();
  selectedId = snapshot.instances[snapshot.instances.length - 1]?.id ?? null;
  selectedIds = new Set(selectedId === null ? [] : [selectedId]);
  syncPolicy();

  try {
    workspaceMode = localStorage.getItem(WORKSPACE_MODE_KEY) === "graph" ? "graph" : "notebook";
  } catch (error) {
    console.warn("无法恢复工作区模式", error);
  }
  syncWorkspaceModeControls();
  showNotebookButton!.addEventListener("click", () => setWorkspaceMode("notebook"));
  showGraphButton!.addEventListener("click", () => setWorkspaceMode("graph"));
  graphExpandButton!.addEventListener("click", () => setGraphExpanded(!graphExpanded));
  graphPropertiesButton!.addEventListener("click", () => setGraphPropertiesOpen(!graphPropertiesOpen));
  notebookAddButton!.addEventListener("click", () => {
    if (workspaceMode === "graph") {
      const rect = notebookBody!.getBoundingClientRect();
      openGraphQuickAdd(rect.left + rect.width / 2, rect.top + Math.min(180, rect.height / 2));
      return;
    }
    if (!document.body.classList.contains("show-palette")) togglePaletteButton?.click();
    blockSearch?.focus();
  });
  graphQuickSearch!.addEventListener("input", renderGraphQuickResults);
  graphQuickSearch!.addEventListener("keydown", (event) => {
    const options = [...graphQuickResults!.querySelectorAll<HTMLButtonElement>(".graph-quick-option")];
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeGraphQuickAdd();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      options.find((option) => option.classList.contains("active"))?.click();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const index = options.findIndex((option) => option.classList.contains("active"));
    const delta = event.key === "ArrowDown" ? 1 : -1;
    const next = (index + delta + options.length) % options.length;
    options.forEach((option, at) => {
      option.classList.toggle("active", at === next);
      option.setAttribute("aria-selected", String(at === next));
    });
    options[next]?.scrollIntoView({ block: "nearest" });
  });
  previewTargetSelect!.addEventListener("change", () => {
    previewTargetId = previewTargetSelect!.value === "" ? null : Number(previewTargetSelect!.value);
    framed = false;
    renderPreviewTargets();
    build();
  });

  buildButton!.addEventListener("click", build);
  undoButton!.addEventListener("click", undo);
  redoButton!.addEventListener("click", redo);
  autoToggle!.addEventListener("change", () => {
    autoUpdate = autoToggle!.checked;
    snapshot.update_policy = autoUpdate ? "auto" : "manual";
    commitHistory();
    window.clearTimeout(buildTimer);
    if (autoUpdate) {
      build();
    } else {
      setLocalizedStatus("status.manual");
    }
  });
  featureToggle!.addEventListener("change", () => {
    if (currentFeatures) currentFeatures.visible = featureToggle!.checked;
  });
  templateButton!.addEventListener("click", () => {
    flushNotebookHistory();
    snapshot = JSON.parse(notebook_template()) as Snapshot;
    synchronizeVariableParams();
    impactedVariableId = null;
    previewTargetId = null;
    selectedId = snapshot.instances[snapshot.instances.length - 1]?.id ?? null;
    selectedIds = new Set(selectedId === null ? [] : [selectedId]);
    framed = false;
    errorId = null;
    nodePositions.clear();
    syncPolicy();
    renderAll();
    fitCanvas();
    resetHistory();
    build();
  });
  newButton!.addEventListener("click", () => {
    flushNotebookHistory();
    snapshot = { version: 1, next_id: 1, instances: [], update_policy: "auto" };
    synchronizeVariableParams();
    impactedVariableId = null;
    previewTargetId = null;
    clearMeshPreview();
    selectedId = null;
    selectedIds.clear();
    errorId = null;
    framed = false;
    nodePositions.clear();
    syncPolicy();
    renderAll();
    fitCanvas();
    resetHistory();
    setLocalizedStatus("status.empty");
  });
  saveButton!.addEventListener("click", saveJson);
  loadButton!.addEventListener("click", () => loadFile!.click());
  loadFile!.addEventListener("change", () => {
    const file = loadFile!.files?.[0];
    if (file) void loadJson(file);
    loadFile!.value = "";
  });

  blockSearch?.addEventListener("input", renderPalette);
  blockSearch?.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    blockSearch.value = "";
    blockSearch.blur();
    renderPalette();
  });
  toggleVariablesButton!.addEventListener("click", () => {
    showVariablesPanel(!document.body.classList.contains("show-variables"));
  });
  variableAddButton!.addEventListener("click", () => addNamedVariable());
  togglePaletteButton?.addEventListener("click", () => {
    const open = document.body.classList.toggle("show-palette");
    togglePaletteButton.classList.toggle("active", open);
    if (open) {
      showVariablesPanel(false);
      document.body.classList.remove("show-properties");
      togglePropertiesButton?.classList.remove("active");
    }
  });
  togglePropertiesButton?.addEventListener("click", () => {
    const open = document.body.classList.toggle("show-properties");
    togglePropertiesButton.classList.toggle("active", open);
    if (open) {
      showVariablesPanel(false);
      document.body.classList.remove("show-palette");
      togglePaletteButton?.classList.remove("active");
    }
  });
  restorePanelLayout();
  bindPanelResizers();

  renderAll();
  resize();
  new ResizeObserver(() => {
    resize();
    updateCanvasSize();
    scheduleWires();
  }).observe(viewport!);
  window.addEventListener("resize", () => {
    refreshLayout();
  });

  notebook!.addEventListener(
    "wheel",
    (event) => {
      if (workspaceMode !== "graph") return;
      event.preventDefault();
      const rect = notebookBody!.getBoundingClientRect();
      setZoomAt(
        event.clientX - rect.left,
        event.clientY - rect.top,
        zoom * Math.exp(-event.deltaY * 0.0014),
      );
    },
    { passive: false },
  );
  notebook!.addEventListener("pointerdown", (event) => {
    if (workspaceMode !== "graph") return;
    const target = event.target as Element;
    if (target.closest("#graph-quick-add")) return;
    if (event.button === 0) {
      if (notebookBody!.contains(target) && !target.closest(".instance, .wire-hit")) {
        startMarquee(event);
      }
    }
    if (event.button !== 1) return;
    event.preventDefault();
    panStart = {
      x: event.clientX,
      y: event.clientY,
      panX,
      panY,
    };
    notebook!.setPointerCapture(event.pointerId);
    notebook!.classList.add("panning");
  });
  notebook!.addEventListener("pointermove", (event) => {
    if (workspaceMode !== "graph") return;
    if (!panStart) return;
    panX = panStart.panX + event.clientX - panStart.x;
    panY = panStart.panY + event.clientY - panStart.y;
    applyView();
  });
  const endPan = (): void => {
    panStart = null;
    notebook!.classList.remove("panning");
  };
  notebook!.addEventListener("pointerup", endPan);
  notebook!.addEventListener("pointercancel", endPan);
  notebook!.addEventListener("contextmenu", (event) => {
    if (workspaceMode !== "graph") return;
    event.preventDefault();
    const target = event.target as Element;
    if (target.closest(".instance, .wire-hit, #graph-quick-add")) return;
    openGraphQuickAdd(event.clientX, event.clientY);
  });
  window.addEventListener("pointerdown", (event) => {
    if (graphQuickAdd!.classList.contains("hidden")) return;
    if (!graphQuickAdd!.contains(event.target as Node)) closeGraphQuickAdd();
  });
  zoomIn!.addEventListener("click", () => zoomAtCenter(1.15));
  zoomOut!.addEventListener("click", () => zoomAtCenter(1 / 1.15));
  fitButton!.addEventListener("click", fitCanvas);
  arrangeButton!.addEventListener("click", () => {
    commitHistory();
    nodePositions.clear();
    layoutNodes();
    renderInstances();
    scheduleWires();
    fitCanvas();
    commitHistory();
  });

  window.addEventListener("keydown", (event) => {
    const target = event.target as HTMLElement | null;
    const editing = Boolean(target?.closest("input, textarea, select"));
    if (event.key === "Escape" && cancelGraphWireDrag) {
      event.preventDefault();
      cancelGraphWireDrag();
      return;
    }
    if (event.key === "Escape" && !graphQuickAdd!.classList.contains("hidden")) {
      event.preventDefault();
      closeGraphQuickAdd();
      return;
    }
    if (event.key === "Escape" && graphPropertiesOpen) {
      event.preventDefault();
      setGraphPropertiesOpen(false);
      return;
    }
    if (event.key === "Escape" && document.body.classList.contains("show-variables")) {
      event.preventDefault();
      showVariablesPanel(false);
      return;
    }
    if (event.key === "Escape" && graphExpanded) {
      event.preventDefault();
      setGraphExpanded(false);
      return;
    }
    if (!editing && event.key === "/") {
      event.preventDefault();
      if (!document.body.classList.contains("show-palette")) togglePaletteButton?.click();
      blockSearch?.focus();
      blockSearch?.select();
      return;
    }
    if (!editing && workspaceMode === "graph" && !event.ctrlKey && !event.metaKey &&
        event.key.toLowerCase() === "a") {
      event.preventDefault();
      const rect = notebookBody!.getBoundingClientRect();
      openGraphQuickAdd(rect.left + rect.width / 2, rect.top + rect.height / 2);
      return;
    }
    if (!editing && (event.key === "Delete" || event.key === "Backspace")) {
      event.preventDefault();
      deleteSelected();
      return;
    }
    if (!(event.ctrlKey || event.metaKey)) return;
    const key = event.key.toLowerCase();
    if (!editing && key === "d") {
      event.preventDefault();
      duplicateSelected();
      return;
    }
    if (editing) return;
    if (key === "z") {
      event.preventDefault();
      if (event.shiftKey) redo();
      else undo();
    } else if (key === "y") {
      event.preventDefault();
      redo();
    }
  });

  fitCanvas();
  applyView();
  resetHistory();
  setLocalizedStatus("status.initialize", { version: version(), count: catalog.length });
  build();
  animate();
}

void main();
