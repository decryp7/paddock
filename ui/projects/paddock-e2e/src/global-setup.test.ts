import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { pathToFileURL } from 'node:url';

// Scratch folders live under ui/.angular (git-ignored), so `@playwright/test` and the sources resolve from them.
const ui = join(import.meta.dirname, '..', '..', '..');
const src = import.meta.dirname;
const work = () => {
  mkdirSync(join(ui, '.angular'), { recursive: true });
  return mkdtempSync(join(ui, '.angular', 'ready-'));
};
const node = (code: string) => `${JSON.stringify(process.execPath)} -e ${JSON.stringify(code)}`;

function playwright(dir: string, serve: string, port: number) {
  mkdirSync(join(dir, 'e2e'));
  writeFileSync(
    join(dir, 'playwright.config.ts'),
    `import { paddockConfig } from ${JSON.stringify(join(src, 'config.ts'))};\n` +
      `export default paddockConfig({ app: 'ready', defaultPort: ${port}, webkit: false, launcher: 'off', testDir: './e2e', serveTimeoutMs: 20000, serve: () => ${JSON.stringify(serve)}, env: { PW_PORT: '${port}' } });\n`,
  );
  writeFileSync(
    join(dir, 'e2e', 'page.spec.ts'),
    `import { test, expect } from '@playwright/test';\ntest('the page loads', async ({ request }) => { expect(await (await request.get('/')).text()).toBe('served by the real global setup'); });\n`,
  );
  return spawnSync(process.execPath, [join(ui, '..', 'node_modules', '@playwright', 'test', 'cli.js'), 'test', '-c', join(dir, 'playwright.config.ts'), '--project=chromium'], {
    cwd: dir,
    encoding: 'utf8',
    env: { ...process.env, PW_SKIP_BUILD_GUARD: '1', PW_REUSE_SERVER: '', CI: '' },
    timeout: 120_000,
  });
}

test('a real Playwright run starts its server through the global setup, runs the spec against it and ends the server', () => {
  const dir = work();
  const port = 34100 + (process.pid % 400);
  const r = playwright(dir, node(`require('http').createServer((q,s)=>s.end('served by the real global setup')).listen(${port}, () => console.log('up'))`), port);
  rmSync(dir, { recursive: true, force: true });
  assert.equal(r.status, 0, `${r.stdout}\n${r.stderr}`);
  assert.match(r.stdout, /\[e2e server out\] up/);
  assert.match(r.stdout, /ready after/);
});

test('positive control: a real Playwright run whose server cannot start fails once with the ::error:: line and runs no test', () => {
  const dir = work();
  const port = 34100 + ((process.pid + 7) % 400);
  const r = playwright(dir, node(`console.error('EADDRINUSE boom'); process.exit(3)`), port);
  rmSync(dir, { recursive: true, force: true });
  assert.notEqual(r.status, 0);
  const lines = `${r.stdout}\n${r.stderr}`.split('\n').filter((l) => l.startsWith('::error::'));
  assert.ok(lines.length >= 1, `${r.stdout}\n${r.stderr}`);
  assert.match(lines[0]!, new RegExp(`http://localhost:${port}.*exit status 3.*EADDRINUSE boom`));
  assert.doesNotMatch(r.stdout, /the page loads/);
});

test('the built package: its global setup file exists next to its config (the build emits only .js)', async () => {
  const dir = work();
  const out = join(dir, 'dist');
  const tsc = spawnSync(process.execPath, [join(ui, '..', 'node_modules', 'typescript', 'bin', 'tsc'), '-p', join(src, '..', 'tsconfig.json'), '--outDir', out, '--declaration', 'false'], { cwd: ui, encoding: 'utf8' });
  assert.equal(tsc.status, 0, tsc.stdout + tsc.stderr);
  writeFileSync(join(out, 'package.json'), '{"type":"module"}\n');
  assert.ok(!existsSync(join(out, 'global-setup.ts')), 'the build emits no .ts');
  const built = (await import(pathToFileURL(join(out, 'config.js')).href)) as typeof import('./config.ts');
  const { globalSetup } = built.paddockServers([]);
  assert.equal(globalSetup, join(out, 'global-setup.js'));
  assert.ok(existsSync(globalSetup as string), `${globalSetup} exists`);
  rmSync(dir, { recursive: true, force: true });
});
