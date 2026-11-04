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

const KEYS = { crate: ['name', 'minimum', 'reason', 'review'], exclude: ['path', 'reason', 'job'] };

/** The line without a trailing `# comment`; a `#` inside a quoted string stays. */
export function stripComment(raw) {
  let quoted = false;
  for (let i = 0; i < raw.length; i += 1) {
    const c = raw[i];
    if (quoted && c === '\\') i += 1;
    else if (c === '"') quoted = !quoted;
    else if (c === '#' && !quoted) return raw.slice(0, i);
  }
  return raw;
}

/** Parses the register text. Returns { crates, excludes, problems }; problems name the line. */
export function parseRegister(text) {
  const crates = [];
  const excludes = [];
  const problems = [];
  let cur = null;
  text.split('\n').forEach((raw, i) => {
    const line = stripComment(raw).trim();
    if (!line || line.startsWith('#')) return;
    if (line === '[[crate]]' || line === '[[exclude]]') {
      cur = { line: i + 1, kind: line === '[[crate]]' ? 'crate' : 'exclude' };
      (line === '[[crate]]' ? crates : excludes).push(cur);
      return;
    }
    const str = /^(\w+)\s*=\s*"((?:[^"\\]|\\.)*)"$/.exec(line);
    const num = /^(\w+)\s*=\s*(\d+(?:\.\d+)?)$/.exec(line);
    if ((!str && !num) || !cur) {
      problems.push(`line ${i + 1}: cannot read "${raw.trim()}" (expected [[crate]] or [[exclude]] and key = "value" or key = number)`);
      return;
    }
    const key = (str ?? num)[1];
    if (!KEYS[cur.kind].includes(key)) {
      problems.push(`line ${i + 1}: unknown key "${key}" in [[${cur.kind}]] (known: ${KEYS[cur.kind].join(', ')})`);
      return;
    }
    cur[key] = str ? str[2].replace(/\\(["\\])/g, '$1') : Number(num[2]);
  });
  return { crates, excludes, problems };
}

const blank = (v) => typeof v !== 'string' || v.trim() === '';
const DATE = /^\d{4}-\d{2}-\d{2}$/;

/**
 * Checks the register against the tree under `root` (the crates folder). Returns { errors, warnings }: strings.
 * `now` is a Date (injected). Errors fail the gate; a stale review date is only a warning.
 */
export function validateRegister({ crates, excludes, problems = [] }, root, now = new Date(), { exists = existsSync, members = null } = {}) {
  const errors = problems.map((p) => `coverage.toml ${p}`);
  const warnings = [];
  const known = new Set(members ?? defaultLs(root));
  const seen = new Set();
  for (const c of crates) {
    const at = `coverage.toml line ${c.line}`;
    if (blank(c.name)) {
      errors.push(`${at}: a [[crate]] needs a name`);
      continue;
    }
    if (seen.has(c.name)) errors.push(`${at}: crate ${c.name} is listed twice`);
    seen.add(c.name);
    if (!known.has(c.name)) errors.push(`${at}: crate ${c.name} is not a member of the workspace in ${root} (remove the entry)`);
    if (typeof c.minimum !== 'number') errors.push(`${at}: crate ${c.name} needs minimum = <recorded figure>`);
    else if (c.minimum < HARD_FLOOR) errors.push(`${at}: crate ${c.name} minimum ${c.minimum} is under the hard floor of ${HARD_FLOOR} % (no reason allows less)`);
    else if (c.minimum >= DEFAULT_MINIMUM) errors.push(`${at}: crate ${c.name} minimum ${c.minimum} is not under ${DEFAULT_MINIMUM} % (remove the entry)`);
    if (blank(c.reason)) errors.push(`${at}: crate ${c.name} below ${DEFAULT_MINIMUM} % needs a reason`);
    else if (/^TODO\b/.test(c.reason.trim())) errors.push(`${at}: crate ${c.name} has a placeholder reason ("${c.reason.slice(0, 30)}"); write why these lines cannot be covered`);
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
    else if (/^TODO\b/.test(e.reason.trim())) errors.push(`${at}: excluded file ${e.path} has a placeholder reason; write why it cannot run in unit tests`);
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

/** The `members` of a workspace Cargo.toml (`members = [ "a", "b" ]`), or null when the text has none. */
export function workspaceMembers(cargoToml) {
  const m = /^members\s*=\s*\[([^\]]*)\]/m.exec(cargoToml);
  if (!m) return null;
  return [...m[1].replace(/#.*$/gm, '').matchAll(/"([^"]+)"/g)].map((x) => x[1]);
}

/**
 * Subjects (a crate name or an excluded path) that the commits of the change add with the user's approval. `trailerLines` are
 * the lines of each commit's trailer block (`git interpret-trailers --parse`); the key is matched case-sensitively. The marker
 * is an honour-system one that review must check (ADR 0050); it never exempts a lowered minimum.
 */
export function addedTrailers(trailerLines) {
  const out = new Set();
  for (const line of trailerLines) {
    const m = /^Coverage-lowered:\s+(\S+)\s+(.+?)\s+approved:\s*user\s*$/.exec(line);
    if (m) out.add(m[1]);
  }
  return out;
}

/**
 * The ratchet against the register of the base. Minimums only go up: a lowered minimum of an existing [[crate]] entry always
 * fails. A new [[crate]] entry and a new [[exclude]] entry fail too, unless a commit of the change carries
 * `Coverage-lowered: <crate|path> <reason> approved: user`. `base` is a parsed register, or null when the base has none (the
 * initial seed: nothing to compare).
 */
export function ratchetErrors(base, current, approved) {
  if (base === null) return [];
  const errors = [];
  const before = new Map(base.crates.map((c) => [c.name, c]));
  const trailer = (subject) => `Next step: if the user approved it, add a trailer \`Coverage-lowered: ${subject} <reason> approved: user\` to a commit of this change; otherwise add tests or remove the entry.`;
  for (const c of current.crates) {
    const old = before.get(c.name);
    if (!old) {
      if (!approved.has(c.name)) errors.push(`crate ${c.name}: a new [[crate]] entry in coverage.toml lowers its crate's bar to ${c.minimum} %. ${trailer(c.name)}`);
    } else if (typeof c.minimum === 'number' && c.minimum < old.minimum) {
      errors.push(`crate ${c.name}: minimum lowered from ${old.minimum} % to ${c.minimum} % in coverage.toml. Next step: minimums only go up and no trailer allows a lower one; add tests, or restore ${old.minimum}.`);
    }
  }
  const paths = new Set(base.excludes.map((e) => e.path));
  for (const e of current.excludes) {
    if (!paths.has(e.path) && !approved.has(e.path)) errors.push(`file ${e.path}: a new [[exclude]] entry in coverage.toml leaves the file out of coverage. ${trailer(e.path)}`);
  }
  return errors;
}
