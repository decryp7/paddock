// Records tests/golden/notices/: what conformance/src/notices (the notices generator `dev notices` ports) writes and prints for two
// fixture trees, with invented data only (ER-14). Run once from the repository root and commit the result:
//   node tools/paddock-dev/tests/golden/notices/record.mjs
// `fixtures.json` (scenario -> { files, args, rustJson }) is written first, then the generator is run on a fresh temp folder per scenario:
// `expected/<scenario>.md` is the notices file it wrote, `expected/<scenario>.messages.txt` what it logged ("out: ") and warned
// ("err: "), `expected/<scenario>.exit` its exit status. The Rust test (tests/notices_golden.rs) materialises the same files and must
// reproduce all three, so it keeps working after the generator is gone (ADR 0022 R6, §3.6). Files are listed in fixtures.json
// because a checked-in tree could not hold a node_modules folder (ignored by git) or CRLF files (text=auto).
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { main } from '../../../../../conformance/src/notices/cli.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const lock = (packages) => `${JSON.stringify({ lockfileVersion: 3, packages }, null, 2)}\n`;
const pkg = (license) => `${JSON.stringify({ license })}\n`;
const MIT = (who) => `MIT License\r\n\r\nCopyright (c) ${who}  \r\n\r\nPermission is hereby granted, free of charge, to any person obtaining a copy.\r\n`;
const APACHE = 'Apache License\nVersion 2.0, January 2004\n\nInvented text for the fixture.\n';
const OFL = 'Copyright 2020 The Invented Font Authors\n\nSIL OPEN FONT LICENSE Version 1.1\n';

const about = (crates, licenses) => ({
  crates: crates.map(([name, version, license, source = 'registry+https://example.invalid/index']) => ({
    package: { name, version, source },
    ...(license === undefined ? {} : { license }),
  })),
  licenses: licenses.map(([id, text, ...used]) => ({ id, text, used_by: used.map(([name, version]) => ({ crate: { name, version } })) })),
});

