#!/usr/bin/env node
// The line-coverage gate over an lcov file: `node scripts/coverage-gate.mjs lcov.info [minimum percent]` (default 99).
//
// One kind of line is not counted: the `#[tauri::command]` attribute itself. Tauri's macro turns the attribute into a
// wrapper (argument extraction with an error arm per argument, the response glue) and llvm-cov attributes every region of
// that wrapper to the attribute's line, so the line reads "not covered" whenever one of its arms cannot be triggered from a
// test (an injected `AppHandle`/`Window` argument never fails to extract, a generic instance for the real runtime is never
// built). The command's body, signature and arguments are separate lines and stay counted. Nothing else is skipped.
import { readFileSync } from 'node:fs';
import { isMain } from '../conformance/src/util/is-main.mjs';

const GENERATED = /^\s*#\[(::)?tauri::command(\(.*\))?\]\s*$/;

/** The crate a source path belongs to: the folder after `crates/`, else the first folder of a relative path. */
export function crateOf(path) {
  const m = /(?:^|\/)crates\/([^/]+)\//.exec(path) ?? /^([^/]+)\//.exec(path);
  return m ? m[1] : '(other)';
}

/**
 * Reads lcov text and returns { hit, total, skipped }. `read` gives a source file's text (or null when unreadable).
 * When `crates` is a Map it also gets one { total, hit } entry per crate, counted the same way.
 */
export function measure(lcov, read = (p) => { try { return readFileSync(p, 'utf8'); } catch { return null; } }, crates = null) {
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
export function gate(lcov, minimum = 99, read) {
  const crates = new Map();
  const { hit, total, skipped } = measure(lcov, read, crates);
  const percent = total === 0 ? 0 : (100 * hit) / total;
  const lines = [`lines ${hit}/${total} = ${percent.toFixed(2)}% (${skipped} generated #[tauri::command] lines not counted)`, table(crates)];
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
  const [path, minimum = '99'] = process.argv.slice(2);
  if (!path) {
    console.error('usage: coverage-gate.mjs <lcov.info> [minimum percent]');
    process.exit(2);
  }
  const { code, lines } = gate(readFileSync(path, 'utf8'), minimum);
  for (const l of lines) console.log(l);
  process.exit(code);
}
