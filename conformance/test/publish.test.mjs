// Ported from the apps' publish tests; run: node --test test/
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { filesUnder, isPrerelease, parseArgs, pickToken, publish } from '../release/publish.mjs';

const args = ['--api', 'https://h/api/v1/', '--repo', 'o/r', '--tag', 'v1.2.0', '--notes', 'n.md', '--dir', 'a'];

test('arguments are parsed and checked', () => {
  assert.deepEqual(parseArgs(args), { api: 'https://h/api/v1', repo: 'o/r', tag: 'v1.2.0', notes: 'n.md', dir: 'a' });
  assert.throws(() => parseArgs(args.slice(0, 8)), /--dir is required/);
  assert.throws(() => parseArgs([...args.slice(0, 4), '--tag', 'latest', ...args.slice(6)]), /vX.Y.Z/);
  // The monorepo's tags (ADR 0014 §4): an app's version tag and its moving latest release.
  assert.equal(parseArgs([...args.slice(0, 4), '--tag', 'debrief-v1.2.0', ...args.slice(6)]).tag, 'debrief-v1.2.0');
  assert.equal(parseArgs([...args.slice(0, 4), '--tag', 'debrief-latest', ...args.slice(6), '--replace']).replace, true);
  assert.throws(() => parseArgs([...args.slice(0, 4), ...args.slice(6)]), /--tag is required/);
  // No update bridge (ADR 0014, amendment 2026-10-25): the old repositories' newest release is never a target.
  assert.throws(() => parseArgs([...args.slice(0, 4), '--newest', ...args.slice(6)]), /unknown/);
  assert.throws(() => parseArgs(['--bogus', 'x']), /unknown/);
});

function fixture() {
  const dir = mkdtempSync(join(tmpdir(), 'publish-'));
  mkdirSync(join(dir, 'a', 'nsis'), { recursive: true });
  writeFileSync(join(dir, 'a', 'latest.json'), '{}');
  writeFileSync(join(dir, 'a', 'nsis', 'App_1.2.0_x64-setup.exe'), 'exe');
  writeFileSync(join(dir, 'n.md'), 'notes');
  return dir;
}

test('files are found recursively in a stable order', () => {
  const dir = fixture();
  assert.deepEqual(filesUnder(join(dir, 'a')).map((f) => f.slice(dir.length)), ['/a/latest.json', '/a/nsis/App_1.2.0_x64-setup.exe']);
});

function fakeApi(existing, drafts = []) {
  const calls = [];
  const fetchImpl = async (url, init) => {
    calls.push(`${init.method} ${url}`);
    const json = (status, body) => ({ status, ok: status < 300, json: async () => body, text: async () => JSON.stringify(body) });
    if (init.method === 'GET' && url.includes('/releases?draft=true')) return json(200, drafts);
    if (init.method === 'GET') return existing ? json(200, existing) : json(404, {});
    if (init.method === 'PATCH') return json(200, { id: 7, html_url: 'https://h/o/r/releases/tag/v1.2.0', draft: false });
    if (url.endsWith('/releases')) return json(201, { id: 7, html_url: 'https://h/o/r/releases/tag/v1.2.0', draft: true, assets: [] });
    return json(201, {});
  };
  return { calls, fetchImpl };
}

test('a new release is created and every file uploaded', async () => {
  const dir = fixture();
  const { calls, fetchImpl } = fakeApi(null);
  const url = await publish({ api: 'https://h/api/v1', repo: 'o/r', tag: 'v1.2.0', notes: join(dir, 'n.md'), dir: join(dir, 'a') }, 't', fetchImpl, () => {});
  assert.equal(url, 'https://h/o/r/releases/tag/v1.2.0');
  assert.deepEqual(calls, [
    'GET https://h/api/v1/repos/o/r/releases/tags/v1.2.0',
    'GET https://h/api/v1/repos/o/r/releases?draft=true&limit=50',
    'POST https://h/api/v1/repos/o/r/releases',
    'POST https://h/api/v1/repos/o/r/releases/7/assets?name=latest.json',
    'POST https://h/api/v1/repos/o/r/releases/7/assets?name=App_1.2.0_x64-setup.exe',
    // Published (un-drafted) only after the last upload.
    'PATCH https://h/api/v1/repos/o/r/releases/7',
  ]);
});

