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
//     said so on the row. `owed_until` is the app's next release-line tag (`<app>-vX.Y.Z`) or a date (YYYY-MM-DD) at most 60 days
//     ahead (the bounds of §1.1); an expired one is a finding. A row never names a test it does not have.
//   * `tests.outcome` and `tests.layout` (the visible-outcome and real-layout rows C86 and C89 will enumerate), required like `tauri`.
//   * `supporting = ["path#title"]`: tests that exist but are not real-engine (a mock-backed spec): looked up, never counted.
//   * `[[unlisted]] surface = "..." why = "..."` (capped, default 5) and `wiring.toml`'s `[[unwired]] wire kind why until`.
// What it reads, from the shell app layout (a source it cannot find is "not measured", never a pass; an extractor that finds
// nothing in a file that exists is a failed positive control, ER-33):
//   routes  `<appDir>/src/app/shell/nav.ts` NAV ids          -> route:/<id>
//   dialogs `<appDir>/src/app/shell/contributions.ts`        -> dialog:<id>        (DIALOG_CONTRACTS['<id>'])
//   Settings pages (same file)                               -> settings:<id>      (defineSettingsPage({ id: '<id>' }))
//   jobs    `<appDir>/src-tauri/src/app_commands.rs`         -> job:<kind>         (spec("<kind>", ...) outside #[cfg(test)])
//   request kinds `<appDir>/src/app/generated/app-commands.ts` -> app-command:<name>  (the command table is the app-agent wire
//           until wire.toml exists; ADR 0052 §1.4 names `[[unwired]]` for a kind with no feature)
// A feature "depends on a platform" when its `platforms` are not all three or one of its wires is privileged (`agent-helper:`
// or `privileged:`); only then does it need a vm step (or an owed entry) for each OS it lists, Windows included.
import { warn } from '../finding.mjs';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parseToml } from '../util/mini-toml.mjs';

const ID = 'C85';
const RULE = 0; // ADR 0052; no numbered rule yet
const OS = ['macos', 'windows', 'linux'];
const DEFAULT_UNLISTED_CAP = 5;
const MAX_DATE_AHEAD_DAYS = 60;
const TEST_KINDS = ['tauri', 'outcome', 'layout'];

const w = (file, line, message, fix) => warn(ID, RULE, file, line, message, fix);
const strings = (v) => Array.isArray(v) && v.every((x) => typeof x === 'string');
const lineOf = (text, i) => text.slice(0, i).split('\n').length;

/** The ids a regexp captures, in order and without repeats. */
function captures(text, re) {
  return [...new Set([...text.matchAll(re)].map((m) => m[1]))];
}

