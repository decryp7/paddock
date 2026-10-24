import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { auditDialog, main, parseTable, readConfig, run } from './paddock-check-dialog-commands.mjs';
import { createAnalysis, readRegistry } from './dialog-commands-analysis.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const dirs = [];
after(() => dirs.forEach((d) => rmSync(d, { recursive: true, force: true })));

test('reads Rust command tables and generated names modules', () => {
  const t = parseTable(`
    Query [WINDOW] => workspace::summary,
    Mutation [MAIN] => window::resolve_close,
    Quiet [MAIN | DIALOG] limit 4 * MIB => a::b::both,
  demo_status: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
  demo_reset: { kind: 'mutation', windows: ['main'], limit: 262144 },`);
  assert.deepEqual(t.get('summary'), ['main', 'popout', 'dialog']);
  assert.deepEqual(t.get('resolve_close'), ['main']);
  assert.deepEqual(t.get('both'), ['main', 'dialog']);
  assert.deepEqual(t.get('demo_status'), ['main', 'dialog']);
  assert.deepEqual(t.get('demo_reset'), ['main']);
});

const table = parseTable('Query [WINDOW] => a::read_it,\nMutation [WINDOW] => a::write_it,\nMutation [MAIN] => a::main_only,\nQuiet [WINDOW] => a::base_one,');
const reached = (...names) => new Map(names.map((n) => [n, 'x.ts:1']));

test('fails a dialog that reaches a command it does not declare, and one that declares a command nothing reaches', () => {
  const audit = (declared, r, exceptions) => auditDialog({ id: 'd', declared, reached: reached(...r), base: ['base_one'], table, exceptions });
  assert.deepEqual(audit(['read_it', 'plugin:dialog|open'], ['read_it', 'base_one']), []);
  assert.match(audit([], ['read_it'])[0], /calls read_it .* does not declare it/);
  assert.match(audit([], ['nope'])[0], /calls nope .* not in the command table/);
  assert.match(audit(['read_it', 'write_it'], ['read_it'])[0], /declares write_it but nothing it reaches calls it/);
  assert.deepEqual(audit(['read_it'], ['read_it', 'write_it'], { reached: ['write_it'] }), []);
  assert.deepEqual(audit(['read_it', 'write_it'], ['read_it'], { declared: ['write_it'] }), []);
});

test('fails a command outside the table and one the table does not open to dialogs; a prefix must match', () => {
  const problems = auditDialog({ id: 'd', declared: ['main_only', 'nope', 'zz_*'], reached: reached('main_only', 'nope'), base: [], table });
  assert.ok(problems.some((p) => /declares main_only, which the command table does not let a dialog call/.test(p)), problems.join('\n'));
  assert.ok(problems.some((p) => /declares nope, which is not in the command table/.test(p)), problems.join('\n'));
  assert.ok(problems.some((p) => /declares zz_\* but nothing/.test(p)), problems.join('\n'));
});

/** A small app to check: a registry, components, a base provider set, a table and a config. */
function app(files, config = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'pd-dlgcmd-'));
  dirs.push(dir);
  mkdirSync(join(dir, 'src'));
  writeFileSync(join(dir, 'tsconfig.json'), JSON.stringify({ compilerOptions: { target: 'ES2022', module: 'ESNext', moduleResolution: 'bundler', strict: true, experimentalDecorators: true, noEmit: true }, include: ['src/**/*.ts'] }));
  writeFileSync(
    join(dir, 'src/prelude.ts'),
    'export declare function inject<T>(t: abstract new (...a: any[]) => T): T;\nexport declare function Component(o: { template?: string; imports?: unknown[] }): ClassDecorator;\nexport declare function defineDialog(d: object): object;\nexport declare function invoke<T>(c: string, a?: object): Promise<T>;',
  );
  writeFileSync(join(dir, 'src/table.ts'), "export const APP_COMMANDS = {\n  read_it: { kind: 'query', windows: ['main', 'dialog'], limit: 1 },\n  write_it: { kind: 'mutation', windows: ['main', 'dialog'], limit: 1 },\n  ui_log: { kind: 'quiet', windows: ['main', 'dialog'], limit: 1 },\n  main_only: { kind: 'mutation', windows: ['main'], limit: 1 },\n} as const;");
  for (const [name, text] of Object.entries(files)) writeFileSync(join(dir, 'src', name), text);
  const path = join(dir, 'paddock-dialog-commands.json');
  writeFileSync(path, JSON.stringify({ tsconfig: 'tsconfig.json', registry: ['src/registry.ts'], table: ['src/table.ts'], ...config }));
  return path;
}

const COMPONENTS = {
  'engine.ts': "import { invoke } from './prelude';\nexport class Engine { read() { return invoke('read_it'); } write() { return invoke('write_it'); } }\nexport const ENGINE_PROVIDERS = [Engine];",
  'reader.ts': "import { inject, Component } from './prelude';\nimport { Engine } from './engine';\n@Component({ template: '<button (click)=\"go()\">Go</button>' })\nexport class ReaderDialog { private readonly e = inject(Engine); go() { return this.e.read(); } }",
  'base.ts': "import { invoke } from './prelude';\nexport function dialogBase() { void invoke('ui_log'); return []; }",
};

