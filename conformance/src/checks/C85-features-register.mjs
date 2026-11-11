// C85 (ADR 0052 §1.4, WP-A4; report-only first: warnings, `--strict C85` makes them errors): every user-facing feature of an app is
// a row of `apps/<app>/features.toml`, and each row names the real-engine test that exercises it and, where it depends on a
// platform, a vm-smoke step for each OS. A source check: nothing is built or run.
//
// Row (ADR 0052 §1.4):
//   [[feature]]
//   id = "models.load"  title = "..."  surfaces = ["route:/models", "job:load-model"]  wires = ["app-command:start_load_model"]
//   platforms = ["macos", "windows", "linux"]
//   tests = { tauri = "e2e-tauri/x.e2e.ts#title", vm = ["windows:step-id"], outcome = "...", layout = "..." }
// Extensions that ADR 0052 does not spell out yet (WP-A1 and ADR 0037 WP-7, the inputs of the full check, are not built):
//   * `owed = ["tauri", "outcome", "layout", "vm:windows"]` with `owed_why` and `owed_until`: a test that does not exist yet,
//     said so on the row. `owed_until` is a tag `<app>-vX.Y.Z` that does not exist yet and is no later than the app's next release
//     line (its latest tag's minor plus one, patch 0), or a real date (YYYY-MM-DD) at most 60 days after the commit that wrote the
//     entry (`git blame`; today where git cannot say): the bounds of §1.1. A tag that exists, a tag past the next line, a date
//     that is not a date, and an expired or reached entry are findings; in a release run (`--version`, pitwall-release.yml) they
//     are errors, so an owed test can never ride past the release it was owed for. A row never names a test it does not have.
//   * `na = ["outcome", "layout"]` with `na_why`: a kind of test that does not apply to the feature (the HTTP APIs have no UI).
//   * `privileged:<kind>` wires, next to `agent-helper:`; `settings:<id>` surfaces (the ADR's example says `setting:`; amended).
//   * `tests.outcome` and `tests.layout` (the visible-outcome and real-layout rows C86 and C89 will enumerate), required like `tauri`.
//   * `supporting = ["path#title"]`: tests that exist but are not real-engine (a mock-backed spec): looked up, never counted.
//   * `[[unlisted]] surface = "..." why = "..."` (capped, default 5) and `wiring.toml`'s `[[unwired]] wire kind why until`.
// What it reads, from the shell app layout (a source it cannot find is "not measured" (exit 6), never a pass; an extractor that
// finds nothing in a file that exists is a failed positive control, "check broken" (exit 7), ER-33; both are unwaivable errors):
//   routes  `<appDir>/src/app/shell/nav.ts` NAV ids          -> route:/<id>
//   dialogs `<appDir>/src/app/generated/dialog-contracts.ts` -> dialog:<id>        (the table Rust generates; the ids that
//           contributions.ts defines (DIALOG_CONTRACTS['<id>']) are compared with it, both ways)
//   Settings pages (same file)                               -> settings:<id>      (defineSettingsPage({ id: '<id>' }))
//   jobs    `<appDir>/src-tauri/src/app_commands.rs`         -> job:<kind>         (spec("<kind>", ...) or JobSpec { kind: "<kind>" }
//           outside every #[cfg(test)] item)
//   request kinds `<appDir>/src/app/generated/app-commands.ts` -> app-command:<name>  (the command table is the app-agent wire
//           until wire.toml exists; ADR 0052 §1.4 names `[[unwired]]` for a kind with no feature)
// NOT measured: panes, tray popovers and widget kinds (`pane:`, `tray:`, `widget:` surfaces are checked for their spelling
// only; nothing enumerates them in the source yet). docs/gates.toml records that gap.
// A feature "depends on a platform" when its `platforms` are not all three or one of its wires is privileged (`agent-helper:`
// or `privileged:`); only then does it need a vm step (or an owed entry) for each OS it lists, Windows included.
import { error, unwaivable, warn } from '../finding.mjs';
import { appendFileSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { parseToml } from './lib/mini-toml.mjs';

const ID = 'C85';
const RULE = 0; // ADR 0052; no numbered rule yet
const OS = ['macos', 'windows', 'linux'];
const DEFAULT_UNLISTED_CAP = 5;
const MAX_DATE_AHEAD_DAYS = 60;
const TEST_KINDS = ['tauri', 'outcome', 'layout'];
/** The surface kinds a row may name (ADR 0052 §3.3's list), and the wire kinds (the app's wires, §1.1). Anything else is a typo or a new kind that needs a reader. */
export const SURFACE_KINDS = ['route', 'dialog', 'settings', 'job', 'pane', 'tray', 'widget', 'http'];
export const WIRE_KINDS = ['app-command', 'app-agent', 'agent-helper', 'plugin', 'privileged'];

const w = (file, line, message, fix) => warn(ID, RULE, file, line, message, fix);
/** A finding about an input the check could not read (exit 6) or a reader that cannot see (exit 7): an error no waiver covers (ER-33). */
const notMeasured = (file, line, message, fix) => unwaivable(error(ID, RULE, file, line, message, fix), 'not-measured');
const broken = (file, line, message, fix) => unwaivable(error(ID, RULE, file, line, message, fix));
const strings = (v) => Array.isArray(v) && v.every((x) => typeof x === 'string');
const lineOf = (text, i) => text.slice(0, i).split('\n').length;
const kindOf = (name) => name.slice(0, Math.max(0, name.indexOf(':')));
/** The line of the first quoted `name` in `text` (1 when it is not there). */
const lineOfName = (text, name) => {
  const i = text.indexOf(`"${name}"`);
  return i < 0 ? 1 : lineOf(text, i);
};

/** The names a regexp captures in `text` from `from` on, in order and without repeats, each with its line: [{ name, line }]. */
function captures(text, re, from = 0) {
  const seen = new Map();
  for (const m of text.slice(from).matchAll(re)) if (!seen.has(m[1])) seen.set(m[1], { name: m[1], line: lineOf(text, from + m.index) });
  return [...seen.values()];
}

/** Index just past the `{ ... }` block that starts at or after `i` (skipping strings, comments and char literals), or `text.length` when it never closes. */
function blockEnd(text, i) {
  let depth = 0;
  for (let j = i; j < text.length; j++) {
    const c = text[j];
    if (c === '"') {
      for (j++; j < text.length && text[j] !== '"'; j++) if (text[j] === '\\') j++;
    } else if (c === '/' && text[j + 1] === '/') {
      while (j < text.length && text[j] !== '\n') j++;
    } else if (c === '/' && text[j + 1] === '*') {
      j = text.indexOf('*/', j + 2);
      if (j < 0) return text.length;
      j++;
    } else if (c === "'" && text[j + 2] === "'") j += 2;
    else if (c === "'" && text[j + 1] === '\\' && text[j + 3] === "'") j += 3;
    else if (c === '{') depth++;
    else if (c === '}' && --depth === 0) return j + 1;
  }
  return text.length;
}

/** The Rust source with every `#[cfg(test)]` item blanked (spaces, newlines kept so lines stay true): test items are not features. */
export function rustProduction(text) {
  let out = '';
  let at = 0;
  for (const m of text.matchAll(/^[ \t]*#\[cfg\(test\)\]/gm)) {
    if (m.index < at) continue; // inside an item already blanked
    let i = m.index + m[0].length;
    for (;;) {
      // further attributes, then the item: up to its `;` or through its `{ }` block
      const rest = /^\s*#\[[^\]]*\]/.exec(text.slice(i, i + 400));
      if (!rest) break;
      i += rest[0].length;
    }
    const brace = text.indexOf('{', i);
    const semi = text.indexOf(';', i);
    const end = brace < 0 || (semi >= 0 && semi < brace) ? (semi < 0 ? text.length : semi + 1) : blockEnd(text, brace);
    out += text.slice(at, m.index) + text.slice(m.index, end).replace(/[^\n]/g, ' ');
    at = end;
  }
  return out + text.slice(at);
}

/**
 * Where each surface kind is read, and how: `ids(text)` is [{ name, line }]. A kind with a `cross` list is compared with it, both
 * ways (the contracts table Rust generates is authoritative for dialogs; the shell's contributions must define the same ones).
 */
export function sources(appDir) {
  const contributions = `${appDir}/src/app/shell/contributions.ts`;
  const tag = (kind, list) => list.map((x) => ({ ...x, name: `${kind}:${x.name}` }));
  return [
    {
      kind: 'route',
      file: `${appDir}/src/app/shell/nav.ts`,
      ids: (t) => {
        const nav = t.indexOf('NAV');
        return nav < 0 ? [] : tag('route', captures(t, /^\s*\{\s*id:\s*'([\w-]+)'/gm, nav)).map((x) => ({ ...x, name: x.name.replace('route:', 'route:/') }));
      },
    },
    {
      kind: 'dialog',
      file: `${appDir}/src/app/generated/dialog-contracts.ts`,
      ids: (t) => tag('dialog', captures(t, /^ {2}'([\w.-]+)':\s*\{/gm)),
      cross: { file: contributions, ids: (t) => tag('dialog', captures(t, /DIALOG_CONTRACTS\['([\w.-]+)'\]/g)) },
    },
    { kind: 'settings', file: contributions, ids: (t) => tag('settings', captures(t, /defineSettingsPage\(\{\s*id:\s*'([\w.-]+)'/g)) },
    {
      kind: 'job',
      file: `${appDir}/src-tauri/src/app_commands.rs`,
      ids: (t) => {
        const prod = rustProduction(t);
        const found = [...captures(prod, /\b(?:os_prompt_)?spec\(\s*"([\w-]+)"/g), ...captures(prod, /\bJobSpec\s*\{\s*kind:\s*"([\w-]+)"/g)];
        return tag('job', [...new Map(found.map((x) => [x.name, x])).values()].sort((a, b) => a.line - b.line));
      },
    },
    { kind: 'app-command', file: `${appDir}/src/app/generated/app-commands.ts`, ids: (t) => tag('app-command', captures(t, /^\s{2}([a-z][a-z0-9_]*):\s*\{\s*kind:/gm)) },
  ];
}

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&').replace(/['"`]/g, (q) => `\\\\?${q}`);

/**
 * Whether a `path#title` test exists under `root` (read through `read`). A TypeScript title is the start of a `test(` or `it(`
 * call's first argument (`.only` too), and a `.skip` or `.fixme` one does not count; a Rust title is a `fn` under a `#[test]`-style
 * attribute, not a helper. A file type it cannot read is refused, never waved through.
 */
export function testExists(spec, read) {
  const [path, ...rest] = spec.split('#');
  const title = rest.join('#');
  if (!path || !title) return { ok: false, why: 'it must be written path#title' };
  const text = read(path);
  if (text === null || text === undefined) return { ok: false, why: `${path} does not exist` };
  if (path.endsWith('.rs')) {
    if (!/^[A-Za-z_]\w*$/.test(title)) return { ok: false, why: `${path}#${title}: a Rust test is named by its fn name` };
    const found = new RegExp(`#\\[[\\w:]*test\\b[^\\]]*\\]\\s*(?:#\\[[^\\]]*\\]\\s*)*(?:pub\\s+)?(?:async\\s+)?fn\\s+${title}\\b`).test(text);
    return found ? { ok: true } : { ok: false, why: `${path} has no test titled "${title}"` };
  }
  if (!/\.(ts|tsx|js|mjs)$/.test(path)) return { ok: false, why: `${path} is not a file type C85 can read tests in (.ts, .rs)` };
  // Comments are not tests, and neither is a call named inside a string or after a dot (`foo.test(`): the call stands on its own.
  const code = text.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' ')).replace(/^[ \t]*\/\/.*$/gm, '');
  const q = '[\'"`]';
  const call = '(?<![\\w$.\'"`])\\b(?:test|it)';
  if (new RegExp(`${call}(?:\\.only)?\\(\\s*${q}${escapeRe(title)}`).test(code)) return { ok: true };
  if (new RegExp(`${call}\\.(?:skip|fixme)\\(\\s*${q}${escapeRe(title)}`).test(code)) return { ok: false, why: `${path} has the test "${title}" but it is skipped (.skip or .fixme)` };
  return { ok: false, why: `${path} has no test titled "${title}"` };
}

/** The step ids of scripts/vm-smoke/manifest.mjs (`row('<id>', ...)`). */
export const vmStepIds = (manifest) => new Set([...manifest.matchAll(/\brow\(\s*'([\w:-]+)'/g)].map((m) => m[1]));

const TAG_PARTS = (app) => {
  const re = new RegExp(`^${app}-v(\\d+)\\.(\\d+)\\.(\\d+)$`);
  return (tag) => {
    const m = re.exec(tag);
    return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
  };
};
const cmpVersion = (a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];

/** A real calendar date `YYYY-MM-DD` as a UTC Date, or null (month 13 and 30 February are not dates). */
export function parseDate(s) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (!m) return null;
  const d = new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])));
  return Number.isNaN(d.getTime()) || d.toISOString().slice(0, 10) !== s ? null : d;
}

/**
 * Whether an `owed_until` is bounded and still open: { ok, why, kind }. `tags` is the app's existing release tags (a pure input:
 * null when git could not say), `since` the date the entry was written (the 60 days count from it; `now` when unknown).
 * `kind` says why not: 'bad' (not a tag or date, or not a date), 'reached' (the tag exists or the date is today or past),
 * 'too-far' (past the next release line, or more than 60 days), or 'not-measured' (no tags to compare with).
 */
export function untilState(until, app, now, { tags = null, since = now } = {}) {
  if (typeof until !== 'string' || until === '') return { ok: false, kind: 'bad', why: 'it names no `owed_until`' };
  const parts = TAG_PARTS(app);
  const tag = parts(until);
  if (tag) {
    const have = (tags ?? []).map(parts).filter(Boolean);
    if (!have.length) return { ok: false, kind: 'not-measured', why: `the existing ${app}-v* tags are not known, so \`owed_until\` ${until} cannot be checked` };
    const latest = have.reduce((a, b) => (cmpVersion(a, b) >= 0 ? a : b));
    if ((tags ?? []).includes(until) || cmpVersion(tag, latest) <= 0) return { ok: false, kind: 'reached', why: `\`owed_until\` ${until} is not past the latest tag ${app}-v${latest.join('.')}: its tests were due` };
    const limit = [latest[0], latest[1] + 1, 0];
    if (cmpVersion(tag, limit) > 0) return { ok: false, kind: 'too-far', why: `\`owed_until\` ${until} is past the next release line (${app}-v${limit.join('.')})` };
    return { ok: true };
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(until)) return { ok: false, kind: 'bad', why: `\`owed_until\` "${until}" is neither ${app}-vX.Y.Z nor a date (YYYY-MM-DD)` };
  const date = parseDate(until);
  if (!date) return { ok: false, kind: 'bad', why: `\`owed_until\` "${until}" is not a real date` };
  const today = now.toISOString().slice(0, 10);
  if (until < today) return { ok: false, kind: 'reached', why: `\`owed_until\` ${until} has passed` };
  if (until === today) return { ok: false, kind: 'reached', why: `\`owed_until\` ${until} is today: its tests are due` };
  const limit = new Date(since.getTime() + MAX_DATE_AHEAD_DAYS * 86400000).toISOString().slice(0, 10);
  if (until > limit) return { ok: false, kind: 'too-far', why: `\`owed_until\` ${until} is more than ${MAX_DATE_AHEAD_DAYS} days after the entry was written (${since.toISOString().slice(0, 10)})` };
  return { ok: true };
}

/** The owed count and the nearest expiry of a register, for the job summary (ADR 0052 §1.1): "owed: 38 tests on 19 features; nearest expiry: ...". */
export function owedSummary(features, app) {
  let tests = 0;
  const rows = new Set();
  const dates = [];
  const tagList = [];
  const parts = TAG_PARTS(app);
  for (const f of features) {
    const owed = strings(f.owed) ? f.owed : [];
    if (!owed.length) continue;
    tests += owed.length;
    rows.add(f.id);
    if (typeof f.owed_until === 'string') {
      if (parts(f.owed_until)) tagList.push(f.owed_until);
      else if (parseDate(f.owed_until)) dates.push(f.owed_until);
    }
  }
  const nearest = [];
  if (dates.length) nearest.push([...dates].sort()[0]);
  if (tagList.length) nearest.push([...tagList].sort((a, b) => cmpVersion(parts(a), parts(b)))[0]);
  return `C85 ${app}: owed ${tests} tests on ${rows.size} features; nearest expiry: ${nearest.length ? nearest.join(', ') : 'none'}`;
}

const privileged = (wires) => wires.some((x) => /^(agent-helper|privileged):/.test(x));

/** Judges the register against what the app has. Pure over its inputs; returns findings. `tags` and `since` feed the owed bounds; `release` makes those findings errors. */
export function judge({ app, appDir, read, now, vmManifest, caps = {}, tags = null, since = () => null, release = false }) {
  const out = [];
  const FILE = 'features.toml';
  const text = read(FILE);
  if (text === null || text === undefined) {
    return [w(FILE, 1, `${app} has no features.toml: none of its features says which real-engine test covers it (ADR 0052 §1.4)`, 'Add apps/<app>/features.toml with one [[feature]] row per user-facing feature (see apps/pitwall/features.toml).')];
  }
  const { tables, problems } = parseToml(text);
  for (const p of problems) out.push(w(FILE, p.line, `features.toml: ${p.message}`, 'Fix the line; the reader supports strings, arrays, inline tables, integers and booleans.'));
  const features = tables.feature ?? [];
  const unlisted = tables.unlisted ?? [];
  if (!features.length) out.push(w(FILE, 1, 'features.toml has no [[feature]] row', 'Add the app\'s features, one row each.'));
  // The owed bounds are errors in a release run, so an owed test never rides past the release it was owed for.
  const owedFinding = (line, message, fix) => (release ? error(ID, RULE, FILE, line, message, fix) : w(FILE, line, message, fix));
  const keyLine = (row, key) => {
    const lines = text.split('\n');
    for (let n = row.__line; n < lines.length && !/^\[/.test(lines[n]); n++) if (new RegExp(`^${key}\\s*=`).test(lines[n])) return n + 1;
    return row.__line;
  };

  // 1. The rows themselves.
  const ids = new Set();
  const covered = new Set(); // surfaces and wires named by a row
  const wired = new Set();
  const stepIds = vmManifest ? vmStepIds(vmManifest) : null;
  let vmUnread = false;
  let tagsUnknown = false;
  const unwiredKinds = new Set((caps.unwired ?? []).map((u) => `${u.wire}:${u.kind}`));
  const kindFinding = (at, name, what, list, items) => {
    for (const x of items) {
      if (typeof x !== 'string' || !/^[a-z-]+:\S/.test(x) || !list.includes(kindOf(x))) out.push(w(FILE, at, `feature ${name}: ${what} "${x}" is not written kind:name with a kind of ${list.join(', ')}`, `Fix the spelling, or add the new kind to ${what === 'surface' ? 'SURFACE_KINDS' : 'WIRE_KINDS'} in C85 with its reader.`));
    }
  };
  for (const f of features) {
    const at = f.__line;
    const name = typeof f.id === 'string' ? f.id : '(no id)';
    if (typeof f.id !== 'string' || !/^[a-z0-9]+([.-][a-z0-9]+)*$/.test(f.id)) out.push(w(FILE, at, `a feature row has no valid id (lower case words joined by "." or "-"): ${name}`, 'Give the row an id like "models.load".'));
    else if (ids.has(f.id)) out.push(w(FILE, at, `feature id ${f.id} appears twice`, 'Ids are unique.'));
    else ids.add(f.id);
    if (typeof f.title !== 'string' || !f.title) out.push(w(FILE, at, `feature ${name} has no title`, 'Add `title = "..."` (what the user sees it as).'));
    if (!strings(f.surfaces) || !f.surfaces.length) out.push(w(FILE, at, `feature ${name} names no surfaces`, 'List where the user meets it: route:/x, dialog:id, settings:id, job:kind, or another surface.'));
    else {
      kindFinding(at, name, 'surface', SURFACE_KINDS, f.surfaces);
      for (const s of f.surfaces) covered.add(s);
    }
    const wires = strings(f.wires) ? f.wires : [];
    if (!strings(f.wires)) out.push(w(FILE, at, `feature ${name} has no \`wires\` list`, 'List the request kinds it sends, e.g. app-command:overview ([] only for a feature with none).'));
    else kindFinding(at, name, 'wire', WIRE_KINDS, wires);
    for (const x of wires) wired.add(x);
    const platforms = strings(f.platforms) && f.platforms.length ? f.platforms : null;
    if (!platforms) out.push(w(FILE, at, `feature ${name} has no \`platforms\` list`, `Say which of ${OS.join(', ')} it works on.`));
    else for (const p of platforms) if (!OS.includes(p)) out.push(w(FILE, at, `feature ${name}: unknown platform "${p}"`, `Use ${OS.join(', ')}.`));
    const tests = f.tests && typeof f.tests === 'object' && !Array.isArray(f.tests) ? f.tests : {};
    if (f.tests !== undefined && tests !== f.tests) out.push(w(FILE, at, `feature ${name}: \`tests\` must be an inline table`, 'tests = { tauri = "path#title", vm = ["os:step"] }'));

    // Owed entries: what does not exist yet, said on the row.
    const owed = strings(f.owed) ? f.owed : [];
    if (f.owed !== undefined && !strings(f.owed)) out.push(w(FILE, at, `feature ${name}: \`owed\` must be a list of strings`, 'owed = ["tauri", "vm:windows"]'));
    const validOwed = new Set([...TEST_KINDS, ...OS.map((o) => `vm:${o}`)]);
    for (const o of owed) if (!validOwed.has(o)) out.push(w(FILE, at, `feature ${name}: owed "${o}" is not one of ${[...validOwed].join(', ')}`, 'Name the missing test kind exactly.'));
    if (owed.length) {
      if (typeof f.owed_why !== 'string' || !f.owed_why.trim()) out.push(w(FILE, at, `feature ${name} owes tests but gives no \`owed_why\``, 'Say why the test does not exist yet.'));
      const line = keyLine(f, 'owed_until');
      const u = untilState(f.owed_until, app, now, { tags, since: since(line) ?? now });
      if (u.kind === 'not-measured') tagsUnknown = true;
      else if (!u.ok) out.push(owedFinding(line, `feature ${name} owes tests and ${u.why}`, 'Name a release-line tag that does not exist yet (at most the next line) or a date within 60 days of writing the entry; moving it later needs the approval trailer of ADR 0052 §1.1. A due test is written, not moved.'));
    }
    // Kinds of test that do not apply to the feature.
    const na = strings(f.na) ? f.na : [];
    if (f.na !== undefined && !strings(f.na)) out.push(w(FILE, at, `feature ${name}: \`na\` must be a list of strings`, 'na = ["outcome", "layout"]'));
    for (const k of na) if (!TEST_KINDS.includes(k)) out.push(w(FILE, at, `feature ${name}: na "${k}" is not one of ${TEST_KINDS.join(', ')}`, 'Name the kind of test exactly.'));
    if (na.length && (typeof f.na_why !== 'string' || !f.na_why.trim())) out.push(w(FILE, at, `feature ${name} marks tests not applicable but gives no \`na_why\``, 'Say why the kind of test does not apply.'));
    // A test that is named is looked up; one that is both named and owed (or not applicable) is a contradiction.
    const named = (kind) => typeof tests[kind] === 'string' && tests[kind] !== '';
    for (const kind of TEST_KINDS) {
      if (tests[kind] !== undefined && typeof tests[kind] !== 'string') out.push(w(FILE, at, `feature ${name}: tests.${kind} must be "path#title"`, 'One string.'));
      if (named(kind)) {
        const r = testExists(tests[kind], read);
        if (!r.ok) out.push(w(FILE, at, `feature ${name}: tests.${kind} names a test that does not exist: ${r.why}`, 'Name an existing spec title, or move the entry to `owed` with a reason.'));
        if (owed.includes(kind)) out.push(w(FILE, at, `feature ${name} names tests.${kind} and also owes it`, 'Keep one of the two.'));
        if (na.includes(kind)) out.push(w(FILE, at, `feature ${name} names tests.${kind} and also marks it not applicable`, 'Keep one of the two.'));
      } else if (owed.includes(kind) && na.includes(kind)) out.push(w(FILE, at, `feature ${name} owes tests.${kind} and also marks it not applicable`, 'Keep one of the two.'));
      else if (!owed.includes(kind) && !na.includes(kind)) out.push(w(FILE, at, `feature ${name} has no tests.${kind}: neither a test nor an \`owed\` entry for it`, `Add tests.${kind} = "path#title", put "${kind}" in \`owed\` with owed_why and owed_until, or in \`na\` with na_why.`));
    }
    const vm = strings(tests.vm) ? tests.vm : [];
    if (tests.vm !== undefined && !strings(tests.vm)) out.push(w(FILE, at, `feature ${name}: tests.vm must be a list of "os:step-id"`, 'vm = ["windows:step-id"]'));
    const vmOs = new Set();
    for (const s of vm) {
      const m = /^(macos|windows|linux):([\w:-]+)$/.exec(s);
      if (!m) {
        out.push(w(FILE, at, `feature ${name}: vm step "${s}" must be written os:step-id (os is macos, windows or linux)`, 'e.g. windows:helper-smart'));
        continue;
      }
      vmOs.add(m[1]);
      if (platforms && !platforms.includes(m[1])) out.push(w(FILE, at, `feature ${name}: vm step ${s} is for an OS the feature does not list`, 'Add the OS to `platforms` or drop the step.'));
      if (stepIds === null) vmUnread = true;
      else if (!stepIds.has(m[2])) out.push(w(FILE, at, `feature ${name}: vm step ${s} names a step that does not exist in scripts/vm-smoke/manifest.mjs`, 'Name an existing step id, or move the OS to `owed` with a reason.'));
    }
    if (platforms && (platforms.length < OS.length || privileged(wires))) {
      for (const os of platforms) {
        if (vmOs.has(os)) {
          if (owed.includes(`vm:${os}`)) out.push(w(FILE, at, `feature ${name} names a ${os} vm step and also owes it`, 'Keep one of the two.'));
        } else if (!owed.includes(`vm:${os}`)) {
          out.push(w(FILE, at, `feature ${name} depends on a platform (${privileged(wires) ? 'a privileged wire' : `platforms ${platforms.join(', ')}`}) and has no vm-smoke step for ${os}`, `Add "${os}:<step-id>" to tests.vm, or put "vm:${os}" in \`owed\` with owed_why and owed_until.${os === 'windows' ? ' Windows is never left out.' : ''}`));
        }
      }
    }
    if (f.supporting !== undefined && !strings(f.supporting)) out.push(w(FILE, at, `feature ${name}: \`supporting\` must be a list of "path#title" strings`, 'supporting = ["app/e2e/x.e2e.ts#title"]'));
    for (const s of strings(f.supporting) ? f.supporting : []) {
      const r = testExists(s, read);
      if (!r.ok) out.push(w(FILE, at, `feature ${name}: supporting test does not exist: ${r.why}`, 'Remove it or fix the path#title; a row never names a test it does not have.'));
    }
  }
  // Said once, whatever the number of rows: an input that was not read is "not measured" (exit 6).
  if (vmUnread) out.push(notMeasured(FILE, 1, 'not measured: scripts/vm-smoke/manifest.mjs was not read (no --paddock checkout given, or it has no manifest), so no vm step could be looked up', 'Run with --paddock <repository root>.'));
  if (tagsUnknown) out.push(notMeasured(FILE, 1, `not measured: no ${app}-v* tag is known (no --paddock checkout given, git cannot list them, or the clone has no tags), so owed_until tags cannot be checked`, 'Run with --paddock <repository root> in a clone that has the tags (fetch-depth 0).'));

  // 2. [[unlisted]] rows are capped and say why.
  const cap = Number.isInteger(caps.unlisted) ? caps.unlisted : DEFAULT_UNLISTED_CAP;
  if (unlisted.length > cap) out.push(w(FILE, unlisted[cap].__line, `${unlisted.length} [[unlisted]] rows exceed the cap of ${cap}`, 'Give each surface a feature row; raising the cap needs an Escape-hatch trailer (ADR 0052 §1.1).'));
  const unlistedSurfaces = new Set();
  for (const u of unlisted) {
    if (typeof u.surface !== 'string' || typeof u.why !== 'string' || !u.why.trim()) out.push(w(FILE, u.__line, '[[unlisted]] needs `surface` and a non-empty `why`', 'unlisted = a surface that is deliberately not a feature, with the reason.'));
    else {
      kindFinding(u.__line, `[[unlisted]] ${u.surface}`, 'surface', SURFACE_KINDS, [u.surface]);
      unlistedSurfaces.add(u.surface);
    }
  }

  // 3. What the app has must be in the register (the source readers).
  for (const src of sources(appDir)) {
    const body = read(src.file);
    if (body === null || body === undefined) {
      out.push(notMeasured(src.file, 1, `not measured: ${src.file} does not exist, so the app's ${src.kind}s were not checked against features.toml`, 'Say where this app keeps them (a source reader for its layout); a missing source never passes.'));
      continue;
    }
    const found = src.ids(body);
    if (!found.length) {
      out.push(broken(src.file, 1, `check broken: no ${src.kind} was found in ${src.file}, so a ${src.kind} with no feature would not be seen`, `Fix the reader for ${src.kind} in conformance/src/checks/C85-features-register.mjs, or the file's shape.`));
      continue;
    }
    for (const { name: id, line } of found) {
      if (src.kind === 'app-command') {
        if (!wired.has(id) && !unwiredKinds.has(`app-command:${id.slice('app-command:'.length)}`)) out.push(w(src.file, line, `request kind ${id} belongs to no feature and is not [[unwired]]`, 'Add it to a feature\'s `wires`, or declare it in wiring.toml with a reason and an `until`.'));
      } else if (!covered.has(id) && !unlistedSurfaces.has(id)) out.push(w(src.file, line, `${src.kind} ${id} maps to no feature (and is not [[unlisted]])`, 'Add it to a feature\'s `surfaces`, or list it under [[unlisted]] with a reason.'));
    }
    // A second list of the same thing must agree with the authoritative one.
    if (src.cross) {
      const other = read(src.cross.file);
      if (other !== null && other !== undefined) {
        const mine = new Set(found.map((x) => x.name));
        const theirs = src.cross.ids(other);
        for (const x of theirs) if (!mine.has(x.name)) out.push(w(src.cross.file, x.line, `${x.name} is defined in ${src.cross.file} but ${src.file} (generated from Rust) does not list it`, 'Regenerate the contracts (PADDOCK_WRITE_TS=1 cargo test --test shell) or fix the id.'));
        const theirNames = new Set(theirs.map((x) => x.name));
        for (const x of found) if (!theirNames.has(x.name)) out.push(w(src.file, x.line, `${x.name} is in ${src.file} but ${src.cross.file} does not define it (${found.length} ${src.kind}s here, ${theirs.length} there)`, 'Define the dialog in contributions.ts or drop it from the Rust table.'));
      }
    }
    // A row that names a surface or wire of an enumerated kind that the app no longer has is stale.
    const have = new Set(found.map((x) => x.name));
    const names = src.kind === 'app-command' ? wired : covered;
    for (const s of names) if (s.startsWith(`${src.kind}:`) && !have.has(s)) out.push(w(FILE, lineOfName(text, s), `features.toml names ${s}, which ${src.file} does not have`, 'Remove the stale name or fix its spelling.'));
  }
  return out;
}

/** `wiring.toml`'s `[[unwired]]` rows and `[caps]` (ADR 0052 §1.1), when the file exists. */
function wiringOf(read) {
  const text = read('wiring.toml');
  if (text === null || text === undefined) return {};
  const { tables } = parseToml(text);
  return { unwired: tables.unwired ?? [], unlisted: tables.caps?.unlisted };
}

/** The app's release tags of the checkout at `paddock` (`git tag -l '<app>-v*'`), or null when git cannot say. */
function tagsOf(paddock, app) {
  try {
    return execFileSync('git', ['-C', paddock, 'tag', '-l', `${app}-v*`], { encoding: 'utf8', timeout: 20000 }).split('\n').map((s) => s.trim()).filter(Boolean);
  } catch {
    return null;
  }
}

/** A function from a line of features.toml to the date of the commit that wrote it (`git blame`), or null (not committed yet, no git). */
function blameDates(root) {
  return (line) => {
    try {
      const o = execFileSync('git', ['-C', root, 'blame', '-L', `${line},${line}`, '--porcelain', '--', 'features.toml'], { encoding: 'utf8', timeout: 20000 });
      const t = /^committer-time (\d+)$/m.exec(o);
      return t && !/^author Not Committed Yet$/m.test(o) ? new Date(Number(t[1]) * 1000) : null;
    } catch {
      return null;
    }
  };
}

export default {
  id: ID,
  title: 'Every feature has a row with its real-engine test and VM steps (features.toml, report-only)',
  rule: RULE,
  scope: 'apps',
  mode: 'report-only',
  run(ctx) {
    const appDir = ctx.config.appDir;
    let vmManifest = null;
    let tags = null;
    if (ctx.options.paddock) {
      try {
        vmManifest = readFileSync(join(ctx.options.paddock, 'scripts', 'vm-smoke', 'manifest.mjs'), 'utf8');
      } catch {
        vmManifest = null;
      }
      tags = ctx.options.tags ?? tagsOf(ctx.options.paddock, ctx.app);
    }
    const found = judge({ app: ctx.app, appDir, read: ctx.read, now: ctx.now, vmManifest, caps: wiringOf(ctx.read), tags, since: ctx.options.since ?? blameDates(ctx.root), release: !!ctx.options.version });
    // The job summary (ADR 0052 §1.1): the owed count and the nearest expiry, so growth is visible.
    const text = ctx.read('features.toml');
    if (text && process.env.GITHUB_STEP_SUMMARY) {
      try {
        appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${owedSummary(parseToml(text).tables.feature ?? [], ctx.app)}\n\n`);
      } catch {
        // a summary that cannot be written never fails the check
      }
    }
    return found;
  },
};
