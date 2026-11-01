import assert from "node:assert/strict";
import test from "node:test";
import { acceptsOutput, createsCycle, planAutoConnections } from "../src/workflow.ts";

test("连接要求输出类型匹配，any 接受任意类型", () => {
  assert.equal(acceptsOutput("field", "field"), true);
  assert.equal(acceptsOutput("field", "mesh"), false);
  assert.equal(acceptsOutput("any", "mesh"), true);
});

test("禁止直接和间接循环依赖，但允许正常的 DAG 共享", () => {
  const nodes = [
    { id: 1, inputs: {} },
    { id: 2, inputs: { field: 1 } },
    { id: 3, inputs: { field: 2 } },
    { id: 4, inputs: { a: 1, b: 2 } },
  ];
  assert.equal(createsCycle(nodes, 1, 1), true);
  assert.equal(createsCycle(nodes, 1, 3), true);
  assert.equal(createsCycle(nodes, 2, 3), true);
  assert.equal(createsCycle(nodes, 3, 1), false);
  assert.equal(createsCycle(nodes, 4, 2), false);
});

test("选中两个 Field 后可一键连接到并集 a/b", () => {
  const plan = planAutoConnections(
    [{ name: "a", value_type: "field" }, { name: "b", value_type: "field" }],
    [{ id: 11, output: "field" }, { id: 12, output: "field" }],
  );
  assert.deepEqual(plan, { a: 11, b: 12 });
});

test("自动连接不会复用来源，也不会把 Field 填入 Mesh 或数值参数", () => {
  const plan = planAutoConnections(
    [
      { name: "a", value_type: "field" },
      { name: "b", value_type: "field" },
      { name: "mesh", value_type: "mesh" },
      { name: "radius", value_type: "number" },
    ],
    [{ id: 7, output: "field" }, { id: 9, output: "mesh" }],
  );
  assert.deepEqual(plan, { a: 7, mesh: 9 });
});
