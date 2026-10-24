#!/usr/bin/env node
// paddock-check-dialog-commands: the dialog command contract (Paddock ADR 0009 §4; ported from Debrief's
// scripts/check-dialog-commands.mjs, ADR 0054). Every dialog declares, in its `defineDialog`, the IPC commands its window
// may call (`commands`); Rust's guard allows exactly those plus the base sets. A list that is too short is a dialog that
// silently does nothing, one that is too long is a hole. This derives the real set by a type-checked call-graph walk
// (`dialog-commands-analysis.mjs`) and fails when
//   1. a dialog reaches a command it does not declare,
//   2. a dialog declares a command nothing it reaches calls,
//   3. a declared command is not in the command table, or the table does not let dialogs call it,
//   4. the app's dialog-window base providers reach a command the base list does not name, or the base list names one
//      they never call.
// The boot harness (`bootDialog` in @decryp7/paddock-ui/shell/testing) checks what a window really sends at start; this
// checks what a click could send.
//
// Usage: paddock-check-dialog-commands [--config paddock-dialog-commands.json] [--json]
// The config (paths relative to it):
//   {
//     "tsconfig": "tsconfig.app.json",                       // the app's program
//     "registry": ["src/app/app.contributions.ts"],          // files with defineDialog({...}) entries
//     "table": ["src/app/core/generated/app-commands.ts"],   // generated names modules and/or Rust files with command_table!
//     "base": ["ui_log"],                                    // the app's commands every dialog window may call
//     "baseEntries": [["src/app/dialog-base.ts", "dialogBaseProviders"]],   // what every dialog window runs
//     "hostOnly": [["src/app/dock/dock.service.ts", "DockService", "member?"]],  // never called from a dialog window
//     "boundary": [["src/app/shared-state.ts", "sharedUiState"]],          // walked no further (listed in base by hand)
//     "exceptions": { "<dialog id>": { "reached": ["cmd"], "declared": ["cmd"] } }  // reviewed, with a comment why
//   }
import { readFileSync, realpathSync } from 'node:fs';
import { dirname, isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { createAnalysis, readRegistry } from './dialog-commands-analysis.mjs';

/** The commands of `paddock_dialogs`' base set: every dialog window may call them whatever its contract says. */
export const SHELL_BASE_COMMANDS = ['dialog_take_input', 'dialog_close', 'dialog_fit_height'];

const CLASSES = { MAIN: 'main', POPOUT: 'popout', DIALOG: 'dialog' };

/**
 * command name -> the window classes (`main`, `popout`, `dialog`) that may call it, from a Rust `command_table!` or a
 * generated names module (`demo_status: { kind: 'query', windows: ['main', 'dialog'], ... }`).
 */
export function parseTable(text) {
  const out = new Map();
  for (const m of text.matchAll(/^\s*(?:Query|Mutation|Quiet)\s*\[([^\]]+)\][^=\n]*=>\s*([\w:]+)\s*,/gm)) {
    const name = m[2].split('::').pop();
    const classes = m[1].split('|').map((c) => c.trim());
    out.set(name, classes.includes('WINDOW') ? ['main', 'popout', 'dialog'] : classes.map((c) => CLASSES[c]).filter(Boolean));
  }
  for (const m of text.matchAll(/^\s*['"]?([a-z][a-z0-9_]*)['"]?\s*:\s*\{\s*kind:\s*['"]\w+['"],\s*windows:\s*\[([^\]]*)\]/gm)) {
    out.set(m[1], [...m[2].matchAll(/['"](\w+)['"]/g)].map((x) => x[1]));
  }
  return out;
}

const covers = (declared, command) => declared.some((d) => (d.endsWith('*') ? command.startsWith(d.slice(0, -1)) : d === command));

/** The problems of one dialog: derived `reached` (command -> where) against `declared`, `base` and the `table`. */
export function auditDialog({ id, declared, reached, base, table, exceptions = {} }) {
  const problems = [];
  const skip = exceptions.reached ?? [];
  const extraOk = exceptions.declared ?? [];
  for (const c of [...reached.keys()].filter((c) => !covers(base, c) && !skip.includes(c))) {
    if (!table.has(c)) problems.push(`${id}: calls ${c} (${reached.get(c)}), which is not in the command table`);
    else if (!covers(declared, c)) problems.push(`${id}: calls ${c} (${reached.get(c)}) but does not declare it in 'commands'`);
  }
  for (const d of declared) {
    if (d.startsWith('plugin:')) continue;
    const matches = d.endsWith('*') ? [...reached.keys()].some((c) => c.startsWith(d.slice(0, -1))) : reached.has(d);
    if (!matches && !extraOk.includes(d)) problems.push(`${id}: declares ${d} but nothing it reaches calls it`);
    if (!d.endsWith('*')) {
      const classes = table.get(d);
      if (!classes) problems.push(`${id}: declares ${d}, which is not in the command table`);
      else if (!classes.includes('dialog')) problems.push(`${id}: declares ${d}, which the command table does not let a dialog call (add DIALOG to its entry, reviewed)`);
    }
  }
  return problems;
}

/** Reads a config file; paths in it are resolved against its folder. */
export function readConfig(path) {
  const dir = dirname(resolve(path));
  const raw = JSON.parse(readFileSync(path, 'utf8'));
  const at = (p) => (isAbsolute(p) ? p : resolve(dir, p));
  const list = (v) => (Array.isArray(v) ? v : []);
  if (typeof raw.tsconfig !== 'string') throw new Error(`${path}: "tsconfig" names the app's tsconfig`);
  return {
    tsconfig: at(raw.tsconfig),
    registry: list(raw.registry).map(at),
    table: list(raw.table).map(at),
    base: list(raw.base),
    baseEntries: list(raw.baseEntries).map(([f, n]) => [at(f), n]),
    hostOnly: list(raw.hostOnly).map(([f, n, m]) => [at(f), n, m]),
    boundary: list(raw.boundary).map(([f, n]) => [at(f), n]),
    exceptions: raw.exceptions ?? {},
  };
}

/** Runs the check: `{ problems, derived, dialogs, commands }` (`derived`: what each dialog reaches beyond the base sets). */
export function run(config) {
  const analysis = createAnalysis(config.tsconfig);
  const table = new Map();
  for (const f of config.table) for (const [k, v] of parseTable(readFileSync(f, 'utf8'))) table.set(k, v);
  const known = new Set(table.keys());
  const base = [...SHELL_BASE_COMMANDS, ...config.base];
  const problems = [];
  const derived = {};
  const find = (file, name) => {
    const sf = analysis.program.getSourceFile(file);
    let out = null;
    const visit = (n) => {
      if ((ts.isClassDeclaration(n) || ts.isFunctionDeclaration(n) || ts.isVariableDeclaration(n)) && n.name?.getText() === name) out = n;
      ts.forEachChild(n, visit);
    };
    if (sf) visit(sf);
    return out;
  };
  const boundary = new Set();
  for (const [file, name] of config.boundary) {
    const n = find(file, name);
    if (n) boundary.add(n);
    else problems.push(`boundary names ${name} (${file}), which does not exist: update the config`);
  }
  for (const [file, name, member] of config.hostOnly) {
    const c = find(file, name);
    const target = member ? c?.members?.find((m) => m.name?.getText?.() === member) : c;
    if (target) boundary.add(target);
    else problems.push(`hostOnly names ${name}${member ? `.${member}` : ''} (${file}), which does not exist: update the config`);
  }
  // What every dialog window runs besides the shell: its commands are the base set.
  const entries = [];
  for (const [file, name] of config.baseEntries) {
    const n = find(file, name);
    if (n) entries.push(n);
    else problems.push(`baseEntries names ${name} (${file}), which does not exist: update the config`);
  }
  const baseReached = analysis.reach(entries, known, boundary).commands;
  for (const c of baseReached.keys()) if (!covers(base, c)) problems.push(`the dialog windows' base providers reach ${c} (${baseReached.get(c)}); add it to "base"`);
  for (const c of config.base) if (!c.endsWith('*') && !baseReached.has(c)) problems.push(`"base" lists ${c}, which the base providers never call`);

  let dialogs = 0;
  for (const file of config.registry) {
    for (const entry of readRegistry(analysis, file)) {
      dialogs++;
      const cls = entry.component ? analysis.declsOf(entry.component).find(ts.isClassDeclaration) : null;
      if (!cls) {
        problems.push(`${entry.id}: its load() does not name a component class`);
        continue;
      }
      const roots = analysis.componentEntries(cls);
      if (entry.providers) for (const d of analysis.declsOf(entry.providers)) roots.push({ decl: d });
      const { commands, dynamic } = analysis.reach(roots, known, boundary);
      for (const where of dynamic) problems.push(`${entry.id}: a command is called by a computed name (${where}); call it by a literal so it can be declared`);
      derived[entry.id] = [...commands.keys()].filter((c) => !covers(base, c)).sort();
      problems.push(...auditDialog({ id: entry.id, declared: entry.commands, reached: commands, base, table, exceptions: config.exceptions[entry.id] }));
    }
  }
  return { problems, derived, dialogs, commands: table.size };
}

export function main(argv = process.argv.slice(2), log = console) {
  const at = argv.indexOf('--config');
  const path = at >= 0 ? argv[at + 1] : 'paddock-dialog-commands.json';
  const r = run(readConfig(path));
  if (argv.includes('--json')) {
    log.log(JSON.stringify(r.derived, null, 2));
    return 0;
  }
  if (r.problems.length) {
    log.error(r.problems.join('\n'));
    return 1;
  }
  log.log(`dialog commands: ${r.dialogs} dialogs, ${r.commands} commands in the table, every declared list matches what the dialogs reach`);
  return 0;
}

/** Whether this module is the program (also when started through the `node_modules/.bin` link). */
function isMain() {
  try {
    return !!process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
}

if (isMain()) process.exitCode = main();
