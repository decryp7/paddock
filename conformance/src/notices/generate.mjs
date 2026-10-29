// Third-party notices (plan X42, from Debrief's scripts/notices): THIRD_PARTY_NOTICES.md from cargo-about JSON (Rust crates)
// and the production npm dependencies (package-lock.json + each package's licence files). A pure function of its inputs and
// deterministic (sorted, no paths, no dates), so the output can be compared in CI (`--check`).
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import { allowedFor, EXCEPTIONS } from './licences.mjs';

const LICENCE_FILE = /^(licen[cs]e|licen[cs]ing|copying|notice|unlicense)([-_.].*)?$/i;
const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const hash = (t) => createHash('sha256').update(t).digest('hex');

/** Unifies line endings only, so a text embedded verbatim is the same on a CRLF (Windows) checkout. */
export function lf(text) {
  return text.replace(/\r\n?/g, '\n');
}

/** Line endings unified, trailing blanks per line removed, outer blank lines removed. */
export function normalise(text) {
  return text.replace(/\r\n?/g, '\n').split('\n').map((l) => l.replace(/\s+$/, '')).join('\n').trim();
}

/** Splits an SPDX expression into its identifiers (OR/AND operands; "WITH" stays attached). */
export function spdxIds(expr) {
  return expr.replace(/[()]/g, ' ').split(/\s+(?:OR|AND)\s+/i).map((s) => s.trim()).filter(Boolean);
}

/**
 * The licence expression a package.json or lock entry declares: a string, `{ type }`, or the legacy `licenses: [{ type }]`
 * list (several entries are alternatives, so OR). Null when nothing usable is declared.
 */
export function declaredLicense(v) {
  if (typeof v === 'string') return v.trim() || null;
  if (Array.isArray(v)) {
    const all = v.map(declaredLicense).filter(Boolean);
    return all.length ? all.join(' OR ') : null;
  }
  if (v && typeof v === 'object') return declaredLicense(v.type);
  return null;
}

/** Entries of a package-lock that ship in production (not dev-only). */
export function lockProduction(lock) {
  return Object.entries(lock.packages ?? {})
    .filter(([path, p]) => path.includes('node_modules/') && !p.dev && !p.devOptional)
    .map(([path, p]) => ({ path, name: path.slice(path.lastIndexOf('node_modules/') + 'node_modules/'.length), version: p.version, license: declaredLicense(p.license ?? p.licenses) }));
}

/**
 * The production closure of one workspace of a root npm workspace lock (Paddock's monorepo, ADR 0014 §1): its dependencies,
 * optional and peer dependencies and theirs, resolved in node's lookup order (nested first, then up to the root's
 * node_modules). The root lock's dev flags describe all workspaces together, so they cannot be used here.
 */
export function workspaceProduction(lock, ws) {
  const pk = lock.packages ?? {};
  const entry = pk[ws];
  if (!entry) throw new Error(`the lock has no workspace ${ws}`);
  const resolveFrom = (at, name) => {
    for (let base = at; ; base = base.includes('/node_modules/') ? base.slice(0, base.lastIndexOf('/node_modules/')) : '') {
      const cand = base ? `${base}/node_modules/${name}` : `node_modules/${name}`;
      if (pk[cand]) return pk[cand].link ? null : cand;
      if (!base) return null;
    }
  };
  const deps = (e) => [
    ...Object.keys(e.dependencies ?? {}),
    ...Object.keys(e.optionalDependencies ?? {}),
    ...Object.keys(e.peerDependencies ?? {}).filter((n) => !e.peerDependenciesMeta?.[n]?.optional),
  ];
  const seen = new Set();
  const todo = deps(entry).map((n) => resolveFrom(ws, n));
  while (todo.length) {
    const at = todo.pop();
    if (!at || seen.has(at)) continue;
    seen.add(at);
    for (const n of deps(pk[at])) todo.push(resolveFrom(at, n));
  }
  return [...seen].sort().map((path) => ({ path, name: path.slice(path.lastIndexOf('node_modules/') + 'node_modules/'.length), version: pk[path].version, license: pk[path].license ?? null }));
}

/** The folder with the package-lock.json that covers `dir`: `dir` itself, or the npm workspace root above it; null if none. */
function lockRoot(dir) {
  for (let d = dir; ; d = dirname(d)) {
    if (existsSync(join(d, 'package-lock.json'))) return d;
    if (dirname(d) === d) return null;
  }
}

