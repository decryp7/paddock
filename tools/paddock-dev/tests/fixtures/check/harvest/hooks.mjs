// Wraps the detection functions of the 12 JS checks and runChecks so that the inputs of the JS tests are recorded.
const FNS = {
  'C39-freed-port.mjs': ['freedPorts'],
  'C53-pid-kills.mjs': ['pidKills', 'pidLiveness'],
  'C120-layout-guess.mjs': ['layoutGuesses', 'unusedWaivers'],
  'C121-declared-data.mjs': ['flagged'],
  'C122-test-path-text.mjs': ['pathTextSites'],
  'C123-test-process-builder.mjs': ['spawnSites'],
  'C124-gate-outcomes.mjs': ['flaggedRust', 'flaggedJs'],
  'C125-machine-paths.mjs': ['machinePaths'],
  'C130-stand-in-fixtures.mjs': ['sites'],
  'C131-golden-machine.mjs': ['goldenSites'],
  'C119-dev-before-toolchain.mjs': ['devBeforeToolchain'],
};
const PRELUDE = `
import { appendFileSync as __ap, readdirSync as __rd, readFileSync as __rf, existsSync as __ex, statSync as __st } from 'node:fs';
import { join as __join } from 'node:path';
const __safe = (v) => JSON.stringify(v, (k, x) => (x instanceof Set ? [...x] : x instanceof Map ? [...x] : typeof x === 'function' ? '[fn]' : x instanceof RegExp ? String(x) : x));
const __rec = (o) => { if (process.env.HARVEST_OUT) __ap(process.env.HARVEST_OUT, JSON.stringify(o) + '\\n'); };
`;
export async function load(url, context, next) {
  const r = await next(url, context);
  const base = url.split('/').pop();
  if (url.includes('/conformance/src/checks/') && FNS[base]) {
    let src = String(r.source);
    const names = FNS[base];
    let tail = '';
    for (const n of names) {
      src = src.replace(new RegExp(`^export function ${n}\\(`, 'm'), `function ${n}(`);
      tail += `\nconst __w_${n} = (...a) => { let args; try { args = __safe(a); } catch { args = null; } const out = ${n}(...a); __rec({ kind: 'fn', module: ${JSON.stringify(base)}, fn: ${JSON.stringify(n)}, args }); return out; };\nexport { __w_${n} as ${n} };`;
    }
    return { ...r, source: PRELUDE + src + tail };
  }
  if (url.endsWith('/conformance/src/runner.mjs')) {
    let src = String(r.source);
    src = src.replace('export async function runChecks(', 'async function __runChecks(');
    const tail = `
export async function runChecks(opts) {
  let files = null;
  const walk = (dir, rel, out) => {
    for (const e of __rd(dir, { withFileTypes: true })) {
      if (e.name === 'node_modules' || e.name === 'target') continue;
      const p = rel ? rel + '/' + e.name : e.name;
      if (e.name === '.git') { out.git = true; continue; }
      if (e.isDirectory()) walk(__join(dir, e.name), p, out);
      else { try { out.files.push([p, __rf(__join(dir, e.name), 'utf8')]); } catch { out.unreadable = true; } }
    }
  };
  const snap = { files: [], git: false, unreadable: false };
  try { walk(opts.root, '', snap); } catch { snap.unreadable = true; }
  const res = await __runChecks(opts);
  __rec({ kind: 'run', app: opts.app, only: opts.only ?? null, options: __safe(opts.options ?? {}), env: { base: process.env.PADDOCK_BASE_REF ?? null }, snap: __safe(snap), result: __safe({ checks: res.checks, findings: res.findings, waived: res.waived, errors: res.errors, warnings: res.warnings }) });
  return res;
}
`;
    return { ...r, source: PRELUDE + src + tail };
  }
  return r;
}
