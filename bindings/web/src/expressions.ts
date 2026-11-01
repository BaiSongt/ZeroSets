import { convertQuantity, unitFamily, type NamedVariable, type VariableUnit } from "./variables.ts";

type Dimension = "unitless" | "length" | "angle";
type Quantity = { value: number; dimension: Dimension }; // value uses mm / rad / unitless bases
type Token = { kind: "number" | "name" | "operator" | "end"; text: string; start: number; end: number };
type Expression =
  | { kind: "literal"; quantity: Quantity }
  | { kind: "reference"; name: string; start: number; end: number }
  | { kind: "unary"; operator: string; child: Expression }
  | { kind: "binary"; operator: string; left: Expression; right: Expression };

export type FormulaErrorCode = "syntax" | "unknown" | "cycle" | "dimension" | "divideByZero" | "overflow" | "tooLong";
export class FormulaError extends Error {
  readonly code: FormulaErrorCode;
  readonly detail: string;
  variableId: number | null;
  constructor(code: FormulaErrorCode, detail = "", variableId: number | null = null) {
    super(`${code}${detail ? `: ${detail}` : ""}`);
    this.name = "FormulaError";
    this.code = code;
    this.detail = detail;
    this.variableId = variableId;
  }
}

const MAX_F32 = 3.4028234663852886e38;
const MAX_EXPRESSION_LENGTH = 256;
const MAX_TOKENS = 128;
const MAX_DEPTH = 40;
const MAX_DEPENDENCY_DEPTH = 256;
const numberPattern = /(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/y;
const namePattern = /[\p{L}_][\p{L}\p{N}_]*/yu;
const suffixUnits = new Map<string, VariableUnit>([
  ["mm", "millimeter"], ["cm", "centimeter"], ["m", "meter"],
  ["in", "inch"], ["inch", "inch"], ["rad", "radian"], ["deg", "degree"],
]);
const baseUnits: Record<Dimension, VariableUnit> = {
  unitless: "unitless", length: "millimeter", angle: "radian",
};

function safeQuantity(value: number, dimension: Dimension): Quantity {
  if (!Number.isFinite(value) || Math.abs(value) > MAX_F32) throw new FormulaError("overflow");
  return { value, dimension };
}

function tokenize(source: string): Token[] {
  if (source.length > MAX_EXPRESSION_LENGTH) throw new FormulaError("tooLong");
  const tokens: Token[] = [];
  for (let at = 0; at < source.length;) {
    const character = String.fromCodePoint(source.codePointAt(at)!);
    if (/\s/u.test(character)) { at += character.length; continue; }
    let kind: Token["kind"];
    let match: RegExpExecArray | null = null;
    if (/[\d.]/.test(character)) {
      numberPattern.lastIndex = at;
      match = numberPattern.exec(source);
      kind = "number";
    } else if (/[\p{L}_]/u.test(character)) {
      namePattern.lastIndex = at;
      match = namePattern.exec(source);
      kind = "name";
    } else if ("+-*/()".includes(character)) {
      kind = "operator";
    } else {
      throw new FormulaError("syntax", source[at]!);
    }
    const text = match?.[0] ?? (kind === "operator" ? source[at]! : "");
    if (!text) throw new FormulaError("syntax", source[at]!);
    tokens.push({ kind, text, start: at, end: at + text.length });
    if (tokens.length > MAX_TOKENS) throw new FormulaError("tooLong");
    at += text.length;
  }
  tokens.push({ kind: "end", text: "", start: source.length, end: source.length });
  return tokens;
}

function parseFormula(formula: string): { tree: Expression; offset: number } {
  // Also accept spreadsheet-style `=radius + 2 mm` while retaining original text.
  const first = formula.search(/\S/u);
  const offset = first >= 0 && formula[first] === "=" ? first + 1 : 0;
  const tokens = tokenize(formula.slice(offset));
  let at = 0;
  const peek = (): Token => tokens[at]!;
  const take = (): Token => tokens[at++]!;
  const expect = (symbol: string): void => {
    if (peek().text !== symbol) throw new FormulaError("syntax", symbol);
    take();
  };
  const precedence = (token: Token): number => token.text === "+" || token.text === "-" ? 1
    : token.text === "*" || token.text === "/" ? 2 : -1;

  function primary(depth: number): Expression {
    if (depth > MAX_DEPTH) throw new FormulaError("tooLong");
    const token = take();
    if (token.text === "+" || token.text === "-") {
      return { kind: "unary", operator: token.text, child: primary(depth + 1) };
    }
    if (token.text === "(") {
      const child = expression(0, depth + 1);
      expect(")");
      return child;
    }
    if (token.kind === "number") {
      const unit = peek().kind === "name" ? suffixUnits.get(peek().text) : undefined;
      if (unit) take();
      const dimension = unit ? unitFamily(unit) : "unitless";
      const base = convertQuantity(Number(token.text), unit ?? "unitless", baseUnits[dimension]);
      if (base === null) throw new FormulaError("overflow", token.text);
      return { kind: "literal", quantity: { value: base, dimension } };
    }
    if (token.kind === "name") {
      return { kind: "reference", name: token.text, start: token.start, end: token.end };
    }
    throw new FormulaError("syntax", token.text);
  }

  function expression(minPrecedence: number, depth: number): Expression {
    let left = primary(depth + 1);
    while (precedence(peek()) >= minPrecedence) {
      const operator = take().text;
      const right = expression(precedence({ kind: "operator", text: operator, start: 0, end: 0 }) + 1, depth + 1);
      left = { kind: "binary", operator, left, right };
    }
    return left;
  }

  const tree = expression(0, 0);
  if (peek().kind !== "end") throw new FormulaError("syntax", peek().text);
  return { tree, offset };
}

function references(node: Expression, result: { name: string; start: number; end: number }[] = []): typeof result {
  if (node.kind === "reference") result.push(node);
  else if (node.kind === "unary") references(node.child, result);
  else if (node.kind === "binary") { references(node.left, result); references(node.right, result); }
  return result;
}

export function renameFormulaReference(formula: string, oldName: string, newName: string): string {
  const { tree, offset } = parseFormula(formula);
  let rewritten = formula;
  for (const ref of references(tree).filter((ref) => ref.name === oldName).reverse()) {
    rewritten = rewritten.slice(0, ref.start + offset) + newName + rewritten.slice(ref.end + offset);
  }
  return rewritten;
}

function calculate(node: Expression, lookup: (name: string) => Quantity): Quantity {
  if (node.kind === "literal") return node.quantity;
  if (node.kind === "reference") return lookup(node.name);
  if (node.kind === "unary") {
    const child = calculate(node.child, lookup);
    return safeQuantity(node.operator === "-" ? -child.value : child.value, child.dimension);
  }
  const left = calculate(node.left, lookup);
  const right = calculate(node.right, lookup);
  if (node.operator === "+" || node.operator === "-") {
    if (left.dimension !== right.dimension) throw new FormulaError("dimension");
    return safeQuantity(node.operator === "+" ? left.value + right.value : left.value - right.value, left.dimension);
  }
  if (node.operator === "*") {
    if (left.dimension !== "unitless" && right.dimension !== "unitless") throw new FormulaError("dimension");
    const dimension = left.dimension === "unitless" ? right.dimension : left.dimension;
    return safeQuantity(left.value * right.value, dimension);
  }
  if (right.value === 0) throw new FormulaError("divideByZero");
  if (right.dimension !== "unitless" && left.dimension !== right.dimension) throw new FormulaError("dimension");
  const dimension = right.dimension === "unitless" ? left.dimension : "unitless";
  return safeQuantity(left.value / right.value, dimension);
}

export type VariableResolution = {
  values: Map<number, number>;
  dependencies: Map<number, Set<number>>; // formula variable ID -> directly referenced variable IDs
};

// Resolve all formulas together, then let the caller commit values atomically.
export function resolveVariableExpressions(variables: readonly NamedVariable[]): VariableResolution {
  const byId = new Map(variables.map((variable) => [variable.id, variable]));
  const byName = new Map(variables.map((variable) => [variable.name, variable]));
  if (byId.size !== variables.length || byName.size !== variables.length) throw new FormulaError("syntax", "duplicate variable");
  const parsed = new Map<number, Expression>();
  const dependencies = new Map<number, Set<number>>();
  for (const variable of variables) {
    const inputs = new Set<number>();
    dependencies.set(variable.id, inputs);
    if (!variable.expression?.trim()) continue;
    try {
      const { tree } = parseFormula(variable.expression);
      for (const ref of references(tree)) {
        const source = byName.get(ref.name);
        if (!source) throw new FormulaError("unknown", ref.name);
        inputs.add(source.id);
      }
      parsed.set(variable.id, tree);
    } catch (error) {
      if (error instanceof FormulaError && error.variableId === null) error.variableId = variable.id;
      throw error;
    }
  }

  const quantities = new Map<number, Quantity>();
  const values = new Map<number, number>();
  const visiting = new Set<number>();
  function evaluate(id: number, depth = 0): Quantity {
    if (depth > MAX_DEPENDENCY_DEPTH) throw new FormulaError("tooLong", "dependency chain", id);
    const cached = quantities.get(id);
    if (cached) return cached;
    if (visiting.has(id)) throw new FormulaError("cycle", byId.get(id)?.name ?? String(id), id);
    const variable = byId.get(id)!;
    visiting.add(id);
    try {
      const dimension = unitFamily(variable.unit);
      const tree = parsed.get(id);
      const quantity = tree
        ? calculate(tree, (name) => evaluate(byName.get(name)!.id, depth + 1))
        : safeQuantity(convertQuantity(variable.value, variable.unit, baseUnits[dimension]) ?? NaN, dimension);
      if (quantity.dimension !== dimension) throw new FormulaError("dimension", variable.name);
      const result = convertQuantity(quantity.value, baseUnits[dimension], variable.unit);
      if (result === null) throw new FormulaError("overflow", variable.name);
      quantities.set(id, quantity);
      values.set(id, result);
      return quantity;
    } catch (error) {
      if (error instanceof FormulaError && error.variableId === null) error.variableId = id;
      throw error;
    } finally {
      visiting.delete(id);
    }
  }
  for (const variable of variables) evaluate(variable.id);
  return { values, dependencies };
}

export function dependentVariableIds(dependencies: ReadonlyMap<number, ReadonlySet<number>>, sourceId: number): Set<number> {
  const affected = new Set([sourceId]);
  const pending = [sourceId];
  const consumers = new Map<number, number[]>();
  for (const [id, sources] of dependencies) {
    for (const source of sources) consumers.set(source, [...(consumers.get(source) ?? []), id]);
  }
  while (pending.length > 0) {
    for (const id of consumers.get(pending.pop()!) ?? []) {
      if (!affected.has(id)) { affected.add(id); pending.push(id); }
    }
  }
  return affected;
}
