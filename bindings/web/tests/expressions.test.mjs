import assert from "node:assert/strict";
import test from "node:test";
import { dependentVariableIds, FormulaError, renameFormulaReference, resolveVariableExpressions } from "../src/expressions.ts";
import { affectedBlockIds, materializeBoundNumbers } from "../src/variables.ts";

const variable = (id, name, value, unit = "millimeter", expression) => ({ id, name, value, unit, ...(expression ? { expression } : {}) });
const resolved = (variables) => resolveVariableExpressions(variables).values;
const fails = (variables, code) => assert.throws(() => resolveVariableExpressions(variables), (error) =>
  error instanceof FormulaError && error.code === code);

test("公式遵循算术优先级、括号与负号", () => {
  assert.equal(resolved([variable(1, "result", 0, "unitless", "2 + 3 * (4 - 1)")]).get(1), 11);
  assert.equal(resolved([variable(1, "result", 0, "unitless", "-(2 + 3) / 2")]).get(1), -2.5);
  assert.equal(resolved([variable(1, "result", 0, "unitless", "1.5e2 / 3")]).get(1), 50);
});

test("公式允许中文变量名、长度单位字面量及不同长度单位引用", () => {
  const result = resolved([
    variable(1, "基准半径", 12),
    variable(2, "壁厚", 0.2, "centimeter"),
    variable(3, "外半径", 0, "millimeter", "= 基准半径 + 壁厚 + 3 mm"),
    variable(4, "加倍", 0, "centimeter", "外半径 * 2"),
  ]);
  assert.equal(result.get(3), 17);
  assert.equal(result.get(4), 3.4);
  assert.equal(resolved([variable(1, "𝑟", 3, "unitless"), variable(2, "diameter", 0, "unitless", "𝑟 * 2")]).get(2), 6);
});

test("公式支持相同量纲相除得到无量纲，禁止平方或倒数长度", () => {
  assert.equal(resolved([variable(1, "ratio", 0, "unitless", "2 cm / 5 mm")]).get(1), 4);
  fails([variable(1, "area", 0, "millimeter", "2 mm * 3 mm")], "dimension");
  fails([variable(1, "inverse", 0, "unitless", "2 / 3 mm")], "dimension");
  fails([variable(1, "radius", 0, "millimeter", "2 mm + 1")], "dimension");
  fails([variable(1, "angle", 0, "radian", "90 deg + 2 mm")], "dimension");
});

test("公式发现直接及间接循环、未知变量、除零及溢出", () => {
  fails([variable(1, "a", 0, "unitless", "a + 1")], "cycle");
  fails([variable(1, "a", 0, "unitless", "b + 1"), variable(2, "b", 0, "unitless", "a + 1")], "cycle");
  fails([variable(1, "a", 0, "unitless", "missing + 1")], "unknown");
  fails([variable(1, "a", 0, "unitless", "1 / (2 - 2)")], "divideByZero");
  fails([variable(1, "a", 0, "unitless", "1e38 * 100")], "overflow");
  fails([variable(1, "a", 0, "unitless", "1 +")], "syntax");
});

test("公式不会执行任意 JavaScript，且有长度和嵌套限制", () => {
  fails([variable(1, "a", 0, "unitless", "globalThis.alert(1)")], "syntax");
  fails([variable(1, "a", 0, "unitless", "2 ** 10")], "syntax");
  fails([variable(1, "a", 0, "unitless", "(".repeat(41) + "1" + ")".repeat(41))], "tooLong");
  fails([variable(1, "a", 0, "unitless", "1+".repeat(130) + "1")], "tooLong");
  fails(Array.from({ length: 260 }, (_, index) => variable(index + 1, `x${index}`, 0, "unitless",
    index === 0 ? "1" : `x${index - 1} + 1`)).reverse(), "tooLong");
});

test("变量重命名仅替换公式引用，不误改相似变量或单位后缀", () => {
  assert.equal(renameFormulaReference("= radius + radius2 + 2 mm", "radius", "半径"), "= 半径 + radius2 + 2 mm");
  assert.equal(renameFormulaReference("2 mm + mm", "mm", "毫米变量"), "2 mm + 毫米变量");
});

test("公式依赖扩散到下游变量及其绑定的 Block，且 JSON 保存后可重新求值", () => {
  const vars = [variable(1, "base", 10), variable(2, "double", 0, "millimeter", "base * 2")];
  const nodes = [
    { id: 1, inputs: {}, variable_refs: { radius: 2 }, params: { radius: { Number: { value: 0, unit: "centimeter" } } } },
    { id: 2, inputs: { field: 1 }, params: {} },
    { id: 3, inputs: {}, params: {} },
  ];
  const { values, dependencies } = resolveVariableExpressions(vars);
  assert.deepEqual([...dependentVariableIds(dependencies, 1)].sort(), [1, 2]);
  assert.deepEqual([...affectedBlockIds(nodes, dependentVariableIds(dependencies, 1))].sort(), [1, 2]);
  vars[1].value = values.get(2);
  assert.equal(materializeBoundNumbers(nodes, vars), true);
  assert.equal(nodes[0].params.radius.Number.value, 2);
  const saved = JSON.parse(JSON.stringify({ variables: vars, instances: nodes }));
  saved.variables[0].value = 12;
  assert.equal(resolved(saved.variables).get(2), 24);
});
