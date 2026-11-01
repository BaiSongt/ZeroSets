import { acceptsOutput, createsCycle, type LinkNode } from "./workflow.ts";

export type GraphPoint = { x: number; y: number };
export type GraphPort =
  | { kind: "output"; nodeId: number; valueType: string }
  | { kind: "input"; nodeId: number; name: string; valueType: string };
export type GraphConnection = { targetId: number; inputName: string; sourceId: number };
export type GraphConnectionPlan =
  | { ok: true; connection: GraphConnection }
  | { ok: false; reason: "direction" | "type" | "cycle" };

// Graph 位置使用世界坐标，缩放和平移都不会改变节点的实际位置。
export function graphPointAt(
  clientX: number, clientY: number,
  left: number, top: number, panX: number, panY: number, zoom: number,
): GraphPoint {
  return {
    x: (clientX - left - panX) / zoom,
    y: (clientY - top - panY) / zoom,
  };
}

export function bezierWire(from: GraphPoint, to: GraphPoint): string {
  const control = Math.max(28, Math.abs(to.x - from.x) * 0.45);
  return `M ${from.x} ${from.y} C ${from.x + control} ${from.y}, ${to.x - control} ${to.y}, ${to.x} ${to.y}`;
}

// Graph layout is optional UI metadata in v1 JSON and is never forwarded to WASM.
export function exportGraphLayout(positions: ReadonlyMap<number, GraphPoint>): Record<string, GraphPoint> {
  return Object.fromEntries([...positions].map(([id, point]) => [String(id), { ...point }]));
}

export function importGraphLayout(layout: unknown, ids: ReadonlySet<number>): Map<number, GraphPoint> {
  const positions = new Map<number, GraphPoint>();
  if (layout === undefined) return positions;
  if (typeof layout !== "object" || layout === null || Array.isArray(layout)) throw new Error("Invalid graph layout");
  for (const [key, point] of Object.entries(layout)) {
    const id = Number(key);
    if (!Number.isSafeInteger(id) || id <= 0 || !point || typeof point !== "object" || Array.isArray(point)) {
      throw new Error("Invalid graph layout");
    }
    const { x, y } = point as { x?: unknown; y?: unknown };
    if (typeof x !== "number" || typeof y !== "number" || !Number.isFinite(x) || !Number.isFinite(y) ||
        x < 0 || y < 0 || x > 100_000 || y > 100_000) throw new Error("Invalid graph position");
    if (ids.has(id)) positions.set(id, { x, y });
  }
  return positions;
}

// 输入或输出都可以作为拖线起点；同一套校验用于预览和正式落点。
export function planGraphConnection(
  nodes: readonly LinkNode[], first: GraphPort, second: GraphPort,
): GraphConnectionPlan {
  const source = first.kind === "output" ? first : second.kind === "output" ? second : null;
  const target = first.kind === "input" ? first : second.kind === "input" ? second : null;
  if (!source || !target) return { ok: false, reason: "direction" };
  if (!acceptsOutput(target.valueType, source.valueType)) return { ok: false, reason: "type" };
  if (createsCycle(nodes, target.nodeId, source.nodeId)) return { ok: false, reason: "cycle" };
  return { ok: true, connection: { targetId: target.nodeId, inputName: target.name, sourceId: source.nodeId } };
}
