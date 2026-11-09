// C13: the Rust lint preset (plan X11, lint/). The workspace's `[workspace.lints]` has every lint of lint/workspace-lints.toml
// at that level or stricter, every member crate opts in with `[lints] workspace = true`, and the root clippy.toml lists
// every `disallowed-methods` and `disallowed-types` path of lint/clippy.toml (an app may add paths and reword reasons, not drop a path).
//
// C13 is gated per app (scope `apps`); Debrief, Pitcrew and Spotter omit the two keyring::Entry / keyring_core::Entry type entries on
// purpose, until their secrets move to paddock-secrets files (their C99 waivers in paddock.waivers.toml).
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { error, warn } from '../finding.mjs';
import { tomlTables } from '../sync/baseline.mjs';
import { paddockRoot } from '../sync/paddock-root.mjs';

const LEVELS = { allow: 0, warn: 1, deny: 2, forbid: 3 };

/** `{ lint: level }` from the lines of one `[workspace.lints.*]` table (`key = "warn"` or `key = { level = "warn", ... }`). */
export function lintLevels(lines) {
  const out = {};
  for (const l of lines) {
    const m = /^\s*([\w:-]+)\s*=\s*(?:"(\w+)"|\{[^}]*\blevel\s*=\s*"(\w+)")/.exec(l);
    if (m) out[m[1]] = m[2] ?? m[3];
  }
  return out;
}

/** Levels of a table in a Cargo.toml text, or null when the table is absent. */
function tableLevels(text, name) {
  const t = tomlTables(text).tables.find((x) => x.header === `[${name}]`);
  return t ? lintLevels(t.lines.slice(1)) : null;
}

/** Disallowed paths listed in a clippy.toml text, from its `disallowed-methods` array or (`key`) another one. */
export function disallowedPaths(text, key = 'disallowed-methods') {
  const start = text.search(new RegExp(`^${key}\\s*=`, 'm'));
  if (start < 0) return [];
  const end = text.indexOf('\n]', start);
  const region = text.slice(start, end < 0 ? text.length : end);
  return [...region.matchAll(/^\s*\{[^}\n]*?\bpath\s*=\s*"([^"]+)"/gm)].map((m) => m[1]);
}

/**
 * The workspace folder of the app from its `paddock.ci.json` `cargo` field (ER-33: the probe reads the declared workspace, it does
 * not assume the app root). `applies: false` for an app without Rust; no contract at all keeps the old reading (the root).
 */
export function workspaceBase(ctx) {
  const text = ctx.read('paddock.ci.json');
  if (text === null) return { applies: true, base: '' };
  let contract;
  try {
    contract = JSON.parse(text);
  } catch {
    return { applies: true, base: '' };
  }
  if (typeof contract.cargo !== 'string' || contract.cargo === '') return { applies: false, base: '' };
  const dir = contract.cargo.replace(/^\.\/?/, '').replace(/\/$/, '');
  return { applies: true, base: dir === '' ? '' : `${dir}/` };
}

/**
 * The members `docs/gates.toml` lists in `not_yet` for this check and app (a `[[ratchet]]` entry, ADR 0060 section 3.2): workspace
 * members that have not opted in to the lint preset yet. The list may only shrink (C100 checks that); here they are exempt.
 */
