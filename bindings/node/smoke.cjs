const assert = require("node:assert/strict");
const kernel = require("./index.js");

const defaultMesh = kernel.meshBinary("bracket", 8);
assert.equal(defaultMesh.subarray(0, 4).toString(), "ZSM2");
assert.ok(defaultMesh.readUInt32LE(12) > 0);

const noFeatures = kernel.meshBinary("bracket", 8, 120);
assert.equal(noFeatures.subarray(0, 4).toString(), "ZSM2");
assert.equal(noFeatures.readUInt32LE(12), 0);

console.log("node smoke ok: ZSM2 + feature angle");

(async () => {
  const started = Date.now();
  const pending = kernel.meshBinaryAsync("bracket", 128, 30);
  const dispatchMs = Date.now() - started;
  assert.ok(dispatchMs < 500, `async dispatch blocked for ${dispatchMs}ms`);
  let ticked = false;
  await new Promise((resolve) => {
    setImmediate(() => {
      ticked = true;
      resolve();
    });
  });
  const asyncMesh = await pending;
  assert.ok(ticked, "event loop did not run while async mesh was pending");
  assert.equal(asyncMesh.subarray(0, 4).toString(), "ZSM2");
  assert.ok(asyncMesh.readUInt32LE(12) > 0);
  let rejected = false;
  try {
    await kernel.meshBinaryAsync("not-a-preset", 8);
  } catch (error) {
    rejected = /unknown preset/.test(String(error));
  }
  assert.ok(rejected, "async errors should reject the Promise");
  console.log(
    `node smoke ok: async mesh dispatch ${dispatchMs}ms, total ${Date.now() - started}ms`,
  );
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
