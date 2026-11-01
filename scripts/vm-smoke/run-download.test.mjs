import assert from 'node:assert/strict';
import { mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { downloadArtifact } from './run.mjs';

const REPO = 'o/r';
/** A fake Gitea: `routes` maps a path to [status, body]; the request log keeps path and the Authorization header. */
async function fake(routes) {
  const seen = [];
  const srv = createServer((req, res) => {
    seen.push([req.url, req.headers.authorization]);
    const r = routes[req.url];
    if (!r) { res.writeHead(404).end('no'); return; }
    res.writeHead(r[0], { 'content-type': r[2] ?? 'application/zip' }).end(r[1]);
  });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  return { server: `http://127.0.0.1:${srv.address().port}`, seen, close: () => srv.close() };
}
const run = async (routes, name = 'dyno-macOS') => {
  const f = await fake(routes);
  const dest = mkdtempSync(join(tmpdir(), 'dl-test-'));
  const lines = [];
  try {
    const dir = await downloadArtifact('7310', name, dest, { token: 'sekret', server: f.server, repoPath: REPO, log: (l) => lines.push(l), unzip: async () => ({ code: 0, stderr: '' }) }).then((d) => ({ dir: d }), (e) => ({ err: e }));
    return { ...dir, seen: f.seen, lines, dest };
  } finally {
    f.close();
  }
};

test('the web URL form is tried first, with the token in the header, and status and size are logged without the token', async () => {
  const r = await run({ [`/${REPO}/actions/runs/7310/artifacts/dyno-macOS`]: [200, 'ZIPDATA'] });
  assert.ok(r.dir, String(r.err));
  assert.deepEqual(r.seen, [[`/${REPO}/actions/runs/7310/artifacts/dyno-macOS`, 'token sekret']]);
  assert.match(r.lines[0], /HTTP 200, 7 bytes \(application\/zip\)/);
  assert.ok(!r.lines.join('\n').includes('sekret'), 'the token is never logged');
  assert.deepEqual(readdirSync(r.dest).sort(), ['dyno-macOS.zip', 'unzipped']);
  rmSync(r.dest, { recursive: true, force: true });
});

test('a 404 on the web URL falls back to the REST list for a v4 artifact', async () => {
  // Routes need the server's own port in the archive URL, so they are filled in after start.
  const routes = {};
  const s = await fake(routes);
  routes[`/api/v1/repos/${REPO}/actions/runs/7310/artifacts`] = [200, JSON.stringify({ artifacts: [{ name: 'dyno-macOS', archive_download_url: `${s.server}/dl/42` }] }), 'application/json'];
  routes['/dl/42'] = [200, 'V4ZIP'];
  const dest = mkdtempSync(join(tmpdir(), 'dl-test-'));
  const lines = [];
  try {
    const dir = await downloadArtifact('7310', 'dyno-macOS', dest, { token: 'sekret', server: s.server, repoPath: REPO, log: (l) => lines.push(l), unzip: async () => ({ code: 0, stderr: '' }) });
    assert.ok(dir.endsWith('unzipped'));
    assert.deepEqual(s.seen.map((x) => x[0]), [`/${REPO}/actions/runs/7310/artifacts/dyno-macOS`, `/api/v1/repos/${REPO}/actions/runs/7310/artifacts`, '/dl/42']);
    assert.match(lines.join('\n'), /HTTP 404/);
    assert.match(lines.at(-1), /HTTP 200, 5 bytes/);
  } finally {
    s.close();
    rmSync(dest, { recursive: true, force: true });
  }
});

test('a 404 everywhere is an error that names the artifact, the run and the next step', async () => {
  const r = await run({ [`/api/v1/repos/${REPO}/actions/runs/7310/artifacts`]: [200, '{"artifacts":[]}', 'application/json'] });
  assert.ok(r.err);
  assert.match(r.err.message, /artifact "dyno-macOS" of run 7310 was not found/);
  assert.match(r.err.message, /Next step: open .*actions\/runs\/7310/);
  assert.ok(!r.err.message.includes('sekret'));
  rmSync(r.dest, { recursive: true, force: true });
});

test('other statuses and missing input fail fast with a next step', async () => {
  const r = await run({ [`/${REPO}/actions/runs/7310/artifacts/dyno-macOS`]: [403, 'no'] });
  assert.match(r.err.message, /HTTP 403.*Next step: check GITEA_TOKEN/);
  rmSync(r.dest, { recursive: true, force: true });
  await assert.rejects(downloadArtifact('1', 'a/b', tmpdir(), { token: 't' }), /bad artifact name/);
  await assert.rejects(downloadArtifact('x', 'a', tmpdir(), { token: 't' }), /bad run id/);
});
