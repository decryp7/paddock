#!/usr/bin/env node
// The per-crate line-coverage gate (ADR 0050).
//
//   node scripts/coverage-gate.mjs check <lcov.info> <crates/coverage.toml> [--root <crates folder>] [--summary <file>]
//   node scripts/coverage-gate.mjs ratchet <crates/coverage.toml> --base <rev>   (the register against the base's; needs history)
//   node scripts/coverage-gate.mjs suggest <lcov.info> <crates/coverage.toml> [--root <crates folder>]   (the whole register, measured)
//   node scripts/coverage-gate.mjs regex <crates/coverage.toml>     (the --ignore-filename-regex for cargo llvm-cov)
//
// Every crate must reach DEFAULT_MINIMUM (99 %) of its lines. A crate may sit lower only through a [[crate]] entry of the
// register (a reason, a review date, a minimum that is its recorded figure and only goes up); no entry allows less than the
// hard floor of 90 %. Whole files that cannot run in unit tests are excluded by [[exclude]] entries, each with a reason; the
// regex that CI gives cargo llvm-cov is generated from them (scripts/coverage-register.mjs).
//
// The ratchet: a minimum equals its crate's measured figure (a figure above it by more than max(0.05 %, 2 lines) is an error:
// raise the minimum; under it is an error, exactly). `ratchet --base <rev>` compares the register with the one at that revision:
// a lowered minimum always fails; a new [[crate]] or [[exclude]] entry fails unless a commit in <rev>..HEAD has the trailer
// `Coverage-lowered: <crate|path> <reason> approved: user` (an honour-system marker that review checks). A base that has no
// register (git ls-tree succeeds with no output) is the initial seed; any other git error fails.
//
// One kind of line is not counted: the `#[tauri::command]` attribute itself. Tauri's macro turns the attribute into a
// wrapper (argument extraction with an error arm per argument, the response glue) and llvm-cov attributes every region of
// that wrapper to the attribute's line, so the line reads "not covered" whenever one of its arms cannot be triggered from a
// test (an injected `AppHandle`/`Window` argument never fails to extract, a generic instance for the real runtime is never
// built). The command's body, signature and arguments are separate lines and stay counted. Nothing else is skipped.
import { execFileSync } from 'node:child_process';
import { appendFileSync, readFileSync } from 'node:fs';
import { join, resolve, sep } from 'node:path';
import { isMain } from '../conformance/src/util/is-main.mjs';
import { DEFAULT_MINIMUM, HARD_FLOOR, ignoreRegex, addedTrailers, parseRegister, ratchetErrors, validateRegister, workspaceMembers } from './coverage-register.mjs';

/** A figure above its minimum by more than max(this percent, RAISE_LINES lines) is an error: the minimum is the measured figure, raised by hand. */
export const RAISE_TOLERANCE = 0.05;
export const RAISE_LINES = 2;

const GENERATED = /^\s*#\[(::)?tauri::command(\(.*\))?\]\s*$/;
const readSource = (p) => { try { return readFileSync(p, 'utf8'); } catch { return null; } };

/** The crate a source path belongs to: its first folder under `root` (null outside it); without a root, the folder after the last `/crates/`. */
export function crateOf(file, root = null) {
  const slash = file.split(sep).join('/');
  if (root) {
    const prefix = `${resolve(root).split(sep).join('/')}/`;
    return slash.startsWith(prefix) ? slash.slice(prefix.length).split('/')[0] : null;
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
  for (const [crate, { hit, total }] of [...byCrate].sort((a, b) => String(a[0]).localeCompare(String(b[0])))) {
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
    } else if (entry && status === 'pass' && figure - minimum > Math.max(RAISE_TOLERANCE, (100 * RAISE_LINES) / total)) {
      status = 'FAIL';
      errors.push(`crate ${crate}: ${shown} % is above its recorded minimum ${minimum} % by more than max(${RAISE_TOLERANCE} %, ${RAISE_LINES} lines). Next step: raise the minimum to ${floor2(figure).toFixed(2)} in crates/coverage.toml (minimums equal the measured figures and only go up).`);
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

/** The workspace members of `<root>/Cargo.toml`, or null when it cannot be read. */
function membersOf(root) {
  try {
    return workspaceMembers(readFileSync(join(root, 'Cargo.toml'), 'utf8'));
  } catch {
    return null;
  }
}

/**
 * The whole register the measured figures give: the header and the [[exclude]] entries as they are, and the [[crate]] blocks
 * regenerated (minimum = the figure cut to two decimals; each entry's reason and review date kept, a placeholder for a new
 * crate, which validation refuses until a person writes the reason). Running it on its own output gives the same text.
 */
export function suggest(byCrate, registerText, notes = []) {
  const register = parseRegister(registerText);
  const entries = new Map(register.crates.map((c) => [c.name, c]));
  const lines = registerText.split('\n');
  const first = lines.findIndex((l) => l.trim() === '[[crate]]' || l.trim() === '[[exclude]]');
  const header = (first < 0 ? lines : lines.slice(0, first)).join('\n').replace(/\n+$/, '');
  const firstExclude = lines.findIndex((l) => l.trim() === '[[exclude]]');
  const excludes = firstExclude < 0 ? '' : lines.slice(firstExclude).join('\n').replace(/\n+$/, '');
  const q = (t) => t.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  const blocks = [];
  for (const [crate, { hit, total }] of [...byCrate].sort((a, b) => String(a[0]).localeCompare(String(b[0])))) {
    if (crate === null || total === 0) continue;
    const figure = (100 * hit) / total;
    if (figure >= DEFAULT_MINIMUM) continue;
    const e = entries.get(crate);
    entries.delete(crate);
    blocks.push(`[[crate]]\nname = "${crate}"\nminimum = ${floor2(figure).toFixed(2)}\nreason = "${q(e?.reason ?? 'TODO: why these lines cannot be covered')}"\nreview = "${e?.review ?? 'YYYY-MM-DD'}"\n`);
  }
  // A registered crate with no measured lines (or one at 99 % that is not registered any more is simply not printed): its entry is
  // kept as it is, and a note says so, so a partial lcov file never drops a registration silently.
  for (const e of entries.values()) {
    const m = byCrate.get(e.name);
    if (m && m.total > 0) {
      notes.push(`crate ${e.name} reaches ${DEFAULT_MINIMUM} %: its entry is left out of the register.`);
      continue;
    }
    notes.push(`crate ${e.name} has no measured lines in the lcov file: its entry is kept as it is.`);
    blocks.push(`[[crate]]\nname = "${e.name}"\nminimum = ${Number(e.minimum).toFixed(2)}\nreason = "${q(e.reason ?? '')}"\nreview = "${e.review ?? ''}"\n`);
  }
  const nameOf = (b) => /name = "([^"]*)"/.exec(b)[1];
  blocks.sort((x, y) => nameOf(x).localeCompare(nameOf(y)));
  return [header, '', ...(blocks.length ? [blocks.join('\n')] : []), excludes, ''].join('\n').replace(/\n{3,}/g, '\n\n');
}

/** git, run in `cwd`; null on failure. */
function git(args, cwd) {
  try {
    return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 64 << 20 });
  } catch {
    return null;
  }
}

