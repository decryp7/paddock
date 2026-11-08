#!/usr/bin/env node
// node scripts/vm-smoke/release-verdict.mjs <results folder>: reads the *.json sidecars the runners wrote and prints the release verdict.
// Exit 0 PASS, 1 a product FAIL, 3 INCOMPLETE (a run is missing, partial, incomplete or from another CI run).
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { judgeRelease } from './report.mjs';

const folder = process.argv[2];
if (!folder) { console.error('usage: release-verdict.mjs <results folder>'); process.exit(2); }
const sidecars = readdirSync(folder).filter((f) => f.endsWith('.json')).sort().map((f) => JSON.parse(readFileSync(join(folder, f), 'utf8')));
const r = judgeRelease(sidecars);
console.log(`Release verdict for ${folder}: ${r.verdict}`);
for (const p of r.problems) console.log(`- ${p}`);
process.exit(r.exit);