test('passes an app whose declared lists match what its dialogs reach, through injected providers and the template', () => {
  const path = app({
    ...COMPONENTS,
    'registry.ts': "import { defineDialog } from './prelude';\nexport const D = [defineDialog({ id: 'a.reader', kind: 'viewer', commands: ['read_it'], providers: () => import('./engine').then((m) => m.ENGINE_PROVIDERS), load: () => import('./reader').then((m) => m.ReaderDialog) })];",
  }, { base: ['ui_log'], baseEntries: [['src/base.ts', 'dialogBase']] });
  const r = run(readConfig(path));
  assert.deepEqual(r.problems, []);
  assert.deepEqual(r.derived, { 'a.reader': ['read_it'] });
  const out = [];
  assert.equal(main(['--config', path], { log: (m) => out.push(m), error: (m) => out.push(m) }), 0);
  assert.match(out[0], /1 dialogs, 4 commands in the table/);
  assert.equal(main(['--config', path, '--json'], { log: (m) => out.push(m), error: () => undefined }), 0);
  assert.deepEqual(JSON.parse(out[1]), { 'a.reader': ['read_it'] });
});

test('fails a planted undeclared command, a stale declaration, a host-only command and a base set out of date', () => {
  const path = app({
    ...COMPONENTS,
    'sneaky.ts': "import { Component, invoke } from './prelude';\n@Component({ template: '' })\nexport class SneakyDialog { go() { return invoke('write_it'); } wipe() { return invoke('main_only'); } dyn(n: string) { const x = String(n) + '!'; return invoke(x); } }",
    'registry.ts': "import { defineDialog } from './prelude';\nexport const D = [defineDialog({ id: 'a.sneaky', kind: 'tool', commands: ['read_it'], load: () => import('./sneaky').then((m) => m.SneakyDialog) }), defineDialog({ id: 'a.broken', kind: 'tool', commands: [], load: () => Promise.resolve(null) })];",
  }, { base: ['gone_cmd'], baseEntries: [['src/base.ts', 'dialogBase'], ['src/base.ts', 'missing']], hostOnly: [['src/engine.ts', 'Nope']], boundary: [['src/engine.ts', 'AlsoNope']] });
  const { problems } = run(readConfig(path));
  const want = [
    /boundary names AlsoNope/,
    /hostOnly names Nope/,
    /baseEntries names missing/,
    /base providers reach ui_log/,
    /"base" lists gone_cmd/,
    /a\.sneaky: a command is called by a computed name/,
    /a\.sneaky: calls write_it .* does not declare it/,
    /a\.sneaky: calls main_only .* does not declare it/,
    /a\.sneaky: declares read_it but nothing it reaches calls it/,
    /a\.broken: its load\(\) does not name a component class/,
  ];
  for (const w of want) assert.ok(problems.some((p) => w.test(p)), `${w}\n${problems.join('\n')}`);
  const errors = [];
  assert.equal(main(['--config', path], { log: () => undefined, error: (m) => errors.push(m) }), 1);
  assert.match(errors[0], /a\.sneaky/);
});

test('a host-only member and a boundary stop the walk', () => {
  const path = app({
    'host.ts': "import { invoke } from './prelude';\nexport class Host { act() { return invoke('main_only'); } read() { return invoke('read_it'); } }\nexport function shared() { return invoke('write_it'); }",
    'dlg.ts': "import { inject, Component } from './prelude';\nimport { Host, shared } from './host';\n@Component({ template: '' })\nexport class D { private readonly h = inject(Host); go() { this.h.act(); this.h.read(); shared(); } }",
    'registry.ts': "import { defineDialog } from './prelude';\nexport const D = [defineDialog({ id: 'a.d', kind: 'tool', commands: ['read_it'], load: () => import('./dlg').then((m) => m.D) })];",
  }, { hostOnly: [['src/host.ts', 'Host', 'act']], boundary: [['src/host.ts', 'shared']] });
  assert.deepEqual(run(readConfig(path)).problems, []);
});

test('refuses a config without a tsconfig and a registry outside the program', () => {
  const dir = mkdtempSync(join(tmpdir(), 'pd-dlgcmd-'));
  dirs.push(dir);
  writeFileSync(join(dir, 'c.json'), '{}');
  assert.throws(() => readConfig(join(dir, 'c.json')), /"tsconfig"/);
  const path = app({ 'registry.ts': 'export {};' });
  const a = createAnalysis(readConfig(path).tsconfig);
  assert.throws(() => readRegistry(a, join(dir, 'nowhere.ts')), /not part of the program/);
  assert.ok(ts.version);
});

test("the shell demo's dialogs declare exactly what they reach", () => {
  const r = run(readConfig(join(here, '../../shell-demo/paddock-dialog-commands.json')));
  assert.deepEqual(r.problems, []);
  assert.deepEqual(r.derived['sd.inspector'], ['demo_status']);
  assert.equal(r.dialogs, 5);
});
