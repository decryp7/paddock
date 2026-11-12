#!/usr/bin/env node
// The JS a window loads before first paint, split by package, from `ng build --stats-json` (the metafile `browser-stats.json`).
//   node ui/scripts/first-paint-weight.mjs <browser-stats.json> src/main.ts [more entry sources...]
// Prints one JSON line: total kB and the kB of `@angular/material`, `@angular/cdk` and paddock-ui in the static closure of the entry
// chunks (dynamic imports are lazy chunks and are not counted). Built on conformance's window-weight `measure()`; for a
// configured window use `paddock-conformance window-weight` instead. Used for the block in docs/benchmarks.md (shell/contributions).
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { indexOutputs, measure } from '../../conformance/src/window-weight/measure.mjs';

const [stats, ...entries] = process.argv.slice(2);
if (!stats || !entries.length) {
  console.error('usage: first-paint-weight.mjs <browser-stats.json> <entry source>...');
  process.exit(2);
}
const outputs = JSON.parse(readFileSync(stats, 'utf8')).outputs;
const m = measure(outputs, 'main', { entries });
if (!m) {
  console.error(`::error::an entry chunk of ${entries.join(', ')} is not in ${stats}; build with \`ng build --stats-json\` first`);
  process.exit(2);
}
const by = indexOutputs(outputs);
const sum = { material: 0, cdk: 0, paddockUi: 0 };
for (const f of m.files) {
  for (const [input, v] of Object.entries(by.get(f).inputs ?? {})) {
    if (/@angular\/material/.test(input)) sum.material += v.bytesInOutput;
    else if (/@angular\/cdk/.test(input)) sum.cdk += v.bytesInOutput;
    else if (/paddock-ui/.test(input)) sum.paddockUi += v.bytesInOutput;
  }
}
const kb = (b) => Number((b / 1024).toFixed(1));
console.log(JSON.stringify({ totalKb: kb(m.total), chunks: m.count, materialKb: kb(sum.material), cdkKb: kb(sum.cdk), paddockUiKb: kb(sum.paddockUi) }));
