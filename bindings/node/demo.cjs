'use strict';

const { writeFileSync } = require('node:fs');
const { join } = require('node:path');
const kernel = require('./index.js');

const [preset = 'bracket', resolution = '64'] = process.argv.slice(2);
const res = Number(resolution);

console.log('kernel   :', kernel.version());
console.log('preset   :', preset);
console.log('center   :', kernel.sample(preset, 0, 0, 0));
console.log('hole     :', kernel.sample(preset, 18, 10, 0));

const stl = kernel.meshStl(preset, res);
const output = join(__dirname, `${preset}-${res}.stl`);
writeFileSync(output, stl);
console.log('stl      :', output, `${stl.length} bytes`);

const packed = kernel.meshBinary(preset, res);
const vertices = packed.readUInt32LE(4);
const triangles = packed.readUInt32LE(8);
console.log('mesh     :', `${vertices} vertices, ${triangles} triangles`);
