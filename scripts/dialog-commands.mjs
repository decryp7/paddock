#!/usr/bin/env node
// The nightly dialog command check (ADR 0026 §3, decision D4): every command a dialog contract declares was sent by some
// run (the recorder's dialog-commands-seen.json files, unioned), unless its entry is listed as `unexercised` with a written
// reason; the number of `unexercised` entries may only fall (the committed baseline). With --visited, every dialog of the
// contracts was opened by the e2e (`openEveryDialog`'s file). It never runs in affected-only CI (ER-7): the comparison
// needs the union of every run.
//
// Usage: node scripts/dialog-commands.mjs --contracts <dialog-contracts.ts>... --seen <seen.json>...
//          --unexercised <dialog-commands-unexercised.json> --baseline <dialog-commands-unexercised.count> [--visited <visited.json>]
import { readFileSync } from 'node:fs';
import { isMain } from '../conformance/src/util/is-main.mjs';

/** The dialog ids and commands of a generated `dialog-contracts.ts` (`paddock_dialogs::ts::contracts_module`'s shape). */
export function contractsOf(text) {
  const out = {};
  for (const m of text.matchAll(/^ {2}'([a-z0-9.-]+)': \{\n([\s\S]*?)\n {2}\},$/gm)) {
    const commands = /^ {4}commands: \[(.*)\],$/m.exec(m[2])?.[1] ?? '';
    out[m[1]] = [...commands.matchAll(/'([^']+)'/g)].map((c) => c[1]);
  }
  return out;
}

/** Whether `seen` holds a command covered by `declared` (an exact name, or a `prefix*`). */
const sentFor = (declared, seen) => (declared.endsWith('*') ? seen.some((s) => s.startsWith(declared.slice(0, -1))) : seen.includes(declared));

/**
 * The problems of the check: declared commands no run sent and no reason excuses, reasons that are blank or name
 * nothing declared, a count of reasons above the baseline, and dialogs no e2e visited.
 */
export function dialogCommandProblems({ contracts, seen, unexercised, baseline, visited = null }) {
  const problems = [];
  for (const [id, declared] of Object.entries(contracts)) {
    const got = seen[id] ?? [];
    for (const c of declared) {
      if (c.startsWith('plugin:') || sentFor(c, got)) continue;
      const reason = unexercised[id]?.[c];
      if (typeof reason !== 'string' || !reason.trim()) problems.push(`${id} declares ${c}, which no run sent: exercise it, or list it as unexercised with the reason`);
    }
  }
  let count = 0;
  for (const [id, entries] of Object.entries(unexercised)) {
    for (const [c, reason] of Object.entries(entries)) {
      count++;
      if (!contracts[id]?.includes(c)) problems.push(`unexercised names ${id} ${c}, which no contract declares: remove it`);
      else if (typeof reason !== 'string' || !reason.trim()) problems.push(`unexercised ${id} ${c} gives no reason`);
    }
  }
  if (count > baseline) problems.push(`${count} unexercised commands, above the baseline of ${baseline}: the count may only fall (ADR 0026 D4)`);
  if (visited) for (const id of Object.keys(contracts)) if (!visited.includes(id)) problems.push(`no e2e opened the dialog ${id} (openEveryDialog)`);
  return { problems, count };
}

function args(argv) {
  const out = { contracts: [], seen: [], unexercised: null, baseline: null, visited: null };
  let key = null;
  for (const a of argv) {
    if (a.startsWith('--')) key = a.slice(2);
    else if (key === 'contracts' || key === 'seen') out[key].push(a);
    else if (key) out[key] = a;
  }
  if (!out.contracts.length || !out.unexercised || !out.baseline) throw new Error('usage: --contracts <file>... --seen <file>... --unexercised <file> --baseline <file> [--visited <file>]');
  return out;
}

export function main(argv = process.argv.slice(2), read = (p) => readFileSync(p, 'utf8'), log = console) {
  const a = args(argv);
  const contracts = Object.assign({}, ...a.contracts.map((p) => contractsOf(read(p))));
  const seen = {};
  for (const p of a.seen) for (const [id, list] of Object.entries(JSON.parse(read(p)).seen ?? {})) seen[id] = [...new Set([...(seen[id] ?? []), ...list])];
  const unexercised = JSON.parse(read(a.unexercised));
  const baseline = Number(read(a.baseline).trim());
  const visited = a.visited ? JSON.parse(read(a.visited)).visited : null;
  const { problems, count } = dialogCommandProblems({ contracts, seen, unexercised, baseline, visited });
  for (const p of problems) log.error(p);
  if (!problems.length) log.log(`dialog commands: every declared command was sent or has a reason (${count} unexercised, baseline ${baseline})`);
  return problems.length ? 1 : 0;
}

if (isMain(import.meta.url)) process.exitCode = main();
