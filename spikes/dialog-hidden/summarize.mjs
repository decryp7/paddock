// Summarises a probe log (probe.jsonl) into one Markdown table per method and scenario.
// Usage: node summarize.mjs probe.jsonl [--trials]
import { readFileSync } from 'node:fs';

const [file, flag] = process.argv.slice(2);
const lines = readFileSync(file, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
const byLabel = new Map();
for (const l of lines) {
  if (!l.label || l.label === 'main') continue;
  if (!byLabel.has(l.label)) byLabel.set(l.label, { trial: l.trial, ev: [] });
  byLabel.get(l.label).ev.push(l);
}

const first = (t, name) => t.ev.find((e) => e.ev === name);
const rows = [];
for (const [label, t] of byLabel) {
  const shown = first(t, 'shown');
  const atDeadline = first(t, 'shown_at_deadline');
  const showT = (atDeadline ?? shown)?.t_ms ?? Infinity;
  const before = (name) => { const e = first(t, name); return e ? e.t_ms < showT : false; };
  const ready = first(t, 'js:ready');
  const after = first(t, 'js:after_show');
  const settle = first(t, 'js:settle');
  const script = first(t, 'js:script');
  const want = ready ? Math.ceil(ready.data.h * (ready.data.dpr / (shown?.data.scale ?? 1))) : null;
  // The window's inner height once settled: GTK applies set_size asynchronously, so the value right after it is stale.
  const inner = (first(t, 'settled') ?? shown)?.data.inner?.[1];
  const sync = first(t, 'js:shown_sync');
  const roAfter = t.ev.filter((e) => e.ev === 'js:ro' && e.t_ms > showT).map((e) => e.data.h);
  const exact = !!(ready && after && settle && Math.abs(after.data.h - ready.data.h) < 0.5 && Math.abs(settle.data.h - ready.data.h) < 0.5
    && Math.abs(after.data.ih - ready.data.h) < 1.0 && inner != null && Math.abs(inner - want) < 1.0);
  rows.push({
    label, ...t.trial,
    built: first(t, 'built')?.t_ms, script: script?.t_ms, visAtScript: script?.data.vis, builtVisible: first(t, 'built')?.data.visible,
    raf: before('js:raf1'), fonts: before('js:fonts_ready_initial'), ro: before('js:ro'), t0lat: first(t, 'js:timeout0')?.data.lat,
    ready: ready?.t_ms, deadline: !!first(t, 'deadline'), font: ready?.data.font, h: ready?.data.h, afterH: after?.data.h, ih: after?.data.ih,
    settleH: settle?.data.h, syncIh: sync?.data.ih, inner, want, scale: shown?.data.scale, dpr: ready?.data.dpr, roAfter: roAfter.length, exact,
    show: shown?.t_ms, afterShow: after ? after.t_ms - (shown?.t_ms ?? 0) : null,
  });
}

const med = (xs) => { const s = xs.filter((x) => x != null).sort((a, b) => a - b); return s.length ? s[Math.floor(s.length / 2)] : null; };
const max = (xs) => { const s = xs.filter((x) => x != null); return s.length ? Math.max(...s) : null; };
const f = (x) => (x == null ? '-' : Math.round(x));
const count = (xs, p) => `${xs.filter(p).length}/${xs.length}`;

if (flag === '--trials') {
  console.log('| label | method | scenario | vis@script | built visible | raf hidden | fonts hidden | RO hidden | ready ms | deadline | font at ready | h | after h | innerHeight at show | innerHeight next frame | window inner | want | scale | dpr | RO after show | exact |');
  console.log('|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|');
  for (const r of rows) {
    console.log(`| ${r.label} | ${r.method} | ${r.scenario} | ${r.visAtScript} | ${r.builtVisible} | ${r.raf} | ${r.fonts} | ${r.ro} | ${f(r.ready)} | ${r.deadline} | ${r.font} | ${r.h} | ${r.afterH} | ${r.syncIh} | ${r.ih} | ${r.inner} | ${r.want} | ${r.scale} | ${r.dpr} | ${r.roAfter} | ${r.exact} |`);
  }
  process.exit(0);
}

const groups = new Map();
for (const r of rows) {
  const k = `${r.method}|${r.scenario}`;
  if (!groups.has(k)) groups.set(k, []);
  groups.get(k).push(r);
}
console.log('| method | scenario | n | JS runs hidden | rAF before show | fonts.ready before show | ResizeObserver before show | setTimeout(0) lag ms (median) | ready ms median / max | missed 3 s deadline | font loaded at ready | size exact after show | resize events after show |');
console.log('|---|---|---|---|---|---|---|---|---|---|---|---|---|');
for (const [k, rs] of [...groups].sort()) {
  const [method, scenario] = k.split('|');
  console.log(`| ${method} | ${scenario} | ${rs.length} | ${count(rs, (r) => r.script != null && r.script < (r.show ?? Infinity))} | ${count(rs, (r) => r.raf)} | ${count(rs, (r) => r.fonts)} | ${count(rs, (r) => r.ro)} | ${f(med(rs.map((r) => r.t0lat)))} | ${f(med(rs.map((r) => r.ready)))} / ${f(max(rs.map((r) => r.ready)))} | ${count(rs, (r) => r.deadline)} | ${count(rs, (r) => r.font === true)} | ${count(rs, (r) => r.exact)} | ${f(max(rs.map((r) => r.roAfter)))} |`);
}
