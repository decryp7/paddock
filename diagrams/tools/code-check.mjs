// Diagrams match the code (the 2026-11-06 redesign, generalised): the code names a diagram draws (class boxes with their
// operations and fields, node names and technology lines, calls in sequence messages, file and crate names) are checked
// against the app's sources. One helper for every app's diagram sources (rule 42); an app's test passes its source folder,
// its code roots and the few names that are prose on purpose, each with a reason.
//
//   const rec = await recordDiagrams(srcDir, ['c4', 'c43', 'cls', 'seq']);   // imports the sources without writing files
//   const code = indexCode([appDir + '/crates', appDir + '/app/src']);
//   assert.deepEqual(unknownCode(rec, code, { skip }), []);

import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { basename, join } from 'node:path';
import { pathToFileURL } from 'node:url';

/** Imports each diagram source (`<dir>/<name>.mjs`) with writing switched off and returns { diagramFile: { nodes, classes, messages } }. */
export async function recordDiagrams(dir, names) {
  process.env.DIAGRAM_RECORD = '1';
  for (const n of names) await import(pathToFileURL(join(dir, `${n}.mjs`)).href);
  return globalThis.__recordedDiagrams ?? {};
}

const SKIP_DIRS = new Set(['node_modules', 'target', 'dist', '.git', 'vendor']);
function walk(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    if (e.isDirectory()) {
      if (!SKIP_DIRS.has(e.name)) walk(join(dir, e.name), out);
    } else out.push(join(dir, e.name));
  }
  return out;
}

/** What the code defines: types, functions, modules (items and files), crate names, file base paths, and every identifier word. */
export function indexCode(roots, { crateRoots = [] } = {}) {
  const files = roots.flatMap((r) => walk(r)).filter((f) => /\.(rs|ts|toml)$/.test(f));
  const types = new Set(), fns = new Set(), mods = new Set(), crates = new Set(), words = new Set(), paths = [];
  for (const f of files) {
    paths.push(f);
    const text = readFileSync(f, 'utf8');
    if (f.endsWith('Cargo.toml')) {
      const m = text.match(/^name\s*=\s*"([^"]+)"/m);
      if (m) crates.add(m[1]);
      continue;
    }
    if (f.endsWith('.toml')) continue;
    for (const m of text.matchAll(/\b(?:struct|enum|trait|type|union|interface|class)\s+([A-Za-z_]\w*)/g)) types.add(m[1]);
    for (const m of text.matchAll(/\b(?:fn|function)\s+([A-Za-z_]\w*)/g)) fns.add(m[1]);
    for (const m of text.matchAll(/\bmod\s+([a-z_]\w*)/g)) mods.add(m[1]);
    for (const m of text.matchAll(/\b(?:const|static)\s+([A-Za-z_]\w*)/g)) types.add(m[1]);
    for (const m of text.matchAll(/[A-Za-z_]\w*/g)) words.add(m[0]);
    mods.add(basename(f).replace(/\.(rs|ts)$/, ''));
  }
  // Crates and packages the app uses but does not hold (the shared ones): their names count, their code does not.
  for (const f of crateRoots.flatMap((r) => walk(r))) {
    if (f.endsWith('Cargo.toml')) {
      const m = readFileSync(f, 'utf8').match(/^name\s*=\s*"([^"]+)"/m);
      if (m) crates.add(m[1]);
    } else if (f.endsWith('package.json')) {
      const name = JSON.parse(readFileSync(f, 'utf8')).name;
      if (name) crates.add(name.split('/').pop());
    }
  }
  return { types, fns, mods, crates, words, paths };
}

/** The part of a Rust path that names the item: `memory::Preset` → Preset, `Reading<T>` → Reading. */
const item = (s) => s.replace(/<.*$/, '').split('::').pop().trim();
const IDENT = /^[A-Za-z_]\w*$/;

/** Operations of a class box: `a, b(x) → R` gives [a, b]; continuation lines (indented, `//`, `→`) and prose give nothing. */
export function opNames(op) {
  if (/^\s|^\/\/|^→/.test(op)) return [];
  let s = op.replace(/\/\/.*$/, '').replace(/→.*$/, '');
  for (let i = 0; i < 4; i++) s = s.replace(/\([^()]*\)/g, '');
  return s.split(',').map((t) => item(t.trim())).filter((t) => IDENT.test(t));
}

/** Field or variant names of an attribute line: `id: ModelId`, `disks, disk_io, network`, `Available(T)`; prose and notes give nothing. */
export function attrNames(attr) {
  if (/^\s|^\/\/|[^\w\s:,()<>'`&\[\]]/.test(attr.replace(/\/\/.*$/, '').split(':')[0])) return [];
  const head = attr.replace(/\/\/.*$/, '').replace(/\([^)]*\)/g, '').split(':')[0];
  return head.split(',').map((t) => t.trim()).filter((t) => IDENT.test(t));
}

const CAMEL = /^[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]*)+$/;

/**
 * Names the diagrams draw that the code does not have: [`<diagram>: <what>`]. skip: Map or object of "<diagram>: <what>" strings
 * (exactly as printed) to the reason that name is prose, so a skip is a visible, reviewed line.
 */
export function unknownCode(recorded, code, { skip = {}, crateNames = true } = {}) {
  const out = [];
  const bad = (diagram, what) => {
    const key = `${diagram}: ${what}`;
    if (!(key in skip)) out.push(key);
  };
  const hasPath = (p) => code.paths.some((f) => f.endsWith(`/${p}`));
  for (const [diagram, { nodes = [], classes = [], messages = [] }] of Object.entries(recorded)) {
    for (const c of classes) {
      const n = item(c.name);
      if (IDENT.test(n) && !code.types.has(n)) bad(diagram, `class ${c.name}`);
      const variants = /enum/.test(c.stereotype ?? '');
      for (const a of c.attrs) for (const t of attrNames(a)) if (!code.words.has(t)) bad(diagram, `${c.name} ${variants ? 'variant' : 'field'} ${t}`);
      for (const o of c.ops) for (const t of opNames(o)) if (!code.fns.has(t)) bad(diagram, `${c.name} operation ${t}()`);
    }
    for (const nd of nodes) {
      const text = [nd.name, nd.tech, nd.desc].filter(Boolean).join(' · ');
      for (const m of text.matchAll(/[\w./-]+\.(?:rs|ts)\b/g)) if (!hasPath(m[0])) bad(diagram, `file ${m[0]}`);
      if (crateNames) for (const m of text.matchAll(/(?<![\w/@.-])((?:pitwall|dyno|paddock)-[a-z][a-z-]*[a-z])\b/g)) if (!code.crates.has(m[1])) bad(diagram, `crate ${m[1]}`);
      for (const t of [nd.name, nd.tech].filter(Boolean).join(' · ').split(/ · |, | \+ /).map((x) => x.trim())) {
        if (CAMEL.test(t) && !code.types.has(t)) bad(diagram, `type ${t}`);
      }
    }
    for (const msg of messages) {
      for (const m of msg.matchAll(/\b([a-z_][a-z0-9_]*)\(/g)) if (!code.fns.has(m[1]) && !code.words.has(m[1])) bad(diagram, `call ${m[1]}()`);
    }
  }
  return out.sort();
}
