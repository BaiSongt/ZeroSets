import assert from "node:assert/strict";
import test from "node:test";
import { bezierWire, exportGraphLayout, graphPointAt, importGraphLayout, planGraphConnection } from "../src/graph-editor.ts";

const nodes = [
  { id: 1, inputs: {} },
  { id: 2, inputs: { a: 1 } },
  { id: 3, inputs: {} },
];
const output = (nodeId, valueType = "field") => ({ kind: "output", nodeId, valueType });
const input = (nodeId, name = "a", valueType = "field") => ({ kind: "input", nodeId, name, valueType });

test("Graph 双向拖线都生成 target.input -> source 的同一连接", () => {
  const expected = { ok: true, connection: { targetId: 3, inputName: "a", sourceId: 1 } };
  assert.deepEqual(planGraphConnection(nodes, output(1), input(3)), expected);
  assert.deepEqual(planGraphConnection(nodes, input(3), output(1)), expected);
});

test("Graph 连线预览拒绝不匹配类型、同向端口和循环依赖", () => {
  assert.deepEqual(planGraphConnection(nodes, output(1, "mesh"), input(3)), { ok: false, reason: "type" });
  assert.deepEqual(planGraphConnection(nodes, output(1), output(3)), { ok: false, reason: "direction" });
  assert.deepEqual(planGraphConnection(nodes, output(2), input(1)), { ok: false, reason: "cycle" });
  assert.deepEqual(planGraphConnection(nodes, output(1), input(1)), { ok: false, reason: "cycle" });
});

test("画布坐标在缩放和平移后仍能精确落点", () => {
  assert.deepEqual(graphPointAt(410, 310, 10, 10, 100, 50, 2), { x: 150, y: 125 });
  assert.deepEqual(graphPointAt(110, 60, 10, 10, 100, 50, 0.5), { x: 0, y: 0 });
});

test("预览边与正式边共享曲线路径，并允许从右向左回连", () => {
  assert.equal(bezierWire({ x: 10, y: 20 }, { x: 100, y: 50 }), "M 10 20 C 50.5 20, 59.5 50, 100 50");
  assert.equal(bezierWire({ x: 100, y: 20 }, { x: 10, y: 50 }), "M 100 20 C 140.5 20, -30.5 50, 10 50");
});

test("Graph 节点坐标随 JSON 保存/加载恢复，且兼容没有布局的旧文件", () => {
  const original = new Map([[1, { x: 120.5, y: 30 }], [3, { x: 450, y: 210 }]]);
  const encoded = JSON.parse(JSON.stringify(exportGraphLayout(original)));
  assert.deepEqual(importGraphLayout(encoded, new Set([1, 3])), original);
  assert.deepEqual(importGraphLayout(undefined, new Set([1])), new Map());
  assert.deepEqual(importGraphLayout(encoded, new Set([1])), new Map([[1, { x: 120.5, y: 30 }]]));
});

test("Graph JSON 布局拒绝无限值、越界坐标和非法节点 ID", () => {
  assert.throws(() => importGraphLayout({ 1: { x: -1, y: 2 } }, new Set([1])));
  assert.throws(() => importGraphLayout({ 1: { x: 1e101, y: 2 } }, new Set([1])));
  assert.throws(() => importGraphLayout({ notId: { x: 1, y: 2 } }, new Set([1])));
});
