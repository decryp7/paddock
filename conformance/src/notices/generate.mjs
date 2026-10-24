// Third-party notices (plan X42, from Debrief's scripts/notices): THIRD_PARTY_NOTICES.md from cargo-about JSON (Rust crates)
// and the production npm dependencies (package-lock.json + each package's licence files). A pure function of its inputs and
// deterministic (sorted, no paths, no dates), so the output can be compared in CI (`--check`).
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { PERMISSIVE } from './licences.mjs';

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

/** Entries of a package-lock that ship in production (not dev-only). */
export function lockProduction(lock) {
  return Object.entries(lock.packages ?? {})
    .filter(([path, p]) => path.includes('node_modules/') && !p.dev && !p.devOptional)
    .map(([path, p]) => ({ path, name: path.slice(path.lastIndexOf('node_modules/') + 'node_modules/'.length), version: p.version, license: p.license ?? null }));
}

/** The production npm packages of an app folder with their licence texts: { packages, warnings }. Needs `npm ci --omit=dev` first for the texts. */
export function npmPackages(appDir) {
  const lockFile = join(appDir, 'package-lock.json');
  if (!existsSync(lockFile)) throw new Error(`${lockFile} not found: run the notices from the app's folder or pass --npm <dir>`);
  const lock = JSON.parse(readFileSync(lockFile, 'utf8'));
  const warnings = [];
  const seen = new Set();
  const packages = lockProduction(lock)
    .filter((p) => !seen.has(`${p.name}@${p.version}`) && seen.add(`${p.name}@${p.version}`))
    .map((p) => {
      const dir = join(appDir, p.path);
      let pkgJson = {};
      try {
        pkgJson = existsSync(join(dir, 'package.json')) ? JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')) : {};
      } catch {
        warnings.push(`npm ${p.name}@${p.version}: package.json is not valid JSON`);
      }
      const license = p.license ?? (typeof pkgJson.license === 'string' ? pkgJson.license : pkgJson.license?.type) ?? 'UNKNOWN';
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

/** Packages none of whose licence options is permissive: ["rust x@1: GPL-3.0-only"]. */
export function nonPermissive(pkgs) {
  return pkgs.filter((p) => p.spdx.length && !p.spdx.some((id) => PERMISSIVE.has(id))).map((p) => `${p.kind} ${p.name}@${p.version}: ${p.license}`);
}
