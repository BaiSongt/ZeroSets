import assert from "node:assert/strict";
import test from "node:test";
import { affectedBlockIds, convertQuantity, materializeBoundNumbers, unitFamily, validVariableName } from "../src/variables.ts";

test("长度变量按内核 mm 基准换算", () => {
  assert.equal(convertQuantity(2, "centimeter", "millimeter"), 20);
  assert.equal(convertQuantity(1, "inch", "millimeter"), 25.4);
  assert.equal(convertQuantity(1000, "millimeter", "meter"), 1);
});

test("角度与无量纲独立，拒绝跨量纲引用", () => {
  assert.ok(Math.abs(convertQuantity(180, "degree", "radian") - Math.PI) < 1e-10);
  assert.equal(convertQuantity(1, "degree", "millimeter"), null);
  assert.equal(convertQuantity(NaN, "unitless", "unitless"), null);
  assert.equal(unitFamily("inch"), "length");
  assert.equal(convertQuantity(1e100, "millimeter", "meter"), null);
  assert.equal(convertQuantity(1e38, "radian", "degree"), null);
});

test("同一变量驱动多个 Block，并且 JSON 往返后绑定信息不丢失", () => {
  const data = {
    version: 1,
    variables: [{ id: 6, name: "壁厚", value: 2.5, unit: "centimeter" }],
    instances: [
      { id: 1, inputs: {}, variable_refs: { radius: 6 }, params: { radius: { Number: { value: 1, unit: "millimeter" } } } },
      { id: 2, inputs: {}, variable_refs: { radius: 6 }, params: { radius: { Number: { value: 1, unit: "centimeter" } } } },
    ],
  };
  assert.equal(materializeBoundNumbers(data.instances, data.variables), true);
  assert.equal(data.instances[0].params.radius.Number.value, 25);
  assert.equal(data.instances[1].params.radius.Number.value, 2.5);
  const restored = JSON.parse(JSON.stringify(data));
  restored.variables[0].value = 3;
  assert.equal(materializeBoundNumbers(restored.instances, restored.variables), true);
  assert.equal(restored.instances[0].params.radius.Number.value, 30);
  assert.deepEqual(restored.instances.map((node) => node.variable_refs.radius), [6, 6]);
});

test("中文/英文变量名可以使用，但不能以数字开头或包含空格", () => {
  assert.equal(validVariableName("wallThickness"), true);
  assert.equal(validVariableName("壁厚_1"), true);
  assert.equal(validVariableName("1radius"), false);
  assert.equal(validVariableName("wall thickness"), false);
});

test("变量变更高亮全部下游，且不影响无关 Block", () => {
  const nodes = [
    { id: 1, inputs: {}, variable_refs: { radius: 7 } },
    { id: 2, inputs: {} },
    { id: 3, inputs: { a: 1, b: 2 } },
    { id: 4, inputs: { field: 3 } },
    { id: 5, inputs: {} },
  ];
  assert.deepEqual([...affectedBlockIds(nodes, 7)].sort((a, b) => a - b), [1, 3, 4]);
  assert.deepEqual([...affectedBlockIds(nodes, 12)], []);
});
