#!/usr/bin/env node
// The line-coverage gate over an lcov file: `node scripts/coverage-gate.mjs lcov.info [minimum percent]` (default 90).
// The floor is 90% over everything and gates. The security code listed in SECURITY has a 99% target that is report-only
// (a ::warning:: with the files below it) until the privileged/trust tests land (the real lcov is at 96.3 to 96.7%).
//
// One kind of line is not counted: the `#[tauri::command]` attribute itself. Tauri's macro turns the attribute into a
// wrapper (argument extraction with an error arm per argument, the response glue) and llvm-cov attributes every region of
// that wrapper to the attribute's line, so the line reads "not covered" whenever one of its arms cannot be triggered from a
// test (an injected `AppHandle`/`Window` argument never fails to extract, a generic instance for the real runtime is never
// built). The command's body, signature and arguments are separate lines and stay counted. Nothing else is skipped.
import { readFileSync } from 'node:fs';
import { isMain } from '../conformance/src/util/is-main.mjs';

/** Security code that keeps the strict minimum: paddock-env's private module, paddock-service's privileged side, paddock-ipc's trust. */
export const SECURITY = [
  /(?:^|\/)paddock-env\/src\/private\.rs$/,
  /(?:^|\/)paddock-service\/src\/privileged\/(?!trust_fake\.rs$)/,
  /(?:^|\/)paddock-ipc\/src\/(?:trust|identity)\.rs$/,
];
export const SECURITY_MINIMUM = 99;

const GENERATED = /^\s*#\[(::)?tauri::command(\(.*\))?\]\s*$/;

/** The crate a source path belongs to: the folder after `crates/`, else the first folder of a relative path. */
export function crateOf(path) {
  const m = /(?:^|\/)crates\/([^/]+)\//.exec(path) ?? /^([^/]+)\//.exec(path);
  return m ? m[1] : '(other)';
}

/**
 * Reads lcov text and returns { hit, total, skipped }. `read` gives a source file's text (or null when unreadable).
 * When `crates` is a Map it also gets one { total, hit } entry per crate, counted the same way; when `security` is
 * { patterns, total, hit, files: Map, matched: Set } it gets the lines of the files that match one of its patterns.
 */
export function measure(lcov, read = (p) => { try { return readFileSync(p, 'utf8'); } catch { return null; } }, crates = null, security = null) {
  let file = null;
  let source = null;
  let hit = 0;
  let total = 0;
  let skipped = 0;
  for (const line of lcov.split('\n')) {
    if (line.startsWith('SF:')) {
      file = line.slice(3);
      const text = read(file);
      source = text === null ? null : text.split('\n');
    } else if (line.startsWith('DA:')) {
      const [n, count] = line.slice(3).split(',').map(Number);
      if (source !== null && GENERATED.test(source[n - 1] ?? '')) {
        skipped += 1;
        continue;
      }
      total += 1;
      if (count > 0) hit += 1;
      if (security) {
        const hitPattern = security.patterns.findIndex((p) => p.test(file));
        if (hitPattern >= 0) {
          security.matched.add(hitPattern);
          security.total += 1;
          if (count > 0) security.hit += 1;
          const per = security.files.get(file) ?? { total: 0, hit: 0 };
          per.total += 1;
          if (count > 0) per.hit += 1;
          security.files.set(file, per);
        }
      }
      if (crates) {
        const c = crates.get(crateOf(file)) ?? { total: 0, hit: 0 };
        c.total += 1;
        if (count > 0) c.hit += 1;
        crates.set(crateOf(file), c);
      }
    }
  }
  return { hit, total, skipped };
}

/** The per-crate table, the crates with most uncovered lines first. */
export function table(crates) {
  const rows = [...crates].map(([name, c]) => ({ name, ...c, missing: c.total - c.hit }));
  rows.sort((a, b) => b.missing - a.missing || a.name.localeCompare(b.name));
  const width = Math.max(5, ...rows.map((r) => r.name.length));
  const out = [`${'crate'.padEnd(width)} ${'lines'.padStart(7)} ${'covered'.padStart(7)} ${'missing'.padStart(7)} ${'%'.padStart(7)}`];
  for (const r of rows) {
    const pct = r.total === 0 ? 100 : (100 * r.hit) / r.total;
    out.push(`${r.name.padEnd(width)} ${String(r.total).padStart(7)} ${String(r.hit).padStart(7)} ${String(r.missing).padStart(7)} ${pct.toFixed(2).padStart(7)}`);
  }
  return out.join('\n');
}

/** Runs the gate over lcov text; returns { code, lines } (the output lines, the exit code). The rule: aggregate >= minimum. */
export function gate(lcov, minimum = 90, read, patterns = SECURITY) {
  const crates = new Map();
  const security = { patterns, total: 0, hit: 0, files: new Map(), matched: new Set() };
  const { hit, total, skipped } = measure(lcov, read, crates, security);
  const percent = total === 0 ? 0 : (100 * hit) / total;
  const lines = [`lines ${hit}/${total} = ${percent.toFixed(2)}% (${skipped} generated #[tauri::command] lines not counted)`, table(crates)];
  // Report-only until the privileged/trust tests land: never fails the gate (the workspace floor does).
  const unmatched = patterns.map((p, k) => [p, k]).filter(([, k]) => !security.matched.has(k)).map(([p]) => String(p));
  if (security.total === 0 || unmatched.length > 0) {
    lines.push(`::warning::security code: not measured (no file in the lcov matches ${unmatched.length ? unmatched.join(', ') : 'any security pattern'}). Next step: check SECURITY in scripts/coverage-gate.mjs and the llvm-cov ignore regex.`);
  }
  if (security.total > 0) {
    const securityPercent = (100 * security.hit) / security.total;
    lines.push(`security lines ${security.hit}/${security.total} = ${securityPercent.toFixed(2)}% (target ${SECURITY_MINIMUM}%, report-only)`);
    if (securityPercent < SECURITY_MINIMUM) {
      const below = [...security.files]
        .filter(([, c]) => c.hit < c.total && (100 * c.hit) / c.total < SECURITY_MINIMUM)
        .map(([file, c]) => `${file} ${c.hit}/${c.total}`);
      lines.push(`::warning::security code line coverage ${securityPercent.toFixed(2)}% is below the ${SECURITY_MINIMUM}% target (report-only until the privileged/trust tests land). Files below it: ${below.join(', ')}`);
    }
  }
  const pass = total > 0 && percent >= Number(minimum);
  if (!pass) {
    const needed = Math.max(0, Math.ceil((Number(minimum) / 100) * total) - hit);
    lines.push(
      `::error::line coverage ${percent.toFixed(2)}% is below the minimum ${Number(minimum)}%: ${total - hit} lines uncovered, ${needed} more must be covered. Next step: add tests for the crates at the top of the table above.`,
    );
  }
  return { code: pass ? 0 : 1, lines };
}

if (isMain(import.meta.url)) {
  const [path, minimum = '90'] = process.argv.slice(2);
  if (!path) {
    console.error('usage: coverage-gate.mjs <lcov.info> [minimum percent]');
    process.exit(2);
  }
  const { code, lines } = gate(readFileSync(path, 'utf8'), minimum);
  for (const l of lines) console.log(l);
  process.exit(code);
}
