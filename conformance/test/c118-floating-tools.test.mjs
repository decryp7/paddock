import assert from 'node:assert/strict';
import { test } from 'node:test';
import { floatingOn, floatingTools } from '../src/checks/C118-floating-tools.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const kinds = (line) => floatingOn(line).map((f) => f.kind);

test('C118 bad: moving Rust channels in every spelling', () => {
  assert.deepEqual(kinds('        toolchain: stable'), ['toolchain']);
  assert.deepEqual(kinds('          toolchain: "nightly"'), ['toolchain']);
  assert.deepEqual(kinds('      - uses: dtolnay/rust-toolchain@stable'), ['toolchain']);
  assert.deepEqual(kinds('rustup toolchain install stable --profile minimal'), ['rustup']);
  assert.deepEqual(kinds('rustup default beta'), ['rustup']);
  assert.deepEqual(kinds('rustup update'), ['rustup']);
  assert.deepEqual(kinds('rustup update && cargo build'), ['rustup']);
});

test('C118 good: pinned Rust toolchains', () => {
  for (const l of ['toolchain: 1.99.1', 'toolchain: nightly-2026-11-01', 'rustup toolchain install 1.99.1 --profile minimal', 'rustup toolchain install nightly-2026-11-01', 'uses: dtolnay/rust-toolchain@1.99.1', 'rustup target add x86_64-pc-windows-msvc', '# toolchain: stable']) {
    assert.deepEqual(kinds(l), [], l);
  }
});

test('C118 bad: @latest, unversioned npx, node release lines', () => {
  assert.deepEqual(kinds('run: npm i -g pnpm@latest'), ['latest']);
  assert.deepEqual(kinds('run: npx tsx scripts/a.ts'), ['npx']);
  assert.deepEqual(kinds('run: npx -y serve'), ['npx']);
  for (const v of ['lts/*', 'lts/iron', 'latest', 'node', 'current']) assert.deepEqual(kinds(`node-version: ${v}`), ['node'], v);
});

test('C118 good: versioned npx, lockfile node, no-install', () => {
  for (const l of ['run: npx tsx@4.19.2 scripts/a.ts', 'run: npx -y @scope/pkg@1.2.3', 'run: npx --no-install playwright test', 'node-version-file: .nvmrc', 'node-version: 22.11.0']) {
    assert.deepEqual(kinds(l), [], l);
  }
});

test('C118 bad: cargo install without a pin or without --locked', () => {
  assert.deepEqual(kinds('cargo install cargo-nextest'), ['cargo', 'cargo']);
  assert.deepEqual(kinds('cargo install cargo-nextest --locked'), ['cargo']);
  assert.deepEqual(kinds('cargo install cargo-nextest --version 0.9.131'), ['cargo']);
});

test('C118 good: cargo install pinned and locked, or from a path', () => {
  assert.deepEqual(kinds('cargo install cargo-nextest --version 0.9.131 --locked'), []);
  assert.deepEqual(kinds('cargo install --locked cargo-nextest@0.9.131'), []);
  assert.deepEqual(kinds('cargo install --path tools/paddock-dev'), []);
});

test('C118 bad: a piped installer with neither version nor checksum', () => {
  assert.deepEqual(kinds('curl https://sh.rustup.rs -sSf | sh'), ['curl']);
  assert.deepEqual(kinds('curl -fsSL https://x.example/install.sh | sudo bash'), ['curl']);
  assert.deepEqual(kinds('iwr https://x.example/i.ps1 | iex'), ['curl']);
});

test('C118 good: a versioned or checksummed download', () => {
  assert.deepEqual(kinds('curl -fsSL https://x.example/v1.2.3/install.sh | sh'), []);
  assert.deepEqual(kinds('curl -fsSL https://x.example/i.sh | sha256sum -c'), []);
  assert.deepEqual(kinds('curl -fsSLo a.tgz https://x.example/a-1.2.3.tgz'), []);
});

test('C118 waiver needs a reason, on the line or the line above', () => {
  assert.equal(floatingTools('# conformance: floating-tool — probe runs the newest on purpose\nrun: npm i -g x@latest\n')[0].waived, 'probe runs the newest on purpose');
  assert.equal(floatingTools('run: npm i -g x@latest # conformance: floating-tool — canary of the newest\n')[0].waived, 'canary of the newest');
  assert.equal(floatingTools('# conformance: floating-tool\nrun: npm i -g x@latest\n')[0].waived, 'no-reason');
  assert.equal(floatingTools('run: npm i -g x@latest\n')[0].waived, undefined);
});

test('C118 through the runner: warns in workflows and actions, skips waived and other folders', async () => {
  const root = makeApp({
    '.gitea/workflows/a.yml': 'steps:\n  - run: rustup toolchain install stable\n  - run: npm i -g y@latest # conformance: floating-tool — canary\n',
    '.gitea/actions/b/action.yml': 'runs:\n  steps:\n    - run: cargo install cargo-nextest\n',
    '.gitea/actions/b/run.sh': '#!/bin/sh\nnpx tsx a.ts\n',
    'docs/x.yml': 'toolchain: stable\n',
  });
  const f = await findingsOf('C118', root);
  assert.deepEqual(f.map((x) => [x.file, x.line, x.severity]), [
    ['.gitea/actions/b/action.yml', 3, 'warn'], ['.gitea/actions/b/action.yml', 3, 'warn'],
    ['.gitea/actions/b/run.sh', 2, 'warn'], ['.gitea/workflows/a.yml', 2, 'warn'],
  ]);
  assert.match(f[3].fix, /exact version/);
});

test('C118 passes a tree with only pinned tools', async () => {
  const root = makeApp({ '.gitea/workflows/a.yml': 'steps:\n  - run: rustup toolchain install 1.99.1\n' });
  assert.deepEqual(await findingsOf('C118', root), []);
});
