import fs from 'node:fs'; import { runAll } from './bench.mjs';
const dir = process.argv[2]; const load = async (n) => new Uint8Array(fs.readFileSync(dir + '/' + n)); const out = await runAll(load, (k, v, z) => console.log(k, JSON.stringify(v), z ? JSON.stringify(z) : '')); fs.writeFileSync(process.argv[3], JSON.stringify(out, null, 1));
