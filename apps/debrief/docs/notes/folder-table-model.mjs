// pd-table's O(n) work per change of `rows`, at 14,000 and 100,000 lines: treePlaces (copied, as in table/src/model.ts) plus the maps table.ts builds.
function treePlaces(rows, level) {
  const parentOf = []; const open = [];
  rows.forEach((row, i) => { const l = Math.max(1, level(row)); open.length = l - 1; parentOf.push(l > 1 ? (open[l - 2] ?? -1) : -1); open[l - 1] = i; });
  const counts = new Map(); const seen = new Map(); const out = []; const key = (i) => parentOf[i];
  rows.forEach((_, i) => counts.set(key(i), (counts.get(key(i)) ?? 0) + 1));
  rows.forEach((_, i) => { const n = (seen.get(key(i)) ?? 0) + 1; seen.set(key(i), n); out.push([n, counts.get(key(i))]); });
  return out;
}
function run(n) {
  const rows = Array.from({ length: n }, (_, i) => ({ key: `n${i}`, level: 1 + (i % 7 === 0 ? 1 : 3), name: `file_${i}.log`, size: i, status: i % 4, type: 'Log' }));
  const lines = rows.map((row) => ({ kind: 'row', row }));
  const t0 = performance.now();
  const spots = treePlaces(lines.map((l) => l.row), (r) => r.level);
  const place = new Map(); lines.forEach((l, i) => place.set(l, { pos: spots[i][0], size: spots[i][1] }));
  const positions = new Map(lines.map((l, i) => [l, i]));
  const keyIndex = new Map(lines.map((l, i) => [`r:${l.row.key}`, i]));
  const t1 = performance.now();
  // treeKey allocates two arrays on each arrow key
  const a = lines.map((l) => (l.kind === 'row' ? l.row : null)); const b = a.map((r) => r);
  const t2 = performance.now();
  return { n, perRowsChange: +(t1 - t0).toFixed(2), perArrowKey: +(t2 - t1).toFixed(2), sink: a.length + b.length + keyIndex.size + positions.size };
}
for (const n of [14000, 14000, 14000, 14000, 100000, 100000]) console.log(JSON.stringify(run(n)));
