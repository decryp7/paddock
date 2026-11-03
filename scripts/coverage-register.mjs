// The coverage register (`crates/coverage.toml`, ADR 0050): which crates may sit under the default line coverage and which
// whole files cannot run in unit tests. This module only reads and checks the register; scripts/coverage-gate.mjs measures.
//
// The file uses a small TOML subset, like paddock.waivers.toml: `[[crate]]` and `[[exclude]]` tables whose values are
// strings or numbers. Anything else is reported rather than guessed at.
//
//   [[crate]]    name, minimum (the recorded figure, only goes up), reason, review (YYYY-MM-DD)
//   [[exclude]]  path (relative to crates/), reason, job (optional: the job that covers the file)
import { existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

/** What every crate must reach unless the register says otherwise. */
export const DEFAULT_MINIMUM = 99;
/** No reason allows a crate less than this. */
export const HARD_FLOOR = 90;

/** Parses the register text. Returns { crates, excludes, problems }; problems name the line. */
export function parseRegister(text) {
  const crates = [];
  const excludes = [];
  const problems = [];
  let cur = null;
  text.split('\n').forEach((raw, i) => {
    const line = raw.replace(/\s+#.*$/, '').trim();
    if (!line || line.startsWith('#')) return;
    if (line === '[[crate]]' || line === '[[exclude]]') {
      cur = { line: i + 1 };
      (line === '[[crate]]' ? crates : excludes).push(cur);
      return;
    }
    const str = /^(\w+)\s*=\s*"((?:[^"\\]|\\.)*)"$/.exec(line);
    const num = /^(\w+)\s*=\s*(\d+(?:\.\d+)?)$/.exec(line);
    if ((!str && !num) || !cur) {
      problems.push(`line ${i + 1}: cannot read "${raw.trim()}" (expected [[crate]] or [[exclude]] and key = "value" or key = number)`);
      return;
    }
    cur[(str ?? num)[1]] = str ? str[2].replace(/\\(["\\])/g, '$1') : Number(num[2]);
  });
  return { crates, excludes, problems };
}

const blank = (v) => typeof v !== 'string' || v.trim() === '';
const DATE = /^\d{4}-\d{2}-\d{2}$/;

/**
 * Checks the register against the tree under `root` (the crates folder). Returns { errors, warnings }: strings.
 * `now` is a Date (injected). Errors fail the gate; a stale review date is only a warning.
 */
export function validateRegister({ crates, excludes, problems = [] }, root, now = new Date(), exists = existsSync, lsCrates = defaultLs) {
  const errors = problems.map((p) => `coverage.toml ${p}`);
  const warnings = [];
  const known = new Set(lsCrates(root));
  const seen = new Set();
  for (const c of crates) {
    const at = `coverage.toml line ${c.line}`;
    if (blank(c.name)) {
      errors.push(`${at}: a [[crate]] needs a name`);
      continue;
    }
    if (seen.has(c.name)) errors.push(`${at}: crate ${c.name} is listed twice`);
    seen.add(c.name);
    if (!known.has(c.name)) errors.push(`${at}: crate ${c.name} does not exist in ${root} (remove the entry)`);
    if (typeof c.minimum !== 'number') errors.push(`${at}: crate ${c.name} needs minimum = <recorded figure>`);
    else if (c.minimum < HARD_FLOOR) errors.push(`${at}: crate ${c.name} minimum ${c.minimum} is under the hard floor of ${HARD_FLOOR} % (no reason allows less)`);
    else if (c.minimum >= DEFAULT_MINIMUM) errors.push(`${at}: crate ${c.name} minimum ${c.minimum} is not under ${DEFAULT_MINIMUM} % (remove the entry)`);
    if (blank(c.reason)) errors.push(`${at}: crate ${c.name} below ${DEFAULT_MINIMUM} % needs a reason`);
    if (blank(c.review) || !DATE.test(c.review)) errors.push(`${at}: crate ${c.name} needs review = "YYYY-MM-DD"`);
    else if (new Date(`${c.review}T23:59:59Z`) < now) {
      warnings.push(`crate ${c.name}: review date ${c.review} has passed (${at}); review the reason and set a new date`);
    }
  }
  const paths = new Set();
  for (const e of excludes) {
    const at = `coverage.toml line ${e.line}`;
    if (blank(e.path)) {
      errors.push(`${at}: an [[exclude]] needs a path`);
      continue;
    }
    if (paths.has(e.path)) errors.push(`${at}: ${e.path} is listed twice`);
    paths.add(e.path);
    if (blank(e.reason)) errors.push(`${at}: excluded file ${e.path} needs a reason`);
    if (!exists(join(root, e.path))) errors.push(`${at}: excluded file ${e.path} does not exist under ${root} (remove the entry or fix the path)`);
    if (e.job !== undefined && blank(e.job)) errors.push(`${at}: excluded file ${e.path} has an empty job`);
  }
  return { errors, warnings };
}

function defaultLs(root) {
  try {
    return readdirSync(root, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name);
  } catch {
    return [];
  }
}

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** The `--ignore-filename-regex` of cargo llvm-cov: one alternative per excluded file, in the register's order. */
export function ignoreRegex(excludes) {
  if (excludes.length === 0) return '^$';
  return `(${excludes.map((e) => `/${escapeRe(e.path)}`).join('|')})$`;
}