test('a release is created as a draft; a failed upload leaves it a draft, and a re-run finds the draft, skips its files and publishes it', async () => {
  const dir = fixture();
  const created = [];
  const base = fakeApi(null);
  const failing = async (url, init) => {
    if (init.method === 'POST' && url.endsWith('/releases')) created.push(JSON.parse(init.body));
    if (init.method === 'POST' && url.includes('name=App_1.2.0')) return { status: 500, ok: false, text: async () => 'boom' };
    return base.fetchImpl(url, init);
  };
  const opts = { api: 'https://h/api/v1', repo: 'o/r', tag: 'v1.2.0', notes: join(dir, 'n.md'), dir: join(dir, 'a') };
  await assert.rejects(publish(opts, 't', failing, () => {}), /HTTP 500/);
  assert.equal(created[0].draft, true);
  assert.ok(!base.calls.some((c) => c.startsWith('PATCH')), 'never published after a failed upload');
  const draft = fakeApi(null, [{ id: 7, tag_name: 'v1.2.0', draft: true, html_url: 'u', assets: [{ name: 'latest.json' }] }]);
  await publish(opts, 't', draft.fetchImpl, () => {});
  assert.deepEqual(draft.calls.slice(2), ['POST https://h/api/v1/repos/o/r/releases/7/assets?name=App_1.2.0_x64-setup.exe', 'PATCH https://h/api/v1/repos/o/r/releases/7']);
});

test('a re-run reuses the release and skips files already attached', async () => {
  const dir = fixture();
  const { calls, fetchImpl } = fakeApi({ id: 9, html_url: 'u', assets: [{ name: 'latest.json' }] });
  await publish({ api: 'https://h/api/v1', repo: 'o/r', tag: 'v1.2.0', notes: join(dir, 'n.md'), dir: join(dir, 'a') }, 't', fetchImpl, () => {});
  assert.deepEqual(calls.slice(1), ['POST https://h/api/v1/repos/o/r/releases/9/assets?name=App_1.2.0_x64-setup.exe']);
});

test('an API error names the call, not the token', async () => {
  const dir = fixture();
  const fetchImpl = async (url, init) =>
    init.method === 'GET' ? { status: 404, ok: false, json: async () => ({}) } : { status: 403, ok: false, text: async () => 'forbidden' };
  await assert.rejects(
    publish({ api: 'https://h/api/v1', repo: 'o/r', tag: 'v1.2.0', notes: join(dir, 'n.md'), dir: join(dir, 'a') }, 'secret-token', fetchImpl, () => {}),
    (e) => /POST https:\/\/h\/api\/v1\/repos\/o\/r\/releases: HTTP 403 forbidden/.test(e.message) && !e.message.includes('secret-token'),
  );
});

test('a pre-release is a semver suffix, never the app prefix of a monorepo tag', () => {
  assert.deepEqual(['v1.2.0', 'debrief-v1.2.0', 'debrief-latest', 'v1.2.0-rc.1', 'debrief-v1.2.0-rc.1'].map(isPrerelease), [false, false, false, true, true]);
});

test('--replace deletes and uploads again a file the release has (the moving <app>-latest release)', async () => {
  const dir = fixture();
  const { calls, fetchImpl } = fakeApi({ id: 9, html_url: 'u', assets: [{ name: 'latest.json', id: 41 }] });
  await publish({ api: 'https://h/api/v1', repo: 'o/r', tag: 'debrief-latest', notes: join(dir, 'n.md'), dir: join(dir, 'a'), replace: true }, 't', fetchImpl, () => {});
  assert.deepEqual(calls.slice(1), [
    'DELETE https://h/api/v1/repos/o/r/releases/9/assets/41',
    'POST https://h/api/v1/repos/o/r/releases/9/assets?name=latest.json',
    'POST https://h/api/v1/repos/o/r/releases/9/assets?name=App_1.2.0_x64-setup.exe',
  ]);
});


test('the token comes from RELEASE_TOKEN, else from the fallback GITEA_TOKEN; blank counts as unset', () => {
  assert.deepEqual(pickToken({ RELEASE_TOKEN: 'a', GITEA_TOKEN: 'b' }), { name: 'RELEASE_TOKEN', token: 'a' });
  assert.deepEqual(pickToken({ RELEASE_TOKEN: '', GITEA_TOKEN: 'b' }), { name: 'GITEA_TOKEN', token: 'b' });
  assert.deepEqual(pickToken({ RELEASE_TOKEN: '  ', GITEA_TOKEN: 'b' }), { name: 'GITEA_TOKEN', token: 'b' });
  assert.deepEqual(pickToken({ GITEA_TOKEN: 'b' }), { name: 'GITEA_TOKEN', token: 'b' });
  assert.equal(pickToken({ RELEASE_TOKEN: '' }), null);
  assert.equal(pickToken({}), null);
});
