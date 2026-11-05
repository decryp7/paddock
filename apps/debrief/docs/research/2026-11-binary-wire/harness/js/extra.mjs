// Fairness row: Flechette objects with `fields` parsed, the same object shape as JSON.parse; a facet's 50 visible rows; progressive batches.
import fs from 'node:fs'; import { tableFromIPC as fl } from '@uwdata/flechette'; import { decode as mp } from '@msgpack/msgpack'; import { timeIt } from './bench.mjs';
const dir = process.argv[2]; const rd = (n) => new Uint8Array(fs.readFileSync(dir + '/' + n)); const td = new TextDecoder();
export const res = {}; 
const pages = ['200', '5000'];
for (const t of pages) { const u = rd(`page${t}.arrow_dict`), m = rd(`page${t}.arrow_map`), j = rd(`page${t}.json`);
  res[`page${t} json`] = timeIt(() => JSON.parse(td.decode(j)));
  res[`page${t} flechette, fields JSON.parse`] = timeIt(() => { const a = fl(u).toArray(); for (const r of a) r.fields = JSON.parse(r.fields); return a; });
  res[`page${t} flechette Map, fields to objects`] = timeIt(() => { const a = fl(m).toArray(); for (const r of a) r.fields = Object.fromEntries(r.fields); return a; });
  res[`page${t} flechette open only`] = timeIt(() => fl(u));
  res[`page${t} schema only stream bytes`] = rd(`page${t}.schema_only`).length; res[`page${t} map schema only bytes`] = rd(`page${t}.map_schema_only`).length;
  const parts = [0, 1, 2, 3, 4].map((i) => rd(`page${t}.part${i}`));
  res[`page${t} parts sum bytes`] = parts.reduce((s, p) => s + p.length, 0); res[`page${t} flechette each part alone`] = timeIt(() => fl(parts[2]).at(1).message); res[`page${t} flechette all 5 parts joined`] = timeIt(() => fl(parts).numRows);
  if (fl(parts).numRows !== (t === '200' ? 200 : 5000)) throw new Error('rows'); }
{ const a = rd('facet.arrow'), j = rd('facet.json'); res['facet json parse'] = timeIt(() => JSON.parse(td.decode(j))); res['facet flechette 50 rows of 1000'] = timeIt(() => { const t = fl(a); const v = t.getChild('value'), c = t.getChild('count'); let n = 0; for (let i = 0; i < 50; i++) { n += v.at(i).length + c.at(i); } return n; }); }
{ const a = rd('error.arrow'); res['one-row error flechette'] = timeIt(() => fl(a).at(0).message); }
console.log(JSON.stringify(res, null, 1)); fs.writeFileSync(process.argv[3], JSON.stringify(res, null, 1));