/** The production npm packages of an app folder (itself an npm root, or a workspace of the lock above it) with their licence texts: { packages, warnings }. Needs `npm ci --omit=dev` (or a full `npm ci`) first for the texts. */
export function npmPackages(appDir) {
  const root = lockRoot(appDir);
  if (!root) throw new Error(`${join(appDir, 'package-lock.json')} not found: run the notices from the app's folder or pass --npm <dir>`);
  const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'));
  const ws = relative(root, appDir).split(sep).join('/');
  const list = ws ? workspaceProduction(lock, ws) : lockProduction(lock);
  const warnings = [];
  const seen = new Set();
  const packages = list
    .filter((p) => !seen.has(`${p.name}@${p.version}`) && seen.add(`${p.name}@${p.version}`))
    .map((p) => {
      const dir = join(root, p.path);
      let pkgJson = {};
      try {
        pkgJson = existsSync(join(dir, 'package.json')) ? JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')) : {};
      } catch {
        warnings.push(`npm ${p.name}@${p.version}: package.json is not valid JSON`);
      }
      const license = p.license ?? declaredLicense(pkgJson.license ?? pkgJson.licenses) ?? 'UNKNOWN';
      const files = existsSync(dir) ? readdirSync(dir).filter((f) => LICENCE_FILE.test(f) && !/\.(js|ts|json|css)$/.test(f)).sort() : [];
      const texts = files
        .map((f) => ({ id: /apache/i.test(f) ? 'Apache-2.0' : /mit/i.test(f) ? 'MIT' : /^notice/i.test(f) ? 'NOTICE' : null, text: normalise(readFileSync(join(dir, f), 'utf8')) }))
        .filter((t) => t.text);
      if (!texts.length) warnings.push(`npm ${p.name}@${p.version}: no licence file found (declared ${license})`);
      return { kind: 'npm', name: p.name, version: p.version, license, texts, spdx: spdxIds(license) };
    });
  return { packages, warnings };
}

/** Rust crates from cargo-about JSON documents (workspace crates are the app itself and are skipped): { packages, warnings }. */
export function rustCrates(documents) {
  const crates = new Map();
  const warnings = [];
  for (const j of documents) {
    for (const c of j.crates ?? []) {
      const p = c.package;
      if (!p.source) continue;
      const key = `${p.name} ${p.version}`;
      if (!crates.has(key)) crates.set(key, { kind: 'rust', name: p.name, version: p.version, license: c.license || 'UNKNOWN', texts: [], spdx: spdxIds(c.license || '') });
    }
    for (const l of j.licenses ?? []) {
      const text = normalise(l.text);
      for (const u of l.used_by) {
        const rec = crates.get(`${u.crate.name} ${u.crate.version}`);
        if (rec && !rec.texts.some((t) => t.text === text)) rec.texts.push({ id: l.id, text });
      }
    }
  }
  for (const rec of crates.values()) if (!rec.texts.length) warnings.push(`rust ${rec.name}@${rec.version}: no licence text found (declared ${rec.license})`);
  return { packages: [...crates.values()], warnings };
}

function wrap(items, width = 100, indent = '') {
  const lines = [];
  let cur = '';
  for (const it of items) {
    const next = cur ? `${cur}, ${it}` : it;
    if (indent.length + next.length > width && cur) {
      lines.push(`${indent}${cur},`);
      cur = it;
    } else cur = next;
  }
  if (cur) lines.push(indent + cur);
  return lines;
}