const scenarios = {
  // A monorepo: the root lock and node_modules are shared; apps/x/app ships only its own production closure.
  workspace: {
    args: ['--root', '<root>', '--config', 'apps/x/paddock.ci.json'],
    files: {
      'package-lock.json': lock({
        '': { workspaces: ['apps/x/app', 'ui'] },
        'apps/x/app': {
          dependencies: { shipped: '1', '@fontsource/inter': '5', uilib: '1', ghost: '1' },
          optionalDependencies: { 'optional-thing': '1' },
          peerDependencies: { peerlib: '3', 'peer-opt': '1', devpeer: '1' },
          peerDependenciesMeta: { 'peer-opt': { optional: true } },
          devDependencies: { devpeer: '1', tooling: '1' },
        },
        ui: { dependencies: { other: '1' } },
        'node_modules/shipped': { version: '1.0.0', license: 'MIT', dependencies: { nested: '2', ghost2: '1' } },
        'node_modules/shipped/node_modules/nested': { version: '2.0.0', license: 'Apache-2.0' },
        'node_modules/nested': { version: '9.0.0', license: 'MIT', dev: true },
        'node_modules/@fontsource/inter': { version: '5.0.0', license: 'OFL-1.1' },
        'node_modules/uilib': { resolved: 'ui', link: true },
        'node_modules/optional-thing': { version: '1.2.0', license: 'ISC' },
        'node_modules/peerlib': { version: '3.0.0', license: '(MIT OR GPL-3.0-only)' },
        'node_modules/peer-opt': { version: '1.0.0', license: 'MIT' },
        'node_modules/devpeer': { version: '1.0.0', license: 'MIT' },
        'node_modules/tooling': { version: '1.0.0', license: 'GPL-3.0-only', dev: true },
        'node_modules/other': { version: '4.0.0', license: 'MIT' },
      }),
      'apps/x/app/package.json': '{"name":"x-app"}\n',
      'node_modules/shipped/LICENSE': MIT('Invented Author One'),
      'node_modules/shipped/package.json': pkg('MIT'),
      'node_modules/shipped/node_modules/nested/LICENSE-APACHE': APACHE,
      'node_modules/shipped/node_modules/nested/NOTICE': 'Nested notice\r\nline two\r\n',
      'node_modules/shipped/node_modules/nested/licensing.js': 'module.exports = 1;',
      'node_modules/@fontsource/inter/LICENSE': OFL,
      'node_modules/optional-thing/package.json': pkg('ISC'),
      'node_modules/peerlib/COPYING': 'Invented copying terms.\n',
      'node_modules/peerlib/license.md': 'Second text of peerlib.\n',
      'node_modules/peer-opt/LICENSE-MIT': MIT('Invented Author One').replace(/\r\n/g, '\n'),
      'node_modules/devpeer/LICENSE': MIT('Invented Author Two'),
      'apps/x/app/about-a.json': JSON.stringify(
        about(
          [
            ['x-app', '1.0.0', 'MIT', null],
            ['serde-like', '1.0.1', 'MIT OR Apache-2.0'],
            ['zeta-crate', '0.4.0', 'MIT'],
            ['alpha-crate', '2.0.0', 'Apache-2.0'],
          ],
          [
            ['MIT', MIT('Invented Rust Author A'), ['serde-like', '1.0.1'], ['zeta-crate', '0.4.0']],
            ['Apache-2.0', APACHE, ['serde-like', '1.0.1'], ['alpha-crate', '2.0.0'], ['not-listed', '9.9.9']],
          ],
        ),
      ),
      'apps/x/app/about-b.json': JSON.stringify(
        about(
          [
            ['serde-like', '1.0.1', 'MIT OR Apache-2.0'],
            ['beta-crate', '3.1.0', 'MIT'],
            ['notext-crate', '1.0.0', 'BSD-3-Clause'],
          ],
          [['MIT', MIT('Invented Rust Author B'), ['beta-crate', '3.1.0']]],
        ),
      ),
      'apps/x/paddock.ci.json': JSON.stringify({
        notices: { name: 'Fixture X', npm: 'app', out: ['app/resources', 'app/second'], rustJson: ['app/about-a.json', 'app/about-b.json'] },
      }),
    },
  },
  // A standalone app folder with its own lock, and licences that are refused or odd.
  flat: {
    args: ['--app', 'Flat Demo', '--root', '<root>', '--npm', 'app', '--rust-json', 'about.json', '--out', 'out'],
    files: {
      'app/package-lock.json': lock({
        '': {},
        'node_modules/a': { version: '1.0.0', license: 'MIT' },
        'node_modules/x/node_modules/a': { version: '1.0.0', license: 'MIT' },
        'node_modules/legacy': { version: '0.1.0', licenses: [{ type: 'MIT' }, { type: 'BSD-3-Clause' }] },
        'node_modules/gplthing': { version: '2.0.0', license: 'GPL-3.0-only' },
        'node_modules/unlicensed': { version: '0.0.1' },
        'node_modules/seelic': { version: '1.1.0', license: 'SEE LICENSE IN LICENSE.md' },
        'node_modules/caniuse-lite': { version: '1.0.1', license: 'CC-BY-4.0' },
        'node_modules/llvmish': { version: '1.0.0', license: 'Apache-2.0 WITH LLVM-exception' },
        'node_modules/plus': { version: '1.0.0', license: 'MIT+' },
        'node_modules/nested-expr': { version: '1.0.0', license: '(MIT AND (ISC OR GPL-2.0-only))' },
        'node_modules/mixed': { version: '1.0.0', license: 'MIT AND GPL-2.0-only' },
        'node_modules/lower': { version: '1.0.0', license: 'mit or isc' },
        'node_modules/badjson': { version: '1.0.0', license: 'ISC' },
        'node_modules/dev-thing': { version: '5.0.0', license: 'GPL-3.0-only', dev: true },
        'node_modules/dev-opt': { version: '5.0.0', license: 'GPL-3.0-only', devOptional: true },
      }),
      'app/node_modules/a/LICENSE': MIT('Invented Author A'),
      'app/node_modules/legacy/LICENSE.md': 'Legacy licence text\n   \n\n',
      'app/node_modules/gplthing/LICENSE': 'Invented GPL stand-in text\n',
      'app/node_modules/unlicensed/package.json': '{}',
      'app/node_modules/badjson/package.json': '{ not json',
      'app/node_modules/badjson/LICENSE': MIT('Invented Author A'),
      'app/node_modules/mixed/COPYING': 'Mixed text\n',
      'about.json': JSON.stringify(
        about(
          [
            ['flat-app', '1.0.0', 'MIT', null],
            ['empty-lic', '1.0.0', ''],
            ['no-lic-field', '1.0.0'],
            ['lgpl-or-mit', '2.0.0', 'MIT OR LGPL-2.1-or-later'],
            ['unicode-crate', '1.0.0', 'Unicode-3.0'],
            ['astral-crate', '1.0.0', 'ISC'],
            ['bmp-crate', '1.0.0', 'ISC'],
          ],
          [
            ['Unicode-3.0', 'UNICODE LICENSE © 2020 — invented\r\n', ['unicode-crate', '1.0.0']],
            ['ISC', 'ISC text 𝒜 astral\n', ['astral-crate', '1.0.0']],
            ['ISC', 'ISC text ～ bmp\n', ['bmp-crate', '1.0.0']],
            ['MIT', MIT('Invented Author A'), ['lgpl-or-mit', '2.0.0']],
          ],
        ),
      ),
    },
  },
};

const fixtures = {};
for (const [name, s] of Object.entries(scenarios)) fixtures[name] = { args: s.args, files: s.files };
mkdirSync(join(here, 'expected'), { recursive: true });
writeFileSync(join(here, 'fixtures.json'), `${JSON.stringify(fixtures, null, 1)}\n`);

for (const [name, s] of Object.entries(fixtures)) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-notices-golden-')));
  try {
    for (const [p, content] of Object.entries(s.files)) {
      mkdirSync(dirname(join(root, p)), { recursive: true });
      writeFileSync(join(root, p), content);
    }
    const lines = [];
    const note = (prefix) => (m) => lines.push(`${prefix}${String(m).split(root).join('<root>')}`);
    const exit = main(s.args.map((a) => (a === '<root>' ? root : a)), { log: note('out: '), err: note('err: ') });
    const outDir = name === 'workspace' ? 'apps/x/app/resources' : 'out';
    writeFileSync(join(here, 'expected', `${name}.md`), readFileSync(join(root, outDir, 'THIRD_PARTY_NOTICES.md')));
    writeFileSync(join(here, 'expected', `${name}.messages.txt`), `${lines.join('\n')}\n`);
    writeFileSync(join(here, 'expected', `${name}.exit`), `${exit}\n`);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
