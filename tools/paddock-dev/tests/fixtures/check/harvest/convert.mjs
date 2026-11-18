import { readFileSync, mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';
const repo = process.argv[2];
const out = process.argv[3];
const H = dirname(new URL(import.meta.url).pathname);
const { runChecks } = await import(pathToFileURL(join(repo, 'conformance/src/runner.mjs')).href);
const rows = readFileSync(join(H, 'out.jsonl'), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
const ID = { 'C39-freed-port.mjs': 'C39', 'C53-pid-kills.mjs': 'C53', 'C119-dev-before-toolchain.mjs': 'C119', 'C120-layout-guess.mjs': 'C120', 'C121-declared-data.mjs': 'C121', 'C122-test-path-text.mjs': 'C122', 'C123-test-process-builder.mjs': 'C123', 'C124-gate-outcomes.mjs': 'C124', 'C125-machine-paths.mjs': 'C125', 'C130-stand-in-fixtures.mjs': 'C130', 'C131-golden-machine.mjs': 'C131' };
const TWELVE = new Set(['C39', 'C53', 'C119', 'C120', 'C121', 'C122', 'C123', 'C124', 'C125', 'C129', 'C130', 'C131']);
const SUPPORT = {
  C53: [['conformance/baselines/C53.json', '{"sites":[]}']],
  C121: [['conformance/c121-allow.json', '{"allow":[]}']],
  C124: [['conformance/c124-allow.json', '{"allow":[]}']],
  C125: [['conformance/c125-allow.json', '{"history":[],"allow":[]}']],
  C130: [['conformance/c130-allow.json', '{"allow":[]}']],
  C131: [['conformance/c131-allow.json', '{"allow":[]}']],
};
const cases = {}; // id -> Map(key -> case)
const add = (id, name, files) => {
  const key = JSON.stringify(files);
  (cases[id] ??= new Map()).set(key, { name, app: 'paddock', files });
};
let skipped = { git: 0, unreadable: 0, other: 0, args: 0 };
for (const r of rows) {
  if (r.kind === 'fn') {
    const id = ID[r.module];
    const a = r.args ? JSON.parse(r.args) : null;
    if (!a) { skipped.args++; continue; }
    let files;
    const sup = SUPPORT[id] ?? [];
    switch (r.fn) {
      case 'freedPorts': files = [['crates/x/tests/t.rs', a[0]]]; break;
      case 'pidKills': case 'pidLiveness': files = [[(a[1] && String(a[1]).endsWith('.rs')) ? a[1] : 'crates/x/src/a.rs', a[0]]]; break;
      case 'layoutGuesses': case 'unusedWaivers': files = [[a[2] || (a[1] ? 'scripts/x.sh' : 'crates/x/src/a.rs'), a[0]]]; break;
      case 'flagged': files = [[a[0] === 'registry' ? 'docs/x.toml' : 'tools/t/tests/it/a.rs', a[1]]]; break;
      case 'pathTextSites': files = [[a[1], a[0]]]; break;
      case 'spawnSites': files = [[a[1], a[0]]]; if ((a[2] ?? []).length) { skipped.other++; continue; } break;
      case 'flaggedRust': case 'flaggedJs': files = [[a[0], a[1]]]; break;
      case 'machinePaths': files = [['scripts/a.sh', a[0]]]; break;
      case 'sites': files = [[a[0], a[1]]]; break;
      case 'goldenSites': files = [['x/tests/golden/a.json', a[0]]]; break;
      case 'devBeforeToolchain': {
        files = [['.gitea/workflows/w.yml', a[0]]];
        for (const n of a[1] ?? []) if (n !== 'setup-rust') files.push([`.gitea/actions/${n}/action.yml`, 'runs:\n  steps:\n    - run: bash conformance/ci/kit.sh host x\n']);
        break;
      }
      default: skipped.other++; continue;
    }
    if (files.some(([p, t]) => typeof p !== 'string' || typeof t !== 'string')) { skipped.args++; continue; }
    add(id, `${r.fn}`, [...files, ...sup]);
  } else if (r.kind === 'run') {
    const snap = JSON.parse(r.snap);
    if (snap.git) { skipped.git++; continue; }
    if (snap.unreadable) { skipped.unreadable++; continue; }
    if (!r.only || r.only.length !== 1 || !TWELVE.has(r.only[0])) { skipped.other++; continue; }
    const opts = JSON.parse(r.options);
    if (Object.keys(opts).some((k) => k !== 'now')) { skipped.other++; continue; }
    if (r.env.base) { skipped.other++; continue; }
    const res = JSON.parse(r.result);
    const c = { name: 'run', app: r.app, files: snap.files, now: opts.now ?? null, result: pick(res) };
    (cases[r.only[0]] ??= new Map()).set(JSON.stringify([r.app, snap.files, c.now]), c);
  }
}
function pick(res) {
  return { findings: res.findings, waived: res.waived, errors: res.errors, warnings: res.warnings };
}
// JS answers for the fn cases: run the real runner over the one-file tree.
const result = {};
for (const [id, map] of Object.entries(cases)) {
  result[id] = [];
  let n = 0;
  for (const c of map.values()) {
    if (!c.result) {
      const dir = mkdtempSync(join(tmpdir(), 'p3-'));
      for (const [p, t] of c.files) { mkdirSync(join(dir, dirname(p)), { recursive: true }); writeFileSync(join(dir, p), t); }
      const res = await runChecks({ root: dir, app: c.app, only: [id], options: {} });
      c.result = pick(res);
      rmSync(dir, { recursive: true, force: true });
    }
    result[id].push({ name: `${c.name}-${n++}`, app: c.app, now: c.now ?? null, files: c.files, result: c.result });
  }
}
writeFileSync(out, JSON.stringify(result));
console.error(JSON.stringify(skipped), Object.fromEntries(Object.entries(result).map(([k, v]) => [k, v.length])));
