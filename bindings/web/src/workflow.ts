// 纯数据层连接规则：Notebook、Graph、Inspector 共用，且可独立于 WASM 测试。
export type LinkNode = { id: number; inputs: Record<string, number> };
export type LinkSlot = { name: string; value_type: string };
export type LinkSource = { id: number; output: string };

export function acceptsOutput(inputType: string, outputType: string): boolean {
  return inputType === "any" || inputType === outputType;
}

// 新连接 target -> source 不能让 source 已经反向依赖 target。
export function createsCycle(nodes: readonly LinkNode[], targetId: number, sourceId: number): boolean {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const visited = new Set<number>();
  const pending = [sourceId];
  while (pending.length > 0) {
    const id = pending.pop()!;
    if (id === targetId) return true;
    if (visited.has(id)) continue;
    visited.add(id);
    const node = byId.get(id);
    if (node) pending.push(...Object.values(node.inputs));
  }
  return false;
}

// 按所选来源顺序填充可用的图端口；普通添加 Block 不调用此规划器。
export function planAutoConnections(
  slots: readonly LinkSlot[],
  sources: readonly LinkSource[],
): Record<string, number> {
  const result: Record<string, number> = {};
  const used = new Set<number>();
  for (const slot of slots) {
    if (!["field", "mesh", "list", "any"].includes(slot.value_type)) continue;
    const candidate = sources.find(
      (source) => !used.has(source.id) && acceptsOutput(slot.value_type, source.output),
    );
    if (!candidate) continue;
    result[slot.name] = candidate.id;
    used.add(candidate.id);
  }
  return result;
}
