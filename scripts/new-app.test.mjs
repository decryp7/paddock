// Tests of scripts/new-app.mjs: its pure parts, and a real run into a temp folder (never next to the repository).
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, test } from 'node:test';
import { angularJson, changelog, crateNames, createApp, leftovers, parseArgs, registryDependencies, rewrite, tsconfigJson } from './new-app.mjs';

const defaults = { template: 'shell-demo', accent: 'graphite' };

const scratch = mkdtempSync(join(tmpdir(), 'paddock-new-app-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

describe('the command line', () => {
  test('takes the template and the accent', () => {
    const a = parseArgs(['sightr', 'Sightr', '--template', 'workbench-demo', '--accent', 'sighter']);
    assert.equal(a.template, 'workbench-demo');
    assert.equal(a.accent, 'sighter');
  });

  test('takes an id and a name, with an identifier derived from the id', () => {
    assert.deepEqual(parseArgs(['pacenotes', 'Pacenotes']), { id: 'pacenotes', name: 'Pacenotes', identifier: 'net.decryptology.pacenotes', dest: null, ...defaults });
    assert.deepEqual(parseArgs(['my-app', 'My App', '--dest', '/x', '--identifier', 'com.example.my-app']), {
      id: 'my-app',
      name: 'My App',
      identifier: 'com.example.my-app',
      dest: '/x',
      ...defaults,
    });
  });

  test('refuses what an app cannot be called, saying what to fix', () => {
    assert.throws(() => parseArgs(['pacenotes']), /usage/);
    assert.throws(() => parseArgs(['Pacenotes', 'P']), /lower-case/);
    assert.throws(() => parseArgs(['ab', 'P']), /3 to 32/);
    assert.throws(() => parseArgs(['shell-demo', 'Demo']), /id of your own/);
    assert.throws(() => parseArgs(['workbench-demo', 'Demo']), /workbench-demo is a demo/);
    assert.throws(() => parseArgs(['pacenotes', 'P', '--template', 'nope']), /template "nope" must be one of shell-demo, workbench-demo/);
    assert.throws(() => parseArgs(['pacenotes', 'P', '--accent', 'pink']), /accent "pink" must be one of/);
    assert.throws(() => parseArgs(['pacenotes', 'Pace"notes']), /name/);
    assert.throws(() => parseArgs(['pacenotes', 'P', '--identifier', 'nodots']), /reverse-domain/);
    assert.throws(() => parseArgs(['pacenotes', 'P', '--force']), /unknown option --force/);
    assert.throws(() => parseArgs(['pacenotes', 'P', '--template']), /needs a value/);
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
    assert.match(changelog('Sightr', '2026-10-24', 'workbench-demo'), /Sightr, started from Paddock Workbench Demo: the Paddock workbench/);
  });

  test('the workbench template adds the dock stylesheet and the public folder, and has its own leftovers', () => {
    const demo = { prefix: 'wd', architect: { build: { options: { browser: 'x', tsConfig: 'x', styles: [] } }, serve: { builder: 'b' } } };
    const a = angularJson(demo, 'workbench-demo').projects.app.architect.build.options;
    assert.ok(a.styles.includes('node_modules/dockview/dist/styles/dockview.css'));
    assert.deepEqual(a.assets, [{ glob: '**/*', input: 'public' }]);
    assert.equal(angularJson(demo).projects.app.architect.build.options.assets, undefined);
    assert.equal(leftovers(new Map([['a.rs', 'use paddock_workbench_demo::run;']]), 'workbench-demo').length, 1);
    assert.deepEqual(leftovers(new Map([['a.rs', 'use paddock_workbench_demo::run;']])), [], 'the shell template does not look for the workbench demo');
  });
});

describe('a new app', () => {
  test('is the demo renamed, on the registries, in the family layout', () => {
    const dest = join(scratch, 'pacenotes');
    assert.equal(createApp({ id: 'pacenotes', name: 'Pacenotes', identifier: 'net.decryptology.pacenotes', dest }, '2026-10-24'), dest);
    const read = (p) => readFileSync(join(dest, p), 'utf8');
    const cargo = read('app/src-tauri/Cargo.toml');
    assert.match(cargo, /^name = "pacenotes-app"$/m);
    assert.match(cargo, /paddock-shell = \{ version = "\d+\.\d+", registry = "decryp7", features = \["tray"(, "native-dialogs")?(, "os-notify")?\] \}/);
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
    // An e2e smoke suite on the shared preset: the window, axe in both themes, no horizontal scroll at 320 px.
    assert.equal(pkg.scripts.e2e, 'playwright test --project=chromium');
    // The shell demo has no unit-test target of its own, so there is no `test` script that could only fail.
    assert.equal(pkg.scripts.test, undefined);
    assert.equal(pkg.devDependencies.jsdom, undefined);
    assert.equal(JSON.parse(read('app/angular.json')).projects.app.architect.test, undefined);
    assert.match(read('app/playwright.config.ts'), /paddockConfig\(\{ app: 'pacenotes', defaultPort: 4200, buildGuard: 'none', serve: \(port\) => `npx ng serve --port \$\{port\}` \}\)/);
    const smoke = read('app/e2e/smoke.spec.ts');
    assert.match(smoke, /page\.locator\('#pd-main'\)/);
    assert.match(smoke, /axeInThemes\(page\)/);
    assert.match(smoke, /expectNoHorizontalScroll\(page, '320 px'\)/);
    assert.doesNotMatch(smoke, /WorkbenchApp/);
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

describe('a new app from the workbench demo', () => {
  const dest = join(scratch, 'sightr');
  const args = { id: 'sightr', name: 'Sightr', identifier: 'net.decryptology.sightr', dest, template: 'workbench-demo', accent: 'sighter' };
  const read = (p) => readFileSync(join(dest, p), 'utf8');

  test('is the IDE-style demo renamed, on the registries, with the name, id and accent of the new app', () => {
    assert.equal(createApp(args, '2026-10-24'), dest);
    const cargo = read('app/src-tauri/Cargo.toml');
    assert.match(cargo, /^name = "sightr-app"$/m);
    assert.match(cargo, /^name = "sightr_app"$/m);
    assert.match(cargo, /paddock-workbench = \{ version = "\d+\.\d+", registry = "decryp7", features = \["tauri"\] \}/);
    assert.doesNotMatch(cargo, /path = "\.\.\//);
    assert.doesNotMatch(cargo, /^repository = /m);
    const conf = JSON.parse(read('app/src-tauri/tauri.conf.json'));
    assert.equal(conf.productName, 'Sightr');
    assert.equal(conf.identifier, 'net.decryptology.sightr');
    assert.equal(conf.app.windows[0].title, 'Sightr');
    assert.equal(conf.build.frontendDist, '../dist/app/browser');
    assert.equal(conf.build.beforeBuildCommand, 'npm run build');
    const lib = read('app/src-tauri/src/lib.rs');
    assert.match(lib, /pub const APP_ID: &str = "sightr";/);
    assert.match(lib, /pub const APP_NAME: &str = "Sightr";/);
    assert.match(lib, /SIGHTR_HOME/);
    assert.match(read('app/src-tauri/src/main.rs'), /sightr_app::run\(\);/);
    assert.match(read('app/src-tauri/tests/app.rs'), /use sightr_app::/);
    assert.match(read('app/src-tauri/tests/app.rs'), /app_dir\(\)\.join\("\.\."\)/);
    assert.match(read('app/src/app/params.ts'), /: 'sighter';/);
    assert.match(read('app/src/index.html'), /<title>Sightr<\/title>/);
    assert.match(read('app/src/main-window.ts'), /appName: 'Sightr'/);
    assert.match(read('app/src/app/views/orientation-view.ts'), /<h1>Sightr<\/h1>/);
    assert.match(read('app/src-tauri/deny.toml'), /^# cargo-deny for Sightr /);
  });

  test('keeps the demo\'s window kinds, capabilities, drag ghost and icons, and none of the shell demo\'s agent or tray', () => {
    for (const f of ['app/src/dialog-window.ts', 'app/src/popout-window.ts', 'app/src/main-window.ts', 'app/public/drag-ghost.html', 'app/src-tauri/capabilities/popout.json', 'app/src-tauri/icons/icon.icns']) {
      assert.ok(existsSync(join(dest, f)), f);
    }
    assert.ok(!existsSync(join(dest, 'app/src-tauri/agent')) && !existsSync(join(dest, 'app/src-tauri/icons/tray')));
    const ng = JSON.parse(read('app/angular.json')).projects.app.architect.build.options;
    assert.deepEqual(ng.assets, [{ glob: '**/*', input: 'public' }]);
    assert.equal(JSON.parse(read('app/package.json')).description, 'Sightr: the UI (Angular, @decryp7/paddock-ui). Started from Paddock Workbench Demo.');
    assert.match(read('CHANGELOG.md'), /started from Paddock Workbench Demo/);
    assert.ok(!existsSync(join(dest, 'app/src-tauri/Cargo.lock')));
    const smoke = read('app/e2e/smoke.spec.ts');
    assert.match(smoke, /pd-dock \[role=tab\]/);
    assert.match(smoke, /workbench\.palette\.open\(\)/);
    assert.match(read('app/playwright.config.ts'), /app: 'sightr'/);
    // The demo's unit tests come along, with their own target, setup and tsconfig.
    const test = JSON.parse(read('app/angular.json')).projects.app.architect.test;
    assert.equal(test.builder, '@angular/build:unit-test');
    assert.deepEqual([test.options.tsConfig, test.options.setupFiles], ['tsconfig.spec.json', ['src/test-setup.ts']]);
    assert.equal(JSON.parse(read('app/package.json')).scripts.test, 'ng test --watch=false');
    assert.ok(JSON.parse(read('app/package.json')).devDependencies.jsdom, 'the unit tests need a DOM');
    assert.match(read('app/tsconfig.spec.json'), /"extends": "\.\/tsconfig\.json"/);
    assert.ok(existsSync(join(dest, 'app/src/test-setup.ts')) && existsSync(join(dest, 'app/src/app/demo.contributions.spec.ts')));
  });

  test('an unknown template stops the run before anything is written', () => {
    const other = join(scratch, 'stale');
    assert.throws(() => createApp({ ...args, dest: other, template: 'nope' }), /unknown template nope/);
    assert.ok(!existsSync(other));
  });
});
