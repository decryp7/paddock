#!/usr/bin/env node
// The per-crate line-coverage gate (ADR 0050).
//
//   node scripts/coverage-gate.mjs check <lcov.info> <crates/coverage.toml> [--root <crates folder>] [--summary <file>]
//   node scripts/coverage-gate.mjs regex <crates/coverage.toml>     (the --ignore-filename-regex for cargo llvm-cov)
//
// Every crate must reach DEFAULT_MINIMUM (99 %) of its lines. A crate may sit lower only through a [[crate]] entry of the
// register (a reason, a review date, a minimum that is its recorded figure and only goes up); no entry allows less than the
// hard floor of 90 %. Whole files that cannot run in unit tests are excluded by [[exclude]] entries, each with a reason; the
// regex that CI gives cargo llvm-cov is generated from them (scripts/coverage-register.mjs).
//
// One kind of line is not counted: the `#[tauri::command]` attribute itself. Tauri's macro turns the attribute into a
// wrapper (argument extraction with an error arm per argument, the response glue) and llvm-cov attributes every region of
// that wrapper to the attribute's line, so the line reads "not covered" whenever one of its arms cannot be triggered from a
// test (an injected `AppHandle`/`Window` argument never fails to extract, a generic instance for the real runtime is never
// built). The command's body, signature and arguments are separate lines and stay counted. Nothing else is skipped.
import { appendFileSync, readFileSync } from 'node:fs';
import { resolve, sep } from 'node:path';
import { isMain } from '../conformance/src/util/is-main.mjs';
import { DEFAULT_MINIMUM, HARD_FLOOR, ignoreRegex, parseRegister, validateRegister } from './coverage-register.mjs';

const GENERATED = /^\s*#\[(::)?tauri::command(\(.*\))?\]\s*$/;
const readSource = (p) => { try { return readFileSync(p, 'utf8'); } catch { return null; } };

/** The crate a source path belongs to: its first folder under `root`, else the folder after the last `/crates/`. */
export function crateOf(file, root = null) {
  const slash = file.split(sep).join('/');
  if (root) {
    const prefix = `${resolve(root).split(sep).join('/')}/`;
    if (slash.startsWith(prefix)) return slash.slice(prefix.length).split('/')[0];
  }
  const m = /^(?:.*\/)?crates\/([^/]+)\//.exec(slash);
  return m ? m[1] : null;
}

/** Reads lcov text and returns a Map crate -> { hit, total, skipped }. `read` gives a source file's text (or null). */
export function measureByCrate(lcov, read = readSource, root = null) {
  const crates = new Map();
  let slot = null;
  let source = null;
  for (const line of lcov.split('\n')) {
    if (line.startsWith('SF:')) {
      const file = line.slice(3);
      const name = crateOf(file, root);
      // A file outside crates/ (key null) counts in the totals only; no crate is judged by it.
      slot = crates.get(name) ?? { hit: 0, total: 0, skipped: 0 };
      crates.set(name, slot);
      const text = read(file);
      source = text === null ? null : text.split('\n');
    } else if (line.startsWith('DA:')) {
      const [n, count] = line.slice(3).split(',').map(Number);
      if (source !== null && GENERATED.test(source[n - 1] ?? '')) {
        slot.skipped += 1;
        continue;
      }
      slot.total += 1;
      if (count > 0) slot.hit += 1;
    }
  }
  return crates;
}

/** Totals over all crates: { hit, total, skipped }. */
export function measure(lcov, read = readSource, root = null) {
  const sum = { hit: 0, total: 0, skipped: 0 };
  for (const c of measureByCrate(lcov, read, root).values()) {
    sum.hit += c.hit;
    sum.total += c.total;
    sum.skipped += c.skipped;
  }
  return sum;
}

/** A figure cut (never rounded up) to two decimals: what the register records, so an unchanged crate always passes. */
export const floor2 = (x) => Math.floor(x * 100 + 1e-9) / 100;

/**
 * Judges every crate. Returns { rows, errors, warnings }: a row per crate { crate, figure, minimum, reason, status }
 * with status 'pass' or 'FAIL'; errors and warnings are annotation texts without the `::error::` prefix.
 */