/** The notices document: { markdown, counts }. `appName` is the product name in the intro. */
export function render(rust, npm, appName = 'This application') {
  const all = [...rust, ...npm].map((p) => ({ ...p })).sort((a, b) => cmp(a.kind, b.kind) || cmp(a.name, b.name) || cmp(a.version, b.version));
  const byHash = new Map();
  for (const p of all) {
    p.refs = p.texts.map((t) => {
      const h = hash(t.text);
      if (!byHash.has(h)) byHash.set(h, { text: t.text, id: t.id ?? p.spdx[0] ?? p.license, users: [] });
      byHash.get(h).users.push(`${p.name} ${p.version}`);
      return h;
    });
  }
  const counters = new Map();
  const texts = [...byHash.entries()].sort(([ha, a], [hb, b]) => cmp(a.id, b.id) || cmp(ha, hb));
  for (const [, t] of texts) {
    const n = (counters.get(t.id) ?? 0) + 1;
    counters.set(t.id, n);
    t.label = `${t.id}-${n}`;
  }
  const tally = new Map();
  for (const p of all) tally.set(p.license, (tally.get(p.license) ?? 0) + 1);

  const out = ['# Third-party notices', ''];
  out.push(`${appName} is dual-licensed MIT OR Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE). It includes the third-party software listed below,`);
  out.push('each under its own licence. This file is generated by paddock-notices; do not edit it by hand.', '');
  out.push('## Summary', '', `- Rust crates: ${rust.length}`, `- npm packages: ${npm.length}`, `- Distinct licence texts: ${texts.length}`);
  out.push('', 'Licence expressions (packages):', '');
  for (const [lic, n] of [...tally.entries()].sort(([a, x], [b, y]) => y - x || cmp(a, b))) out.push(`- ${lic}: ${n}`);
  for (const [heading, kind] of [['Rust crates', 'rust'], ['npm packages', 'npm']]) {
    const list = all.filter((p) => p.kind === kind);
    out.push('', `## ${heading} (${list.length})`, '');
    for (const p of list) out.push(`- ${p.name} ${p.version} — ${p.license} [${[...new Set(p.refs.map((h) => byHash.get(h).label))].join(', ') || 'no text'}]`);
  }
  out.push('', '## Licence texts', '');
  for (const [, t] of texts) {
    out.push(`### ${t.label}`, '');
    out.push(`Used by (${new Set(t.users).size}):`, ...wrap([...new Set(t.users)].sort(cmp), 96).map((l) => `  ${l}`), '');
    out.push(...t.text.split('\n').map((l) => (l ? `    ${l}` : '')), '');
  }
  return { markdown: `${out.join('\n').replace(/\n+$/, '')}\n`, counts: { rust: rust.length, npm: npm.length, texts: texts.length, tally } };
}

/**
 * Whether an SPDX expression is acceptable under `allowed`: AND needs every term, OR at least one (AND binds tighter),
 * parentheses group, `X WITH exception` is allowed when the whole phrase is listed or X is (an exception only widens what
 * X permits). A trailing `+` (the deprecated "or later" spelling) is dropped. An expression that does not parse is refused.
 */
export function expressionAllowed(expr, allowed) {
  const tokens = expr.replace(/\(/g, ' ( ').replace(/\)/g, ' ) ').split(/\s+/).filter(Boolean);
  let i = 0;
  const peek = () => tokens[i];
  const isOp = (t, op) => t !== undefined && t.toUpperCase() === op;
  const id = (t) => allowed.has(t.replace(/\+$/, ''));
  function term() {
    const t = tokens[i++];
    if (t === undefined || t === ')' || isOp(t, 'AND') || isOp(t, 'OR') || isOp(t, 'WITH')) throw new Error('bad expression');
    if (t === '(') {
      const v = or();
      if (tokens[i++] !== ')') throw new Error('unbalanced');
      return v;
    }
    if (isOp(peek(), 'WITH')) {
      i++;
      const exc = tokens[i++];
      if (exc === undefined || exc === '(' || exc === ')') throw new Error('bad exception');
      return allowed.has(`${t} WITH ${exc}`) || id(t);
    }
    return id(t);
  }
  function and() {
    let v = term();
    while (isOp(peek(), 'AND')) { i++; const r = term(); v = v && r; }
    return v;
  }
  function or() {
    let v = and();
    while (isOp(peek(), 'OR')) { i++; const r = and(); v = v || r; }
    return v;
  }
  try {
    const v = or();
    return i === tokens.length && v;
  } catch {
    return false;
  }
}

/**
 * Packages whose licence is not acceptable for their ecosystem ("rust x@1: GPL-3.0-only"). Fail-closed: a package is
 * accepted only when its parsed expression evaluates true (so a missing or empty licence, NOASSERTION, UNLICENSED,
 * `SEE LICENSE IN ...`, `LicenseRef-*` and imprecise names such as "BSD" are refused) or when a package-scoped exception names
 * the package and its whole declared expression.
 */
export function nonPermissive(pkgs) {
  return pkgs
    .filter((p) => {
      const expr = typeof p.license === 'string' ? p.license.trim() : '';
      if (EXCEPTIONS.some((e) => e.kind === p.kind && e.packages.includes(p.name) && e.license === expr)) return false;
      return !(expr && expressionAllowed(expr, allowedFor(p.kind, p.name)));
    })
    .map((p) => `${p.kind} ${p.name}@${p.version}: ${p.license || '(no licence declared)'}`);
}