/** The part of a Rust file before its first `#[cfg(test)]` (test items are not features). */
const rustProduction = (text) => text.split(/^#\[cfg\(test\)\]/m)[0];

/** Where each surface kind is read, and how. `ctx` supplies appDir. */
export function sources(appDir) {
  return [
    { kind: 'route', file: `${appDir}/src/app/shell/nav.ts`, ids: (t) => captures(t.slice(t.indexOf('NAV')), /^\s*\{\s*id:\s*'([\w-]+)'/gm).map((id) => `route:/${id}`) },
    { kind: 'dialog', file: `${appDir}/src/app/shell/contributions.ts`, ids: (t) => captures(t, /DIALOG_CONTRACTS\['([\w.-]+)'\]/g).map((id) => `dialog:${id}`) },
    { kind: 'settings', file: `${appDir}/src/app/shell/contributions.ts`, ids: (t) => captures(t, /defineSettingsPage\(\{\s*id:\s*'([\w.-]+)'/g).map((id) => `settings:${id}`) },
    { kind: 'job', file: `${appDir}/src-tauri/src/app_commands.rs`, ids: (t) => captures(rustProduction(t), /\b(?:os_prompt_)?spec\(\s*"([\w-]+)"/g).map((id) => `job:${id}`) },
    { kind: 'app-command', file: `${appDir}/src/app/generated/app-commands.ts`, ids: (t) => captures(t, /^\s{2}([a-z][a-z0-9_]*):\s*\{\s*kind:/gm).map((id) => `app-command:${id}`) },
  ];
}

/** Whether a `path#title` test exists under `root` (read through `read`): the file is there and holds the title (a Rust title is a `fn` under a `#[test]`-style attribute, not a helper). */
export function testExists(spec, read) {
  const [path, ...rest] = spec.split('#');
  const title = rest.join('#');
  if (!path || !title) return { ok: false, why: 'it must be written path#title' };
  const text = read(path);
  if (text === null || text === undefined) return { ok: false, why: `${path} does not exist` };
  const found = /^[A-Za-z_]\w*$/.test(title) && path.endsWith('.rs') ? new RegExp(`#\\[[\\w:]*test\\b[^\\]]*\\]\\s*(?:#\\[[^\\]]*\\]\\s*)*(?:pub\\s+)?(?:async\\s+)?fn\\s+${title}\\b`).test(text) : text.includes(title);
  return found ? { ok: true } : { ok: false, why: `${path} has no test titled "${title}"` };
}

/** The step ids of scripts/vm-smoke/manifest.mjs (`row('<id>', ...)`). */
export const vmStepIds = (manifest) => new Set([...manifest.matchAll(/\brow\(\s*'([\w:-]+)'/g)].map((m) => m[1]));

/** Whether an `owed_until` is bounded and still open: { ok, why }. */
export function untilState(until, app, now) {
  if (typeof until !== 'string' || until === '') return { ok: false, why: 'it names no `owed_until`' };
  if (new RegExp(`^${app}-v\\d+\\.\\d+\\.\\d+$`).test(until)) return { ok: true };
  if (!/^\d{4}-\d{2}-\d{2}$/.test(until)) return { ok: false, why: `\`owed_until\` "${until}" is neither ${app}-vX.Y.Z nor a date (YYYY-MM-DD)` };
  const today = now.toISOString().slice(0, 10);
  if (until < today) return { ok: false, why: `\`owed_until\` ${until} has passed` };
  const limit = new Date(now.getTime() + MAX_DATE_AHEAD_DAYS * 86400000).toISOString().slice(0, 10);
  if (until > limit) return { ok: false, why: `\`owed_until\` ${until} is more than ${MAX_DATE_AHEAD_DAYS} days ahead` };
  return { ok: true };
}

const privileged = (wires) => wires.some((x) => /^(agent-helper|privileged):/.test(x));

/** Judges the register against what the app has. Pure over its inputs; returns findings. */
export function judge({ app, appDir, read, now, vmManifest, caps = {} }) {
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

  // 1. The rows themselves.
  const ids = new Set();
  const covered = new Set(); // surfaces and wires named by a row
  const wired = new Set();
  const stepIds = vmManifest ? vmStepIds(vmManifest) : null;
  const unwiredKinds = new Set((caps.unwired ?? []).map((u) => `${u.wire}:${u.kind}`));
  for (const f of features) {
    const at = f.__line;
    const name = typeof f.id === 'string' ? f.id : '(no id)';
    if (typeof f.id !== 'string' || !/^[a-z0-9]+([.-][a-z0-9]+)*$/.test(f.id)) out.push(w(FILE, at, `a feature row has no valid id (lower case words joined by "." or "-"): ${name}`, 'Give the row an id like "models.load".'));
    else if (ids.has(f.id)) out.push(w(FILE, at, `feature id ${f.id} appears twice`, 'Ids are unique.'));
    else ids.add(f.id);
    if (typeof f.title !== 'string' || !f.title) out.push(w(FILE, at, `feature ${name} has no title`, 'Add `title = "..."` (what the user sees it as).'));
    if (!strings(f.surfaces) || !f.surfaces.length) out.push(w(FILE, at, `feature ${name} names no surfaces`, 'List where the user meets it: route:/x, dialog:id, settings:id, job:kind, or another surface.'));
    else for (const s of f.surfaces) covered.add(s);
    const wires = strings(f.wires) ? f.wires : [];
    if (!strings(f.wires)) out.push(w(FILE, at, `feature ${name} has no \`wires\` list`, 'List the request kinds it sends, e.g. app-command:overview ([] only for a feature with none).'));
    for (const x of wires) {
      wired.add(x);
    }
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
      const u = untilState(f.owed_until, app, now);
      if (!u.ok) out.push(w(FILE, at, `feature ${name} owes tests and ${u.why}`, 'Name the next release-line tag or a date within 60 days; moving it later needs the approval trailer of ADR 0052 §1.1.'));
    }
    // A test that is named is looked up; one that is both named and owed is a contradiction.
    const named = (kind) => typeof tests[kind] === 'string' && tests[kind] !== '';
    for (const kind of TEST_KINDS) {
      if (tests[kind] !== undefined && typeof tests[kind] !== 'string') out.push(w(FILE, at, `feature ${name}: tests.${kind} must be "path#title"`, 'One string.'));
      if (named(kind)) {
        const r = testExists(tests[kind], read);
        if (!r.ok) out.push(w(FILE, at, `feature ${name}: tests.${kind} names a test that does not exist: ${r.why}`, 'Name an existing spec title, or move the entry to `owed` with a reason.'));
        if (owed.includes(kind)) out.push(w(FILE, at, `feature ${name} names tests.${kind} and also owes it`, 'Keep one of the two.'));
      } else if (!owed.includes(kind)) out.push(w(FILE, at, `feature ${name} has no tests.${kind}: neither a test nor an \`owed\` entry for it`, `Add tests.${kind} = "path#title" or put "${kind}" in \`owed\` with owed_why and owed_until.`));
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
      if (stepIds === null) out.push(w(FILE, at, `feature ${name}: not measured: vm step ${s} cannot be looked up (no --paddock checkout given)`, 'Run with --paddock <repository root>.'));
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
    for (const s of strings(f.supporting) ? f.supporting : []) {
      const r = testExists(s, read);
      if (!r.ok) out.push(w(FILE, at, `feature ${name}: supporting test does not exist: ${r.why}`, 'Remove it or fix the path#title; a row never names a test it does not have.'));
    }
  }

  // 2. [[unlisted]] rows are capped and say why.
  const cap = Number.isInteger(caps.unlisted) ? caps.unlisted : DEFAULT_UNLISTED_CAP;
  if (unlisted.length > cap) out.push(w(FILE, unlisted[cap].__line, `${unlisted.length} [[unlisted]] rows exceed the cap of ${cap}`, 'Give each surface a feature row; raising the cap needs an Escape-hatch trailer (ADR 0052 §1.1).'));
  const unlistedSurfaces = new Set();
  for (const u of unlisted) {
    if (typeof u.surface !== 'string' || typeof u.why !== 'string' || !u.why.trim()) out.push(w(FILE, u.__line, '[[unlisted]] needs `surface` and a non-empty `why`', 'unlisted = a surface that is deliberately not a feature, with the reason.'));
    else unlistedSurfaces.add(u.surface);
  }

  // 3. What the app has must be in the register (the source readers).
  for (const src of sources(appDir)) {
    const body = read(src.file);
    if (body === null || body === undefined) {
      out.push(w(src.file, 1, `not measured: ${src.file} does not exist, so the app's ${src.kind}s were not checked against features.toml`, 'Say where this app keeps them (a source reader for its layout), or leave the finding as a recorded gap.'));
      continue;
    }
    const found = src.ids(body);
    if (!found.length) {
      out.push(w(src.file, 1, `check broken: no ${src.kind} was found in ${src.file}, so a ${src.kind} with no feature would not be seen`, `Fix the reader for ${src.kind} in conformance/src/checks/C85-features-register.mjs, or the file's shape.`));
      continue;
    }
    for (const id of found) {
      if (src.kind === 'app-command') {
        if (!wired.has(id) && !unwiredKinds.has(`app-command:${id.slice('app-command:'.length)}`)) out.push(w(src.file, 1, `request kind ${id} belongs to no feature and is not [[unwired]]`, 'Add it to a feature\'s `wires`, or declare it in wiring.toml with a reason and an `until`.'));
      } else if (!covered.has(id) && !unlistedSurfaces.has(id)) out.push(w(src.file, 1, `${src.kind} ${id} maps to no feature (and is not [[unlisted]])`, 'Add it to a feature\'s `surfaces`, or list it under [[unlisted]] with a reason.'));
    }
    // A row that names a surface or wire of an enumerated kind that the app no longer has is stale.
    const have = new Set(found);
    const names = src.kind === 'app-command' ? wired : covered;
    for (const s of names) if (s.startsWith(`${src.kind}:`) && !have.has(s)) out.push(w(FILE, 1, `features.toml names ${s}, which ${src.file} does not have`, 'Remove the stale name or fix its spelling.'));
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

export default {
  id: ID,
  title: 'Every feature has a row with its real-engine test and VM steps (features.toml, report-only)',
  rule: RULE,
  scope: 'apps',
  mode: 'report-only',
  run(ctx) {
    const appDir = ctx.config.appDir;
    let vmManifest = null;
    if (ctx.options.paddock) {
      try {
        vmManifest = readFileSync(join(ctx.options.paddock, 'scripts', 'vm-smoke', 'manifest.mjs'), 'utf8');
      } catch {
        vmManifest = null;
      }
    }
    return judge({ app: ctx.app, appDir, read: ctx.read, now: ctx.now, vmManifest, caps: wiringOf(ctx.read) });
  },
};
