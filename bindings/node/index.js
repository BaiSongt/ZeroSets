'use strict';

const { existsSync } = require('node:fs');
const { join } = require('node:path');

const candidates = [
  './zeroset-kernel.win32-x64-msvc.node',
  './zeroset-kernel.win32-arm64-msvc.node',
  './zeroset-kernel.linux-x64-gnu.node',
  './zeroset-kernel.darwin-arm64.node',
];

let binding = null;
for (const candidate of candidates) {
  const path = join(__dirname, candidate);
  if (existsSync(path)) {
    binding = require(path);
    break;
  }
}

if (!binding) {
  throw new Error(
    'ZeroSet native addon not found. Build it with `cargo build -p zeroset-napi --release` ' +
      'and copy target/release/zeroset_napi.dll to bindings/node/zeroset-kernel.win32-x64-msvc.node.',
  );
}

module.exports = binding;