/** Runs the check; returns { exit, out, summary }. Pure apart from the reads. */
export function check({ lcovText, registerText, root, now = new Date(), read = readSource }) {
  const register = parseRegister(registerText);
  const valid = validateRegister(register, root, now, { members: membersOf(root) });
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

/**
 * The register at `base` (text, or null for the initial seed: the base has no register) and the trailer lines of the commits of
 * base..HEAD. Fails closed: an unknown base or any git error throws.
 */
function baseOf(base, registerPath) {
  const dir = resolve(registerPath, '..');
  const name = registerPath.split(sep).pop();
  const run = (args, input) => {
    try {
      return execFileSync('git', args, { cwd: dir, encoding: 'utf8', input, stdio: ['pipe', 'pipe', 'pipe'], maxBuffer: 64 << 20 });
    } catch (e) {
      throw new Error(`git ${args.slice(0, 2).join(' ')} failed (${String(e.stderr ?? e.message).trim().split('\n')[0]}). Next step: fetch the base with its history (git fetch --no-tags origin ${base}, full depth) and run again.`);
    }
  };
  run(['cat-file', '-e', `${base}^{commit}`]);
  const listed = run(['ls-tree', '--name-only', base, '--', name]);
  const text = listed.trim() === '' ? null : run(['show', `${base}:./${name}`]);
  const shas = run(['rev-list', `${base}..HEAD`]).split('\n').filter(Boolean);
  const trailers = shas.flatMap((sha) => run(['interpret-trailers', '--parse'], run(['show', '-s', '--format=%B', sha])).split('\n').filter(Boolean));
  return { text, trailers };
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
  const args = process.argv.slice(2);
  const flag = (name) => { const i = args.indexOf(name); return i < 0 ? null : args.splice(i, 2)[1]; };
  const root = flag('--root') ?? '.';
  const summaryFile = flag('--summary') ?? process.env.GITHUB_STEP_SUMMARY ?? null;
  const base = flag('--base');
  const [cmd, a, b] = args;
  if (cmd === 'regex' && a) {
    process.stdout.write(`${ignoreRegex(parseRegister(readFileSync(a, 'utf8')).excludes)}\n`);
  } else if (cmd === 'suggest' && a && b) {
    const notes = [];
    process.stdout.write(suggest(measureByCrate(readFileSync(a, 'utf8'), readSource, root), readFileSync(b, 'utf8'), notes));
    for (const n of notes) console.error(`::notice::suggest: ${n}`);
  } else if (cmd === 'ratchet' && a) {
    if (!base) {
      // No base: a pull request or a push to main must have one (fail-open there would skip the ratchet silently).
      const strict = process.env.GITHUB_EVENT_NAME === 'pull_request' || process.env.GITHUB_REF === 'refs/heads/main';
      console.log(`::${strict ? 'warning' : 'notice'}::coverage ratchet not checked: no base revision${strict ? ' on a pull request or a push to main' : ''}.`);
      process.exit(0);
    }
    let errors;
    try {
      const { text, trailers } = baseOf(base, a);
      if (text === null) console.log(`::notice::the base ${base} has no coverage.toml: this is the initial seed, the ratchet compares nothing.`);
      errors = ratchetErrors(text === null ? null : parseRegister(text), parseRegister(readFileSync(a, 'utf8')), addedTrailers(trailers));
    } catch (e) {
      errors = [e.message];
    }
    for (const e of errors) console.log(`::error::${e}`);
    if (errors.length === 0) console.log(`coverage ratchet: ok against ${base}.`);
    process.exit(errors.length === 0 ? 0 : 1);
  } else if (cmd === 'check' && a && b) {
    const { exit, out, summary } = check({ lcovText: readFileSync(a, 'utf8'), registerText: readFileSync(b, 'utf8'), root });
    console.log(out.join('\n'));
    if (summaryFile) appendFileSync(summaryFile, summary);
    process.exit(exit);
  } else {
    console.error('usage: coverage-gate.mjs check <lcov.info> <coverage.toml> [--root <crates>] [--summary <file>] | ratchet <coverage.toml> --base <rev> | suggest <lcov.info> <coverage.toml> [--root <crates>] | regex <coverage.toml>');
    process.exit(2);
  }
}
