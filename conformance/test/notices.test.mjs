import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { ALLOWED } from '../src/notices/licences.mjs';
import { cargoAbout, main, parseArgs } from '../src/notices/cli.mjs';
import { lf, lockProduction, nonPermissive, normalise, npmPackages, render, rustCrates, spdxIds } from '../src/notices/generate.mjs';
import { makeApp, PADDOCK, readText } from './helpers.mjs';

const aboutJson = (crates) => ({
  crates: [{ package: { name: 'app', version: '1.0.0', source: null }, license: 'MIT' }, ...crates.map(([name, version, license]) => ({ package: { name, version, source: 'registry+x' }, license }))],
  licenses: [
    { id: 'MIT', text: 'MIT License\r\n\r\nPermission is hereby granted  \r\n', used_by: crates.filter((c) => c[2] === 'MIT').map(([name, version]) => ({ crate: { name, version } })) },
    { id: 'Apache-2.0', text: 'Apache text', used_by: crates.filter((c) => c[2] === 'Apache-2.0').map(([name, version]) => ({ crate: { name, version } })) },
  ].filter((l) => l.used_by.length),
});

/** An app folder with a lock file and two installed production packages, one dev package, one without a licence file. */
function npmApp() {
  const files = {
    'app/package-lock.json': JSON.stringify({
      packages: {
        '': {},
        'node_modules/left-pad': { version: '1.3.0', license: 'MIT' },
        'node_modules/@scope/chart': { version: '2.0.0', license: 'Apache-2.0' },
        'node_modules/nolicense': { version: '0.1.0' },
        'node_modules/dev-only': { version: '9.9.9', dev: true, license: 'GPL-3.0-only' },
      },
    }),
    'app/node_modules/left-pad/LICENSE': 'MIT License\n\nPermission is hereby granted\n',
    'app/node_modules/left-pad/package.json': '{"license":"MIT"}',
    'app/node_modules/@scope/chart/LICENSE-APACHE': 'Apache text',
    'app/node_modules/@scope/chart/NOTICE': 'Chart notice',
    'app/node_modules/nolicense/package.json': '{"license":{"type":"ISC"}}',
  };
  return makeApp(files);
}

