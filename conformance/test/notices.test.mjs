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
  assert.deepEqual(nonPermissive([pk('MIT OR LGPL-2.1-or-later'), pk('GPL-3.0-only'), pk('OFL-1.1')]), ['rust x@1: GPL-3.0-only']);
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
  assert.match(errs.join('\n'), /--app <product name> is needed/);
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
