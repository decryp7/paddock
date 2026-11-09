import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { resolveFile, serveProd, tauriCsp } from './serve-prod.ts';

function app(csp: unknown): string {
  const dir = mkdtempSync(join(tmpdir(), 'serve-prod-'));
  mkdirSync(join(dir, 'src-tauri'));
  writeFileSync(join(dir, 'src-tauri', 'tauri.conf.json'), JSON.stringify({ app: { security: { csp } } }));
  mkdirSync(join(dir, 'dist', 'app', 'browser'), { recursive: true });
  writeFileSync(join(dir, 'dist', 'app', 'browser', 'index.html'), '<p>spa</p>');
  writeFileSync(join(dir, 'dist', 'app', 'browser', 'main.js'), 'x');
  return dir;
}

test('a missing or empty CSP refuses to serve', () => {
  const dir = app('  ');
  try {
    assert.throws(() => tauriCsp(dir), /no app.security.csp/);
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test('paths: files, SPA fallback for routes, 404 for missing assets, no escape from root', () => {
  const dir = app("default-src 'self'");
  const root = join(dir, 'dist', 'app', 'browser');
  try {
    assert.equal(resolveFile(root, '/main.js?x=1'), join(root, 'main.js'));
    assert.equal(resolveFile(root, '/apps/list'), join(root, 'index.html'));
    assert.equal(resolveFile(root, '/missing.js'), null);
    assert.equal(resolveFile(root, '/../../tauri.conf.json'), null);
    assert.equal(resolveFile(root, '/%E0%A4%A'), null);
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test('the server sends the CSP and the SPA entry point', async () => {
  const dir = app("default-src 'self'");
  const server = serveProd(dir, 0);
  try {
    await new Promise((r) => server.once('listening', r));
    const res = await fetch(`http://localhost:${(server.address() as AddressInfo).port}/route`);
    assert.equal(res.status, 200);
    assert.equal(res.headers.get('content-security-policy'), "default-src 'self'");
    assert.equal(await res.text(), '<p>spa</p>');
  } finally {
    server.close();
    rmSync(dir, { recursive: true });
  }
});