test('the licence list is one list: deny.toml, about.toml and the generator agree', () => {
  const listOf = (file, key) => [...new RegExp(`${key} = \\[([^\\]]*)\\]`).exec(readText(join(PADDOCK, 'templates', 'repo', file)))[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(listOf('deny.toml', 'allow'), ALLOWED);
  assert.deepEqual(listOf('about.toml', 'accepted'), ALLOWED);
});

test('helpers: spdx, line endings, production-only lock entries, permissive check', () => {
  assert.deepEqual(spdxIds('(MIT OR Apache-2.0) AND Unicode-3.0'), ['MIT', 'Apache-2.0', 'Unicode-3.0']);
  assert.equal(lf('\r\n  a  \r\nb\rc\n\n'), '\n  a  \nb\nc\n\n');
  assert.equal(normalise('a  \r\nb\r\n\r\n'), 'a\nb');
  assert.deepEqual(lockProduction({ packages: { '': {}, 'node_modules/a': { version: '1' }, 'node_modules/b': { dev: true }, 'node_modules/c': { devOptional: true } } }).map((p) => p.name), ['a']);
  const pk = (license) => ({ kind: 'rust', name: 'x', version: '1', license, spdx: spdxIds(license) });
  assert.deepEqual(nonPermissive([pk('MIT OR LGPL-2.1-or-later'), pk('GPL-3.0-only'), pk('OFL-1.1')]), ['rust x@1: GPL-3.0-only', 'rust x@1: OFL-1.1']);
});

test('render is deterministic, deduplicates texts and does not depend on input order', () => {
  const rust = rustCrates([aboutJson([['zeta', '1.0.0', 'MIT'], ['alpha', '2.0.0', 'MIT'], ['pad', '3.0.0', 'Apache-2.0']])]);
  assert.equal(rust.packages.length, 3, 'the workspace crate is skipped');
  const a = render(rust.packages, [], 'Pitcrew');
  const b = render([...rust.packages].reverse(), [], 'Pitcrew');
  assert.equal(a.markdown, b.markdown);
  assert.equal(a.counts.texts, 2);
  assert.match(a.markdown, /^# Third-party notices\n\nPitcrew is dual-licensed MIT OR Apache-2.0/);
  assert.ok(a.markdown.indexOf('- alpha 2.0.0') < a.markdown.indexOf('- zeta 1.0.0'));
  assert.match(a.markdown, /Used by \(2\):\n {2}alpha 2\.0\.0, zeta 1\.0\.0/);
  assert.doesNotMatch(a.markdown, /\r/);
});

test('npm packages: licence files found, dev skipped, a missing file is a warning, declared licence read from package.json', () => {
  const { packages, warnings } = npmPackages(join(npmApp(), 'app'));
  assert.deepEqual(packages.map((p) => p.name).sort(), ['@scope/chart', 'left-pad', 'nolicense']);
  const chart = packages.find((p) => p.name === '@scope/chart');
  assert.deepEqual(chart.texts.map((t) => t.id), ['Apache-2.0', 'NOTICE']);
  assert.equal(packages.find((p) => p.name === 'nolicense').license, 'ISC');
  assert.deepEqual(warnings, ['npm nolicense@0.1.0: no licence file found (declared ISC)']);
  assert.throws(() => npmPackages(makeApp({ 'a.txt': 'x' })), /package-lock\.json not found/);
});

test('cli: writes the file, then --check passes; a changed dependency makes --check fail', () => {
  const root = npmApp();
  writeFileSync(join(root, 'about.json'), JSON.stringify(aboutJson([['serde', '1.0.0', 'MIT']])));
  const out = [];
  const io = { log: (s) => out.push(s), err: (s) => out.push(s) };
  const base = ['--app', 'Demo', '--root', root, '--rust-json', 'about.json'];
  assert.equal(main(base, io), 0);
  const file = join(root, 'app/src-tauri/resources/THIRD_PARTY_NOTICES.md');
  assert.match(readFileSync(file, 'utf8'), /Demo is dual-licensed/);
  assert.equal(main([...base, '--check'], io), 0);
  writeFileSync(join(root, 'about.json'), JSON.stringify(aboutJson([['serde', '1.0.0', 'MIT'], ['tokio', '1.0.0', 'MIT']])));
  assert.equal(main([...base, '--check'], io), 1);
  assert.match(out.join('\n'), /out of date/);
  writeFileSync(file, readFileSync(file, 'utf8').replace(/\n/g, '\r\n'));
  writeFileSync(join(root, 'about.json'), JSON.stringify(aboutJson([['serde', '1.0.0', 'MIT']])));
  assert.equal(main([...base, '--check'], io), 0, 'a CRLF checkout compares equal');
});

test('cli: a non-permissive licence fails both modes and is named; a missing file in --check is reported', () => {
  const root = npmApp();
  writeFileSync(join(root, 'about.json'), JSON.stringify({ crates: [{ package: { name: 'gpl', version: '1.0.0', source: 'r' }, license: 'GPL-3.0-only' }], licenses: [] }));
  const out = [];
  const io = { log() {}, err: (s) => out.push(s) };
  const base = ['--app', 'Demo', '--root', root, '--rust-json', 'about.json'];
  assert.equal(main([...base, '--check'], io), 1);
  assert.match(out.join('\n'), /NON-PERMISSIVE: rust gpl@1\.0\.0: GPL-3\.0-only/);
  assert.match(out.join('\n'), /is missing/);
  assert.equal(main(base, io), 1);
});

test('cli: usage errors and tool errors exit 2 with what to do', () => {
  const errs = [];
  const io = { log() {}, err: (s) => errs.push(s) };
  assert.equal(main([], io), 2);
  assert.equal(main(['--app', 'D', '--bogus'], io), 2);
  assert.equal(main(['--app', 'D', '--root', npmApp()], io), 2);
  assert.match(errs.join('\n'), /--app <product name> or --config/);
  assert.match(errs.join('\n'), /unknown option: --bogus/);
  assert.match(errs.join('\n'), /no Rust manifest/);
  assert.throws(() => parseArgs(['--app']), /needs a value/);
});

test('cargo about is run per manifest with the about.toml, and its failures are explained', () => {
  const root = makeApp({ 'Cargo.toml': '' });
  const calls = [];
  const exec = (cmd, args) => {
    calls.push([cmd, ...args]);
    writeFileSync(args[args.indexOf('-o') + 1], JSON.stringify(aboutJson([['serde', '1.0.0', 'MIT']])));
  };
  const docs = cargoAbout({ manifests: ['/m/a/Cargo.toml', '/m/b/Cargo.toml'], aboutToml: '/r/about.toml', root, exec });
  assert.equal(docs.length, 2);
  assert.deepEqual(calls[0].slice(0, 6), ['cargo', 'about', 'generate', '-c', '/r/about.toml', '--format']);
  assert.ok(calls[1].includes('/m/b/Cargo.toml'));
  const failing = (e) => () => cargoAbout({ manifests: ['/m/Cargo.toml'], aboutToml: 'x', root, exec: () => { throw Object.assign(new Error('boom'), e); } });
  assert.throws(failing({ code: 'ENOENT' }), /cargo is not installed/);
  assert.throws(failing({ stderr: 'error: no such command: `about`' }), /cargo install cargo-about/);
  assert.throws(failing({ stderr: 'line1\nline2\nerror: bad manifest' }), /cargo about failed for \/m\/Cargo\.toml: line1 line2 error: bad manifest/);
});

test('the output folder is created and --out may repeat', () => {
  const root = npmApp();
  writeFileSync(join(root, 'about.json'), JSON.stringify(aboutJson([])));
  assert.equal(main(['--app', 'D', '--root', root, '--rust-json', 'about.json', '--out', 'one', '--out', 'two/deeper'], { log() {}, err() {} }), 0);
  assert.ok(readFileSync(join(root, 'one/THIRD_PARTY_NOTICES.md'), 'utf8').startsWith('# Third-party notices'));
  mkdirSync(join(root, 'x'), { recursive: true });
  assert.ok(readFileSync(join(root, 'two/deeper/THIRD_PARTY_NOTICES.md'), 'utf8'));
});

/** A monorepo: root lock and node_modules shared by two workspaces; only apps/x/app's production closure ships. */
function monorepo() {
  return makeApp({
    'package-lock.json': JSON.stringify({
      packages: {
        '': { workspaces: ['apps/x/app', 'ui'] },
        'apps/x/app': { dependencies: { shipped: '1' }, devDependencies: { tool: '1' } },
        ui: { dependencies: { other: '1' } },
        'node_modules/shipped': { version: '1.0.0', license: 'MIT', dependencies: { nested: '1' } },
        'node_modules/nested': { version: '2.0.0', license: 'Apache-2.0', dev: true },
        'node_modules/tool': { version: '3.0.0', license: 'GPL-3.0-only', dev: true },
        'node_modules/other': { version: '4.0.0', license: 'MIT' },
      },
    }),
    'node_modules/shipped/LICENSE': 'MIT License\n\nshipped',
    'node_modules/nested/LICENSE': 'Apache text',
    'apps/x/app/package.json': '{}',
  });
}

test('npm packages of one workspace of a root lock: its production closure only, even where the root lock marks a package dev', () => {
  const { packages, warnings } = npmPackages(join(monorepo(), 'apps/x/app'));
  assert.deepEqual(packages.map((p) => `${p.name}@${p.version}`).sort(), ['nested@2.0.0', 'shipped@1.0.0']);
  assert.deepEqual(warnings, []);
  assert.throws(() => npmPackages(join(monorepo(), 'apps/missing/app')), /no workspace apps\/missing\/app/);
});

test('cli --config: reads the app contract (name, manifests, npm folder, output) from its paddock.ci.json', () => {
  const root = monorepo();
  writeFileSync(join(root, 'apps/x/app/about.json'), JSON.stringify(aboutJson([['serde', '1.0.0', 'MIT']])));
  writeFileSync(join(root, 'apps/x/paddock.ci.json'), JSON.stringify({ notices: { name: 'Xapp', npm: 'app', out: ['app/resources'], rustJson: ['app/about.json'] } }));
  const out = [];
  const io = { log: (s) => out.push(s), err: (s) => out.push(s) };
  const base = ['--root', root, '--config', 'apps/x/paddock.ci.json'];
  assert.equal(main([...base, '--check'], io), 1, 'missing notices fail the check');
  assert.equal(main(base, io), 0);
  const text = readFileSync(join(root, 'apps/x/app/resources/THIRD_PARTY_NOTICES.md'), 'utf8');
  assert.match(text, /Xapp is dual-licensed/);
  assert.match(text, /- serde 1\.0\.0/);
  assert.match(text, /- shipped 1\.0\.0/);
  assert.doesNotMatch(text, /- other 4\.0\.0/, 'a package of another workspace is not shipped');
  assert.equal(main([...base, '--check'], io), 0);
  writeFileSync(join(root, 'apps/x/app/about.json'), JSON.stringify(aboutJson([['serde', '1.0.0', 'MIT'], ['tokio', '1.0.0', 'MIT']])));
  assert.equal(main([...base, '--check'], io), 1, 'a new dependency makes the committed notices stale');
  writeFileSync(join(root, 'apps/x/paddock.ci.json'), JSON.stringify({}));
  assert.equal(main(base, io), 2, 'a config without a notices block is a usage error');
  assert.match(out.join('\n'), /no "notices" block/);
});

test('cli: the shared about.toml is the default when the app has none', () => {
  const calls = [];
  const root = monorepo();
  writeFileSync(join(root, 'apps/x/paddock.ci.json'), JSON.stringify({ notices: { name: 'X', npm: 'app', manifests: ['app/src-tauri/Cargo.toml'], out: ['app/res'] } }));
  main(['--root', root, '--config', 'apps/x/paddock.ci.json'], { log() {}, err() {}, exec: (cmd, args) => { calls.push(args); throw Object.assign(new Error('stop'), { stderr: 'stop' }); } });
  const about = calls[0][calls[0].indexOf('-c') + 1];
  assert.equal(about, join(PADDOCK, 'templates', 'repo', 'about.toml'));
});

test('nonPermissive: the deprecated "MPL-2.0+" spelling is MPL-2.0; CC-BY-4.0 is refused for a Rust crate', () => {
  const pk = (license) => ({ kind: 'rust', name: 'x', version: '1', license, spdx: spdxIds(license) });
  assert.deepEqual(nonPermissive([pk('MPL-2.0+'), pk('CC-BY-4.0'), pk('GPL-2.0+')]), ['rust x@1: CC-BY-4.0', 'rust x@1: GPL-2.0+']);
});

test('nonPermissive evaluates the SPDX expression: AND needs every term allowed, OR at least one, parentheses and WITH respected', () => {
  const bad = (kind, license) => nonPermissive([{ kind, name: 'x', version: '1', license, spdx: spdxIds(license) }]).length === 1;
  assert.ok(bad('rust', 'MIT AND GPL-3.0-only'));
  assert.ok(bad('npm', '(MIT OR Apache-2.0) AND AGPL-3.0-only'));
  assert.ok(!bad('rust', 'MIT OR GPL-3.0-only'));
  assert.ok(!bad('rust', '(MIT OR GPL-3.0-only) AND Apache-2.0'));
  assert.ok(!bad('rust', 'MIT AND (Apache-2.0 OR GPL-3.0-only)'));
  assert.ok(bad('rust', 'GPL-3.0-only OR AGPL-3.0-only'));
  assert.ok(!bad('rust', 'Apache-2.0 WITH LLVM-exception'));
  assert.ok(!bad('rust', 'MIT AND Apache-2.0 WITH LLVM-exception'));
  assert.ok(bad('rust', 'GPL-2.0-only WITH Classpath-exception-2.0'));
  assert.ok(bad('rust', 'MIT AND'), 'an expression that does not parse is refused');
  assert.ok(bad('rust', '(MIT OR Apache-2.0'), 'unbalanced parentheses are refused');
  assert.ok(!bad('rust', 'mit or apache-2.0'.replace('mit', 'MIT').replace('apache-2.0', 'Apache-2.0')), 'operators are case-insensitive');
  assert.ok(bad('npm', 'UNKNOWN'));
});

test('nonPermissive scopes the exceptions by ecosystem: CC-BY-4.0 and OFL-1.1 are for named npm packages only', () => {
  const bad = (kind, name, license) => nonPermissive([{ kind, name, version: '1', license, spdx: spdxIds(license) }]).length === 1;
  assert.ok(bad('rust', 'caniuse-lite', 'CC-BY-4.0'));
  assert.ok(!bad('npm', 'caniuse-lite', 'CC-BY-4.0'));
  assert.ok(bad('rust', '@fontsource/inter', 'MIT AND OFL-1.1'));
  assert.ok(!bad('npm', '@fontsource/inter', 'MIT AND OFL-1.1'));
});

const pkg = (kind, name, license) => ({ kind, name, version: '1', license, spdx: spdxIds(license ?? '') });

test('nonPermissive is fail-closed: a package is accepted only when its expression evaluates true', () => {
  for (const license of ['', '  ', null, undefined, 'UNKNOWN', 'NOASSERTION', 'UNLICENSED', 'SEE LICENSE IN LICENSE.md', 'LicenseRef-custom', 'BSD', 'Apache', 'MIT/X11', 'MIT AND LicenseRef-x']) {
    for (const kind of ['rust', 'npm']) assert.equal(nonPermissive([pkg(kind, 'x', license)]).length, 1, `${kind} ${JSON.stringify(license)} is refused`);
  }
  assert.deepEqual(nonPermissive([pkg('rust', 'x', 'MIT OR Apache-2.0')]), []);
});

test('exceptions are scoped to packages: CC-BY-4.0 to caniuse-lite, OFL-1.1 to the font packages that use it', () => {
  assert.deepEqual(nonPermissive([pkg('npm', 'caniuse-lite', 'CC-BY-4.0'), pkg('npm', '@fontsource/inter', 'OFL-1.1')]), []);
  assert.equal(nonPermissive([pkg('npm', 'other-data', 'CC-BY-4.0')]).length, 1);
  assert.equal(nonPermissive([pkg('npm', 'other-font', 'OFL-1.1')]).length, 1);
  assert.equal(nonPermissive([pkg('rust', 'caniuse-lite', 'CC-BY-4.0')]).length, 1, 'not for a Rust crate of that name');
  assert.equal(nonPermissive([pkg('npm', 'caniuse-lite', 'MIT AND GPL-3.0-only')]).length, 1, 'an exception covers only its own licence');
});

test('declared npm licences: the legacy `licenses: [{type}]` form and object forms are parsed, not skipped', () => {
  const root = makeApp({
    'package-lock.json': JSON.stringify({ packages: { '': {}, 'node_modules/old': { version: '1.0.0' }, 'node_modules/obj': { version: '1.0.0' }, 'node_modules/none': { version: '1.0.0' } } }),
    'node_modules/old/package.json': JSON.stringify({ licenses: [{ type: 'MIT' }, { type: 'Apache-2.0' }] }),
    'node_modules/obj/package.json': JSON.stringify({ license: { type: 'ISC' } }),
    'node_modules/none/package.json': '{}',
  });
  const by = Object.fromEntries(npmPackages(root).packages.map((p) => [p.name, p.license]));
  assert.deepEqual(by, { old: 'MIT OR Apache-2.0', obj: 'ISC', none: 'UNKNOWN' });
});