export function evaluate(byCrate, register, registerWarnings = []) {
  const entries = new Map(register.crates.map((c) => [c.name, c]));
  const rows = [];
  const errors = [];
  const warnings = [...registerWarnings];
  for (const [crate, { hit, total }] of [...byCrate].sort((a, b) => a[0].localeCompare(b[0]))) {
    if (crate === null || total === 0) continue;
    const figure = (100 * hit) / total;
    const entry = entries.get(crate);
    const minimum = entry && typeof entry.minimum === 'number' ? entry.minimum : DEFAULT_MINIMUM;
    const shown = floor2(figure).toFixed(2); // cut like the recorded minimum, so "98.75 under 98.76" is never shown as equal
    let status = 'pass';
    if (figure < HARD_FLOOR) {
      status = 'FAIL';
      errors.push(`crate ${crate}: ${shown} % is under the hard floor of ${HARD_FLOOR} % (its minimum is ${minimum} %). Next step: add tests; no reason in crates/coverage.toml allows a crate under ${HARD_FLOOR} %.`);
    } else if (figure < minimum) {
      status = 'FAIL';
      const next = entry
        ? 'the minimum only ratchets up: add tests for the lines this change left uncovered (a lower minimum is not an option).'
        : `add tests, or, for lines that cannot run in unit tests, exclude the whole file under [[exclude]] (reason, job) or register the crate under [[crate]] with a reason, a review date and its figure (never under ${HARD_FLOOR} %), in crates/coverage.toml.`;
      errors.push(`crate ${crate}: ${shown} % is under its minimum ${minimum} %${entry?.reason ? ` (registered: ${entry.reason})` : ''}. Next step: ${next}`);
    }
    if (entry && figure >= DEFAULT_MINIMUM) {
      warnings.push(`crate ${crate}: ${shown} % reaches ${DEFAULT_MINIMUM} %; remove its [[crate]] entry from crates/coverage.toml.`);
    } else if (entry && status === 'pass' && floor2(figure) > minimum) {
      warnings.push(`crate ${crate}: ${shown} % is above its recorded minimum ${minimum} %; raise minimum to ${floor2(figure).toFixed(2)} in crates/coverage.toml (it only goes up).`);
    }
    rows.push({ crate, figure, minimum, reason: figure < DEFAULT_MINIMUM ? (entry?.reason ?? '') : '', status });
  }
  for (const c of register.crates) {
    if (!byCrate.has(c.name) || byCrate.get(c.name).total === 0) {
      warnings.push(`crate ${c.name}: no measured lines in the lcov file, so its entry was not checked.`);
    }
  }
  return { rows, errors, warnings };
}

/** The job-summary table (markdown). */
export function summaryMarkdown({ rows, errors, warnings }, totals, excludes) {
  const lines = ['### Coverage per crate', '', '| Crate | Lines | Minimum | Reason (below 99 %) | Result |', '| --- | ---: | ---: | --- | --- |'];
  for (const r of rows) {
    lines.push(`| ${r.crate} | ${floor2(r.figure).toFixed(2)} % | ${r.minimum} % | ${r.reason.replace(/\|/g, '\\|')} | ${r.status} |`);
  }
  const all = totals.total === 0 ? 0 : (100 * totals.hit) / totals.total;
  lines.push('', `Workspace: ${totals.hit}/${totals.total} lines = ${all.toFixed(2)} % (information only; the gate is per crate). ` +
    `${totals.skipped} generated #[tauri::command] lines not counted; ${excludes.length} files excluded by crates/coverage.toml.`);
  lines.push('', errors.length === 0 ? 'Result: pass.' : `Result: FAIL, ${errors.length} crate(s) or register entries to fix.`);
  for (const w of warnings) lines.push(`- warning: ${w}`);
  for (const e of errors) lines.push(`- error: ${e}`);
  return `${lines.join('\n')}\n`;
}

/** Runs the check; returns { exit, out, summary }. Pure apart from the reads. */
export function check({ lcovText, registerText, root, now = new Date(), read = readSource }) {
  const register = parseRegister(registerText);
  const valid = validateRegister(register, root, now);
  const byCrate = measureByCrate(lcovText, read, root);
  const judged = evaluate(byCrate, register, valid.warnings);
  const errors = [...valid.errors, ...judged.errors];
  const result = { ...judged, errors };
  const out = [
    ...errors.map((e) => `::error::${e}`),
    ...judged.warnings.map((w) => `::warning::${w}`),
    ...judged.rows.map((r) => `${r.status.padEnd(4)} ${r.crate.padEnd(28)} ${floor2(r.figure).toFixed(2).padStart(6)} % (minimum ${r.minimum} %)${r.reason ? `  ${r.reason}` : ''}`),
  ];
  return { exit: errors.length === 0 ? 0 : 1, out, summary: summaryMarkdown(result, measure(lcovText, read, root), register.excludes) };
}

if (isMain(import.meta.url)) {
  const args = process.argv.slice(2);
  const flag = (name) => { const i = args.indexOf(name); return i < 0 ? null : args.splice(i, 2)[1]; };
  const root = flag('--root') ?? '.';
  const summaryFile = flag('--summary') ?? process.env.GITHUB_STEP_SUMMARY ?? null;
  const [cmd, a, b] = args;
  if (cmd === 'regex' && a) {
    process.stdout.write(`${ignoreRegex(parseRegister(readFileSync(a, 'utf8')).excludes)}\n`);
  } else if (cmd === 'check' && a && b) {
    const { exit, out, summary } = check({ lcovText: readFileSync(a, 'utf8'), registerText: readFileSync(b, 'utf8'), root });
    console.log(out.join('\n'));
    if (summaryFile) appendFileSync(summaryFile, summary);
    process.exit(exit);
  } else {
    console.error('usage: coverage-gate.mjs check <lcov.info> <coverage.toml> [--root <crates>] [--summary <file>] | regex <coverage.toml>');
    process.exit(2);
  }
}
