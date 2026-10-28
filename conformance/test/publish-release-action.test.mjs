// A dry run of the publish-release action's script against a fake Gitea API (no network, no token): an app's tag in the
// monorepo (`pitcrew-v1.0.3`) publishes the release, writes latest.json with the version after the prefix, and puts latest.json
// on the moving `pitcrew-latest` release, replacing the previous one. This is the release Pitcrew's own workflow used to do.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const ACTION = join(PADDOCK, '.gitea', 'actions', 'publish-release');

/** The action's one `run: |` step with the expressions filled in, as the runner would. */
function script(over = {}) {
  const text = readFileSync(join(ACTION, 'action.yml'), 'utf8');
  const body = text.slice(text.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n');
  const values = { 'github.action_path': ACTION, 'github.ref_name': 'pitcrew-v1.0.3', 'github.ref': 'refs/tags/pitcrew-v1.0.3', 'github.server_url': 'http://gitea.invalid', 'github.repository': 'decryp7/paddock', ...over };
  return body.replace(/\$\{\{ ([\w.]+) \}\}/g, (_, k) => values[k] ?? assert.fail(`unexpected expression ${k}`));
}

/** A fake Gitea: releases by tag, assets by release; records every call. */
async function fakeGitea(seed = {}) {
  const releases = new Map(Object.entries(seed).map(([tag, assets], i) => [tag, { id: i + 1, tag_name: tag, html_url: `https://x/${tag}`, assets: assets.map((name, j) => ({ id: 100 * (i + 1) + j, name })) }]));
  const calls = [];
  const bodies = new Map();
  const server = createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      calls.push(`${req.method} ${url.pathname}${url.search}`);
      const send = (code, json) => {
        res.writeHead(code, { 'Content-Type': 'application/json' });
        res.end(json === undefined ? '' : JSON.stringify(json));
      };
      let m;
      if ((m = /\/releases\/tags\/([^/]+)$/.exec(url.pathname)) && req.method === 'GET') {
        const r = releases.get(decodeURIComponent(m[1]));
        return r ? send(200, r) : send(404, {});
      }
      if (url.pathname.endsWith('/releases') && req.method === 'POST') {
        const input = JSON.parse(Buffer.concat(chunks).toString());
        const r = { id: releases.size + 1, tag_name: input.tag_name, html_url: `https://x/${input.tag_name}`, prerelease: input.prerelease, assets: [] };
        releases.set(input.tag_name, r);
        return send(201, r);
      }
      if ((m = /\/releases\/(\d+)\/assets\/(\d+)$/.exec(url.pathname)) && req.method === 'DELETE') {
        const r = [...releases.values()].find((x) => x.id === Number(m[1]));
        r.assets = r.assets.filter((a) => a.id !== Number(m[2]));
        return send(204);
      }
      if ((m = /\/releases\/(\d+)\/assets$/.exec(url.pathname)) && req.method === 'POST') {
        const r = [...releases.values()].find((x) => x.id === Number(m[1]));
        const name = url.searchParams.get('name');
        r.assets.push({ id: 1000 + r.assets.length, name });
        bodies.set(`${r.tag_name}/${name}`, Buffer.concat(chunks).toString('latin1'));
        return send(201, { name });
      }
      return send(500, { unexpected: `${req.method} ${url.pathname}` });
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { url: `http://127.0.0.1:${server.address().port}`, releases, calls, bodies, close: () => new Promise((resolve) => server.close(resolve)) };
}

function artifacts(dir) {
  mkdirSync(dir, { recursive: true });
  for (const [name, content] of [
    ['Pitcrew_1.0.3_x64-setup.exe', 'exe'],
    ['Pitcrew_1.0.3_x64-setup.exe.sig', 'sig-win'],
    ['Pitcrew_aarch64.app.tar.gz', 'tgz'],
    ['Pitcrew_aarch64.app.tar.gz.sig', 'sig-mac'],
    ['Pitcrew_1.0.3_amd64.deb', 'deb'],
    ['Pitcrew_1.0.3_amd64.deb.sig', 'sig-deb'],
    ['Pitcrew_1.0.3_amd64.AppImage', 'appimage'],
    ['Pitcrew_1.0.3_amd64.AppImage.sig', 'sig-appimage'],
    ['Pitcrew_1.0.3_aarch64.dmg', 'dmg'],
    ['CHANGELOG-latest.md', '- one fix\n'],
  ]) {
    writeFileSync(join(dir, name), content);
  }
}

function run(api, dir, over = {}, env = {}) {
  const child = spawn('bash', ['-e', '-o', 'pipefail', '-c', script(over.expr)], {
    env: {
      PATH: process.env.PATH,
      RUNNER_TEMP: dir,
      GITEA_TOKEN: 'secret-token',
      APP_NAME: 'Pitcrew',
      CHANGELOG: join(dir, 'CHANGELOG-latest.md'),
      DIR: dir,
      BASE: '',
      PREFIX: 'pitcrew-',
      LATEST_TAG: 'pitcrew-latest',
      EXTRA: '**Existing installs:** download this version once by hand.',
      ...env,
    },
  });
  let out = '';
  child.stdout.on('data', (c) => (out += c));
  child.stderr.on('data', (c) => (out += c));
  return new Promise((resolve) => child.on('close', (status) => resolve({ status, out })));
}

test('publish-release: a pitcrew-v tag publishes the release and puts latest.json on pitcrew-latest, replacing the old one', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'publish-release-'));
  const gitea = await fakeGitea({ 'pitcrew-latest': ['latest.json'] });
  try {
    artifacts(dir);
    // The action talks to the API at github.server_url + /api/v1.
    const r = await run(gitea.url, dir, { expr: { 'github.server_url': gitea.url } });
    assert.equal(r.status, 0, r.out);
    assert.doesNotMatch(r.out, /secret-token/, 'the token never reaches the log');

    const notes = readFileSync(join(dir, 'RELEASE_NOTES.md'), 'utf8');
    assert.match(notes, /^- one fix\n\n### First launch\n\nPitcrew is not code-signed yet/);
    assert.match(notes, /\n\n\*\*Existing installs:\*\* download this version once by hand\.\n$/);

    // latest.json: the version after the prefix, https URLs of the tagged release, every signature, no .sig left as an asset.
    const manifest = JSON.parse(readFileSync(join(dir, 'latest.json'), 'utf8'));
    assert.equal(manifest.version, '1.0.3');
    assert.deepEqual(Object.keys(manifest.platforms).sort(), ['darwin-aarch64', 'darwin-aarch64-app', 'linux-x86_64', 'linux-x86_64-appimage', 'linux-x86_64-deb', 'windows-x86_64', 'windows-x86_64-nsis']);
    assert.equal(manifest.platforms['windows-x86_64-nsis'].url, 'https://dev.decryptology.net/decryp7/paddock/releases/download/pitcrew-v1.0.3/Pitcrew_1.0.3_x64-setup.exe');
    assert.equal(manifest.platforms['windows-x86_64-nsis'].signature, 'sig-win');

    const tagged = gitea.releases.get('pitcrew-v1.0.3');
    assert.equal(tagged.prerelease, false, 'the app prefix does not make a pre-release');
    assert.deepEqual(tagged.assets.map((a) => a.name).sort(), ['CHANGELOG-latest.md', 'Pitcrew_1.0.3_aarch64.dmg', 'Pitcrew_1.0.3_amd64.AppImage', 'Pitcrew_1.0.3_amd64.deb', 'Pitcrew_1.0.3_x64-setup.exe', 'Pitcrew_aarch64.app.tar.gz', 'RELEASE_NOTES.md', 'latest.json']);
    // The moving release got the new latest.json in place of the old: delete, then upload.
    const moving = gitea.calls.filter((c) => /releases\/\d+\/assets/.test(c) && /\/releases\/1\//.test(c));
    assert.deepEqual(moving.map((c) => c.split(' ')[0]), ['DELETE', 'POST']);
    assert.deepEqual(gitea.releases.get('pitcrew-latest').assets.map((a) => a.name), ['latest.json']);
    assert.equal(gitea.bodies.get('pitcrew-latest/latest.json').includes('"version": "1.0.3"'), true);
  } finally {
    await gitea.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test('publish-release: without signatures a tag fails with a clear message and publishes nothing; another tag is refused (rule 9)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'publish-release-'));
  const gitea = await fakeGitea();
  try {
    artifacts(dir);
    for (const f of ['Pitcrew_1.0.3_x64-setup.exe.sig', 'Pitcrew_aarch64.app.tar.gz.sig', 'Pitcrew_1.0.3_amd64.deb.sig', 'Pitcrew_1.0.3_amd64.AppImage.sig']) rmSync(join(dir, f));
    const unsigned = await run(gitea.url, dir, { expr: { 'github.server_url': gitea.url } });
    assert.notEqual(unsigned.status, 0, unsigned.out);
    assert.match(unsigned.out, /::error::No signed updater artifacts on tag pitcrew-v1\.0\.3: the signing key .* nothing is released/);
    assert.deepEqual([...gitea.releases.keys()], [], 'a tag without a signing key releases nothing');

    const other = await run(gitea.url, dir, { expr: { 'github.server_url': gitea.url, 'github.ref': 'refs/tags/v1.0.3', 'github.ref_name': 'v1.0.3' } });
    assert.notEqual(other.status, 0);
    assert.match(other.out, /releases are published from a pitcrew-v\* tag only \(rule 9\)/);
    const branch = await run(gitea.url, dir, { expr: { 'github.server_url': gitea.url, 'github.ref': 'refs/heads/main', 'github.ref_name': 'main' } });
    assert.notEqual(branch.status, 0);
  } finally {
    await gitea.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test('publish-release: a pre-release tag is published but never replaces pitcrew-latest/latest.json', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'publish-release-'));
  const gitea = await fakeGitea({ 'pitcrew-latest': ['latest.json'] });
  try {
    artifacts(dir);
    const r = await run(gitea.url, dir, { expr: { 'github.server_url': gitea.url, 'github.ref': 'refs/tags/pitcrew-v1.1.0-rc.1', 'github.ref_name': 'pitcrew-v1.1.0-rc.1' } });
    assert.equal(r.status, 0, r.out);
    assert.match(r.out, /is a pre-release: pitcrew-latest keeps its latest\.json/);
    assert.equal(gitea.releases.get('pitcrew-v1.1.0-rc.1').prerelease, true);
    assert.deepEqual(gitea.releases.get('pitcrew-latest').assets.map((a) => a.name), ['latest.json']);
    assert.ok(!gitea.calls.some((c) => c.startsWith('DELETE')), 'the moving release was not touched');
  } finally {
    await gitea.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

// The plan action (shared by every app's release workflow): a main push whose commit carries the app's tag is skipped.
test('plan action: the tag pattern is the app\'s, and a tagged main commit is skipped; an untagged one runs', async () => {
  const { chmodSync } = await import('node:fs');
  const { spawnSync } = await import('node:child_process');
  const text = readFileSync(join(PADDOCK, '.gitea', 'actions', 'plan', 'action.yml'), 'utf8');
  const body = text.slice(text.lastIndexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n')
    .replace(/\$\{\{ github\.action_path \}\}/g, join(PADDOCK, '.gitea', 'actions', 'plan'));
  const dir = mkdtempSync(join(tmpdir(), 'plan-action-'));
  try {
    mkdirSync(join(dir, 'bin'));
    writeFileSync(join(dir, 'bin', 'git'), '#!/bin/sh\necho "$@" >> "$GIT_LOG"\nprintf "%s\\n" "$TAGGED"\n');
    chmodSync(join(dir, 'bin', 'git'), 0o755);
    const run = (env) => {
      const out = join(dir, 'out');
      writeFileSync(out, '');
      const r = spawnSync('bash', ['-e', '-o', 'pipefail', '-c', body], {
        env: { PATH: `${join(dir, 'bin')}:${dirname(process.execPath)}:/usr/bin:/bin`, EVENT: 'push', REF: 'refs/heads/main', SHA: 'abc123', MAIN_REF: 'refs/heads/main', GREEN: '', APP: '', TAG_PATTERN: 'pitcrew-v*', GIT_LOG: join(dir, 'git.log'), GITHUB_OUTPUT: out, ...env },
        encoding: 'utf8',
      });
      assert.equal(r.status, 0, r.stdout + r.stderr);
      return readFileSync(out, 'utf8').trim();
    };
    assert.equal(run({ TAGGED: 'abc123\trefs/tags/pitcrew-v1.0.3' }), 'run=false');
    assert.match(readFileSync(join(dir, 'git.log'), 'utf8'), /ls-remote --tags origin refs\/tags\/pitcrew-v\*/, 'only the app\'s tags are asked for');
    assert.equal(run({ TAGGED: 'def456\trefs/tags/pitcrew-v1.0.2' }), 'run=true');
    assert.equal(run({ TAGGED: 'abc123\trefs/tags/pitcrew-v1.0.3', REF: 'refs/heads/feature' }), 'run=true', 'only a push to main is covered by the tag run');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('publish-release: RELEASE_TOKEN wins, the job token is the fallback, and the log names which one without printing it', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'publish-release-'));
  const gitea = await fakeGitea();
  try {
    const expr = { 'github.server_url': gitea.url };
    artifacts(dir); // each run deletes the .sig files it folds into latest.json
    const both = await run(gitea.url, dir, { expr }, { RELEASE_TOKEN: 'release-secret', GITEA_TOKEN: 'job-secret' });
    assert.equal(both.status, 0, both.out);
    assert.match(both.out, /publish: using RELEASE_TOKEN\n/);
    // An empty RELEASE_TOKEN (the secret is not set) falls back to the job token.
    artifacts(dir);
    const fallback = await run(gitea.url, dir, { expr }, { RELEASE_TOKEN: '', GITEA_TOKEN: 'job-secret' });
    assert.equal(fallback.status, 0, fallback.out);
    assert.match(fallback.out, /publish: using GITEA_TOKEN \(the job token/);
    for (const r of [both, fallback]) assert.doesNotMatch(r.out, /release-secret|job-secret/);
    artifacts(dir);
    const none = await run(gitea.url, dir, { expr }, { RELEASE_TOKEN: '', GITEA_TOKEN: '' });
    assert.equal(none.status, 2, none.out);
    assert.match(none.out, /neither RELEASE_TOKEN nor the fallback GITEA_TOKEN/);
  } finally {
    await gitea.close();
    rmSync(dir, { recursive: true, force: true });
  }
});
