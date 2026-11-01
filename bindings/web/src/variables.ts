// UI 命名变量层。所有值最终仍解析为 Rust 内核已有的 Number + Unit。
export type VariableUnit =
  | "unitless" | "millimeter" | "centimeter" | "meter" | "inch" | "radian" | "degree";

export type NamedVariable = { id: number; name: string; value: number; unit: VariableUnit; expression?: string };
export type BoundNode = {
  id: number;
  inputs: Record<string, number>;
  variable_refs?: Record<string, number>;
};
export type BoundNumericNode = BoundNode & { params: Record<string, unknown> };

const MAX_F32 = 3.4028234663852886e38;

const baseFactor: Record<VariableUnit, number> = {
  unitless: 1,
  millimeter: 1,
  centimeter: 10,
  meter: 1000,
  inch: 25.4,
  radian: 1,
  degree: Math.PI / 180,
};

export function unitFamily(unit: VariableUnit): "unitless" | "length" | "angle" {
  if (unit === "unitless") return "unitless";
  return unit === "radian" || unit === "degree" ? "angle" : "length";
}

export function convertQuantity(value: number, from: VariableUnit, to: VariableUnit): number | null {
  if (!Number.isFinite(value) || unitFamily(from) !== unitFamily(to)) return null;
  const base = value * baseFactor[from];
  if (!Number.isFinite(base) || Math.abs(base) > MAX_F32) return null;
  const result = base / baseFactor[to];
  return Number.isFinite(result) && Math.abs(result) <= MAX_F32 ? result : null;
}

// 兼容 v1 快照：把变量引用解析为实际 Number 参数，不把变量对象送入 WASM。
export function materializeBoundNumbers(
  nodes: readonly BoundNumericNode[],
  variables: readonly NamedVariable[],
): boolean {
  const byId = new Map(variables.map((variable) => [variable.id, variable]));
  for (const node of nodes) {
    for (const [name, id] of Object.entries(node.variable_refs ?? {})) {
      const variable = byId.get(id);
      const param = node.params[name];
      if (!variable || !param || typeof param !== "object" || !("Number" in param)) return false;
      const number = (param as { Number: { value: number; unit: VariableUnit } }).Number;
      const converted = convertQuantity(variable.value, variable.unit, number.unit);
      if (converted === null) return false;
      node.params[name] = { Number: { value: converted, unit: number.unit } };
    }
  }
  return true;
}

export function validVariableName(name: string): boolean {
  return /^[\p{L}_][\p{L}\p{N}_]*$/u.test(name);
}

// 命名变量影响直接绑定的 Block 及全部下游依赖；DAG 异常时也能安全终止。
export function affectedBlockIds(nodes: readonly BoundNode[], variableIds: number | ReadonlySet<number>): Set<number> {
  const ids = typeof variableIds === "number" ? new Set([variableIds]) : variableIds;
  const affected = new Set<number>();
  const pending: number[] = [];
  for (const node of nodes) {
    if (Object.values(node.variable_refs ?? {}).some((id) => ids.has(id))) {
      affected.add(node.id);
      pending.push(node.id);
    }
  }
  const consumers = new Map<number, number[]>();
  for (const node of nodes) {
    for (const sourceId of Object.values(node.inputs)) {
      const list = consumers.get(sourceId) ?? [];
      list.push(node.id);
      consumers.set(sourceId, list);
    }
  }
  while (pending.length > 0) {
    const id = pending.pop()!;
    for (const dependent of consumers.get(id) ?? []) {
      if (affected.has(dependent)) continue;
      affected.add(dependent);
      pending.push(dependent);
    }
  }
  return affected;
}