export function ratchetNotYet(register, app) {
  const out = [];
  for (const block of register.split(/^\[\[/m).slice(1)) {
    if (!block.startsWith('ratchet]]')) continue;
    if (!new RegExp(`^id\\s*=\\s*"C13"`, 'm').test(block) || !new RegExp(`^app\\s*=\\s*"${app}"`, 'm').test(block)) continue;
    const list = /^not_yet\s*=\s*\[([^\]]*)\]/m.exec(block);
    if (list) out.push(...[...list[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]));
  }
  return out;
}

/**
 * The clippy `disallowed-methods` paths the app's clippy.toml may lack while its ratchet lists them in `clippy_not_yet` (ADR 0060
 * 3.2). clippy.toml applies to the whole workspace, so a path whose call sites are not cleaned up yet (Debrief's `temp_dir`) cannot
 * be added without failing every member that is not opted in; the list may only shrink (C100 checks that).
 */
export function ratchetClippyNotYet(register, app) {
  const out = [];
  for (const block of register.split(/^\[\[/m).slice(1)) {
    if (!block.startsWith('ratchet]]')) continue;
    if (!/^id\s*=\s*"C13"/m.test(block) || !new RegExp(`^app\\s*=\\s*"${app}"`, 'm').test(block)) continue;
    const list = /^clippy_not_yet\s*=\s*\[([^\]]*)\]/m.exec(block);
    if (list) out.push(...[...list[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]));
  }
  return out;
}

export default {
  id: 'C13',
  scope: 'apps',
  mode: 'gate',
  needs: 'rust',
  title: 'Rust lint preset: workspace lints, member opt-in and disallowed methods',
  rule: 40,
  run(ctx) {
    const paddock = paddockRoot(ctx.options);
    if (!paddock) return [warn('C13', 40, 'lint', 1, 'no Paddock checkout with lint/ (--paddock): the Rust lint preset was not compared', 'Run with --paddock <checkout of paddock>.')];
    const out = [];
    const { applies, base } = workspaceBase(ctx);
    if (!applies) return [];
    const at = (f) => `${base}${f}`;
    const register = ctx.options.gatesText ?? (existsSync(join(paddock, 'docs', 'gates.toml')) ? readFileSync(join(paddock, 'docs', 'gates.toml'), 'utf8') : '');
    const notYet = ratchetNotYet(register, ctx.app);
    const clippyNotYet = ratchetClippyNotYet(register, ctx.app);
    const preset = readFileSync(join(paddock, 'lint', 'workspace-lints.toml'), 'utf8');
    const root = ctx.read(at('Cargo.toml'));
    if (root === null) return [error('C13', 40, at('Cargo.toml'), 1, `there is no workspace Cargo.toml at the repository root${base ? ` of ${base}` : ''}`, 'Add the workspace manifest with the [workspace.lints] tables from paddock/lint/workspace-lints.toml.')];
    for (const kind of ['rust', 'clippy']) {
      const want = tableLevels(preset, `workspace.lints.${kind}`) ?? {};
      const have = tableLevels(root, `workspace.lints.${kind}`) ?? {};
      for (const [lint, level] of Object.entries(want)) {
        const mine = have[lint];
        if (mine === undefined) out.push(error('C13', 40, at('Cargo.toml'), 1, `[workspace.lints.${kind}] lacks ${lint} = "${level}"`, `Add ${lint} = "${level}" to [workspace.lints.${kind}] (paddock/lint/workspace-lints.toml).`));
        else if ((LEVELS[mine] ?? -1) < LEVELS[level]) out.push(error('C13', 40, at('Cargo.toml'), 1, `[workspace.lints.${kind}] ${lint} = "${mine}" is weaker than the preset's "${level}"`, `Set ${lint} to "${level}" or stricter.`));
      }
    }
    const excluded = [...(/\bexclude\s*=\s*\[([^\]]*)\]/.exec(root)?.[1].matchAll(/"([^"]+)"/g) ?? [])].map((m) => m[1]);
    for (const full of ctx.files().filter((f) => f.startsWith(base) && f.endsWith('Cargo.toml') && f !== at('Cargo.toml'))) {
      const file = full.slice(base.length);
      const dir = file.slice(0, -'/Cargo.toml'.length);
      if (excluded.some((e) => dir === e || dir.startsWith(`${e}/`))) continue;
      if (notYet.includes(dir)) continue;
      const text = ctx.read(full) ?? '';
      const { tables } = tomlTables(text);
      if (!tables.some((t) => t.header === '[package]') || tables.some((t) => t.header === '[workspace]')) continue;
      const optIn = tables.find((t) => t.header === '[lints]')?.lines.some((l) => /^\s*workspace\s*=\s*true\b/.test(l)) || /^\s*lints\.workspace\s*=\s*true\b/m.test(text);
      if (!optIn) out.push(error('C13', 40, full, 1, `${full} does not opt in to the workspace lints`, 'Add `[lints]` with `workspace = true`.'));
    }
    const clippy = ctx.read(at('clippy.toml')) ?? ctx.read(at('.clippy.toml'));
    if (clippy === null) out.push(error('C13', 40, at('clippy.toml'), 1, 'clippy.toml is missing', 'Copy paddock/lint/clippy.toml and word the reasons for this app.'));
    else {
      const preset = readFileSync(join(paddock, 'lint', 'clippy.toml'), 'utf8');
      for (const key of ['disallowed-methods', 'disallowed-types']) {
        const have = new Set(disallowedPaths(clippy, key));
        for (const p of disallowedPaths(preset, key)) {
          if (!have.has(p) && !clippyNotYet.includes(p)) out.push(error('C13', 40, at('clippy.toml'), 1, `${key} lacks ${p}`, 'Add the entry from paddock/lint/clippy.toml (the reason may name this app\'s helper).'));
        }
      }
    }
    return out;
  },
};
