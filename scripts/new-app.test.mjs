// Tests of scripts/new-app.mjs: its pure parts, and a real run into a temp folder (never next to the repository).
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, test } from 'node:test';
import { angularJson, changelog, crateNames, createApp, leftovers, parseArgs, registryDependencies, rewrite, tsconfigJson } from './new-app.mjs';

const scratch = mkdtempSync(join(tmpdir(), 'paddock-new-app-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

describe('the command line', () => {
  test('takes an id and a name, with an identifier derived from the id', () => {
    assert.deepEqual(parseArgs(['pacenotes', 'Pacenotes']), { id: 'pacenotes', name: 'Pacenotes', identifier: 'net.decryptology.pacenotes', dest: null });
    assert.deepEqual(parseArgs(['my-app', 'My App', '--dest', '/x', '--identifier', 'com.example.my-app']), {
      id: 'my-app',
      name: 'My App',
      identifier: 'com.example.my-app',
      dest: '/x',
    });
  });

  test('refuses what an app cannot be called, saying what to fix', () => {
    assert.throws(() => parseArgs(['pacenotes']), /usage/);
    assert.throws(() => parseArgs(['Pacenotes', 'P']), /lower-case/);
    assert.throws(() => parseArgs(['ab', 'P']), /3 to 32/);
    assert.throws(() => parseArgs(['shell-demo', 'Demo']), /id of your own/);
    assert.throws(() => parseArgs(['pacenotes', 'Pace"notes']), /name/);
    assert.throws(() => parseArgs(['pacenotes', 'P', '--identifier', 'nodots']), /reverse-domain/);
    assert.throws(() => parseArgs(['pacenotes', 'P', '--force']), /unknown option --force/);
    assert.throws(() => parseArgs(['pacenotes', 'P', '--dest']), /needs a value/);
  });
});

describe('the rewrites', () => {
  test('crate and library names follow the id', () => {
    assert.deepEqual(crateNames('pace-notes'), { crate: 'pace-notes-app', lib: 'pace_notes_app' });
  });

  test('path dependencies on the shell crates become registry ones at major.minor', () => {
    const toml = 'a = { path = "../../../crates/paddock-shell", features = ["tray"] }\nb = { path = "../../../crates/paddock-jobs" }\n';
    assert.equal(
      registryDependencies(toml, { 'paddock-shell': '0.2.0', 'paddock-jobs': '0.1.3' }),
      'a = { version = "0.2", registry = "decryp7", features = ["tray"] }\nb = { version = "0.1", registry = "decryp7" }\n',
    );
    assert.throws(() => registryDependencies(toml, {}), /no version for the crate paddock-shell/);
  });

  test('a rule that no longer matches fails instead of leaving the demo in the copy', () => {
    assert.equal(rewrite('f', 'one two', [['one', '1'], [/two/, '2']]), '1 2');
    assert.throws(() => rewrite('f', 'one', [['three', '3']]), /f: the starter's rewrite of three no longer matches/);
  });

  test('leftovers name the file and what still points at the demo', () => {
    assert.deepEqual(leftovers(new Map([['ok.rs', 'started from Paddock Shell Demo']])), []);
    assert.equal(leftovers(new Map([['bad.rs', 'include_str!("../../../ui/x")']])).length, 1);
    assert.equal(leftovers(new Map([['bad.ts', "const APP_NAME = 'Paddock Shell Demo';"]])).length, 1);
  });

  test('the workspace files are the demo\'s build without this repository\'s layout', () => {
    const demo = { prefix: 'sd', architect: { build: { options: { browser: 'projects/x/src/main.ts', tsConfig: 'x', styles: [] } }, serve: { builder: 'b' } } };
    const a = angularJson(demo);
    assert.equal(a.projects.app.architect.build.options.browser, 'src/main.ts');
    assert.ok(a.projects.app.architect.build.options.styles.includes('node_modules/@decryp7/paddock-ui/css/paddock.css'));
    assert.equal(demo.architect.build.options.browser, 'projects/x/src/main.ts', 'the demo\'s settings are not changed');
    const t = tsconfigJson({ compilerOptions: { strict: true, paths: { x: ['y'] } }, angularCompilerOptions: { a: 1 } });
    assert.deepEqual(t.compilerOptions, { strict: true });
    assert.match(changelog('Pacenotes', '2026-10-24'), /^## \[0\.1\.0\] - 2026-10-24$/m);
  });
});

describe('a new app', () => {
  test('is the demo renamed, on the registries, in the family layout', () => {
    const dest = join(scratch, 'pacenotes');
    assert.equal(createApp({ id: 'pacenotes', name: 'Pacenotes', identifier: 'net.decryptology.pacenotes', dest }, '2026-10-24'), dest);
    const read = (p) => readFileSync(join(dest, p), 'utf8');
    const cargo = read('app/src-tauri/Cargo.toml');
    assert.match(cargo, /^name = "pacenotes-app"$/m);
    assert.match(cargo, /paddock-shell = \{ version = "\d+\.\d+", registry = "decryp7", features = \["tray"(, "native-dialogs")?\] \}/);
    assert.doesNotMatch(cargo, /path = "\.\.\//);
    const conf = JSON.parse(read('app/src-tauri/tauri.conf.json'));
    assert.equal(conf.productName, 'Pacenotes');
    assert.equal(conf.identifier, 'net.decryptology.pacenotes');
    assert.equal(conf.build.frontendDist, '../dist/app/browser');
    assert.match(read('app/src-tauri/src/lib.rs'), /pub const APP_ID: &str = "pacenotes";/);
    assert.match(read('app/src-tauri/src/lib.rs'), /include_str!\("\.\.\/\.\.\/dialog-contracts\.json"\)/);
    assert.match(read('app/src/app/demo-name.ts'), /APP_NAME = 'Pacenotes'/);
    assert.ok(existsSync(join(dest, 'app/src-tauri/icons/tray/pacenotes-tray-18ptTemplate@2x.png')));
    assert.ok(!readdirSync(join(dest, 'app/src-tauri/icons/tray')).some((f) => f.startsWith('shell-demo')));
    assert.ok(existsSync(join(dest, 'LICENSE-MIT')) && existsSync(join(dest, 'CHANGELOG.md')));
    assert.ok(!existsSync(join(dest, 'app/src-tauri/Cargo.lock')), 'the lock file is made by the new app\'s first build');
    assert.ok(!existsSync(join(dest, 'app/src-tauri/target')) && !existsSync(join(dest, 'app/src-tauri/gen')));
    // The repo root comes from the templates: baseline files, an AGENTS.md of shared blocks, the README skeleton.
    assert.match(read('rustfmt.toml'), /max_width = 140/);
    assert.match(read('cliff.toml'), /All notable changes to Pacenotes/);
    assert.match(read('AGENTS.md'), /<!-- paddock:block rules rev=[0-9a-f]{8} -->/);
    assert.match(read('README.md'), /^# Pacenotes\n/);
    assert.match(read('docs/releasing.md'), /Pacenotes is not code-signed yet/);
    assert.equal(read('app/tsconfig.json').includes('paddock-tsconfig'), false, 'the generator keeps its own tsconfig');
    const pkg = JSON.parse(read('app/package.json'));
    assert.match(pkg.dependencies['@decryp7/paddock-ui'], /^\^\d+\.\d+\.\d+$/);
    assert.equal(JSON.parse(read('app/angular.json')).projects.app.architect.build.options.outputPath, 'dist/app');
  });

  test('never writes into a folder that has something in it', () => {
    const dest = join(scratch, 'taken');
    createApp({ id: 'taken', name: 'Taken', identifier: 'net.decryptology.taken', dest: join(dest) });
    const marker = join(dest, 'mine.txt');
    writeFileSync(marker, 'keep');
    assert.throws(() => createApp({ id: 'taken', name: 'Taken', identifier: 'net.decryptology.taken', dest }), /exists and is not empty/);
    assert.equal(readFileSync(marker, 'utf8'), 'keep');
  });
});
