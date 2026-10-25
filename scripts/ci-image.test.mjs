// The shared CI image (plan X53): its pins, its scripts and the workflow that uses it must agree. Nothing here builds an image or
// touches Docker; the scripts run with a PATH of shims in a scratch folder.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => readFileSync(join(root, p), 'utf8');
const dockerfile = read('ci-image/Dockerfile');
const ci = read('.gitea/workflows/ci.yml');
const scratch = mkdtempSync(join(tmpdir(), 'ci-image-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

const arg = (name) => {
  const m = dockerfile.match(new RegExp(`^ARG ${name}=(\\S+)$`, 'm'));
  assert.ok(m, `ARG ${name} is missing from the Dockerfile`);
  return m[1];
};

describe('pins', () => {
  test('every checksum is 64 lowercase hex digits and every version has an amd64 and an arm64 sum (Chrome: amd64 only)', () => {
    const sums = [...dockerfile.matchAll(/^ARG (\w+_SHA256_(AMD64|ARM64))=(\S+)$/gm)];
    assert.ok(sums.length >= 9, `only ${sums.length} checksums found`);
    for (const [, name, , value] of sums) assert.match(value, /^[0-9a-f]{64}$/, name);
    for (const tool of ['SCCACHE', 'HAKARI', 'MACHETE', 'DUCKDB']) {
      assert.ok(dockerfile.includes(`ARG ${tool}_SHA256_AMD64=`) && dockerfile.includes(`ARG ${tool}_SHA256_ARM64=`), tool);
      assert.match(arg(`${tool}_VERSION`), /^\d+\.\d+\.\d+$/);
    }
    assert.ok(dockerfile.includes('ARG CHROME_SHA256_AMD64='));
  });

  test('Chrome for Testing is the build the apps download (paddock-chromium chromium.toml, linux-x86_64)', () => {
    const toml = read('crates/paddock-chromium/src/chromium.toml');
    const version = toml.match(/^version = "([^"]+)"$/m)[1];
    const linux = toml.split('[[platform]]').find((t) => t.includes('key = "linux-x86_64"'));
    assert.equal(arg('CHROME_VERSION'), version);
    assert.equal(arg('CHROME_SHA256_AMD64'), linux.match(/sha256 = "([0-9a-f]{64})"/)[1]);
    assert.ok(linux.includes(`/${version}/linux64/chrome-linux64.zip`));
  });

  test('sccache is the version the setup-rust action pins, with the same checksums', () => {
    const action = read('.gitea/actions/setup-rust/action.yml');
    assert.equal(arg('SCCACHE_VERSION'), action.match(/VERSION=(\d+\.\d+\.\d+)/)[1]);
    assert.ok(action.includes(arg('SCCACHE_SHA256_AMD64')), 'amd64 checksum differs from the action');
    assert.ok(action.includes(arg('SCCACHE_SHA256_ARM64')), 'arm64 checksum differs from the action');
  });

  test('Node is the .nvmrc major', () => {
    const nvmrc = read('.nvmrc').trim().replace(/^v/, '').split('.')[0];
    assert.equal(arg('NODE_MAJOR'), nvmrc);
  });

  test('Playwright is the version the UI locks, in both stages', () => {
    const lock = JSON.parse(read('ui/package-lock.json'));
    const locked = lock.packages['node_modules/playwright-core'].version;
    const all = [...dockerfile.matchAll(/^ARG PLAYWRIGHT_VERSION=(\S+)$/gm)].map((m) => m[1]);
    assert.ok(all.length >= 2);
    for (const v of all) assert.equal(v, locked);
    assert.match(arg('CHROME_VERSION'), /^\d+\.\d+\.\d+\.\d+$/);
  });

  test('the base is the garage image and the stage-2 copies of the version ARGs agree', () => {
    assert.match(arg('GARAGE_IMAGE').replace('dev.decryptology.net/decryp7/', ''), /^garage:ubuntu-24\.04-\d{8}-r\d+$/);
    for (const name of ['CHROME_VERSION', 'DUCKDB_VERSION', 'SCCACHE_VERSION']) {
      const values = new Set([...dockerfile.matchAll(new RegExp(`^ARG ${name}=(\\S+)$`, 'gm'))].map((m) => m[1]));
      assert.equal(values.size, 1, `${name} differs between stages`);
    }
  });

  test('nothing is downloaded except through the checksum-verified fetch, and the build ends in the verification', () => {
    assert.ok(!/\bcurl\b/.test(dockerfile.replace(/^#.*$/gm, '')), 'a raw curl in the Dockerfile');
    assert.ok(!/\|\s*(ba)?sh\b/.test(dockerfile.replace(/^#.*$/gm, '')), 'a download piped into a shell');
    const last = dockerfile.lastIndexOf('paddock-ci-verify');
    assert.ok(last > dockerfile.indexOf('COPY verify.sh'), 'verify.sh is copied and then run');
    for (const e of ['EXPECT_NODE_MAJOR', 'EXPECT_SCCACHE', 'EXPECT_PLAYWRIGHT', 'EXPECT_DUCKDB', 'EXPECT_CHROME']) assert.ok(dockerfile.includes(e), e);
  });
});

describe('the workflow and ci.yml', () => {
  const version = read('ci-image/VERSION').trim();
  const image = `dev.decryptology.net/decryp7/paddock-ci:v${version}`;

  test('VERSION is a number', () => assert.match(version, /^[1-9]\d*$/));

  test('ci.yml runs every Linux job in the image named by VERSION and in no other', () => {
    const refs = [...ci.matchAll(/dev\.decryptology\.net\/decryp7\/[\w-]+:[\w.-]+/g)].map((m) => m[0]);
    assert.ok(refs.length >= 6, `${refs.length} image references`);
    for (const r of refs) assert.equal(r, image);
  });

  test('ci.yml no longer installs Node, browsers or system packages per job', () => {
    assert.ok(!ci.includes('actions/setup-node'), 'setup-node is back');
    assert.ok(!/playwright install/.test(ci), 'a browser download is back');
    assert.ok(!/apt-get/.test(ci), 'apt-get is back');
    assert.ok(!ci.includes('tauri-linux-deps'), 'the Tauri apt step is back');
  });

  test('the build workflow is a manual dispatch that pushes the VERSION tag from the repo, with no local credentials', () => {
    const wf = read('.gitea/workflows/ci-image.yml');
    assert.match(wf, /^on:\n {2}workflow_dispatch:\n/m);
    const triggers = wf.match(/^on:\n((?: .*\n)+)/m)[1];
    assert.ok(!/^\s+(push|pull_request|schedule):/m.test(triggers), 'it must not run on push');
    assert.ok(wf.includes('paddock-ci:v${{ steps.v.outputs.image }}'));
    assert.ok(wf.includes('ci-image/VERSION') && wf.includes('.nvmrc'));
    assert.ok(wf.includes('secrets.DOCKER_USERNAME') && wf.includes('secrets.DOCKER_PASSWORD'));
    assert.ok(wf.includes('platforms: linux/amd64'));
  });

  test('the setup-rust action skips the toolchain install inside the image', () => {
    const action = read('.gitea/actions/setup-rust/action.yml');
    assert.ok(action.includes('/etc/paddock-ci-release'));
  });
});

const sh = (script, args, { env = {}, cwd = scratch } = {}) => {
  const r = spawnSync('/bin/bash', [join(root, 'ci-image', script), ...args], { cwd, env, encoding: 'utf8' });
  return { code: r.status, out: `${r.stdout}${r.stderr}` };
};

describe('fetch-verified.sh', () => {
  const dir = join(scratch, 'fetch');
  mkdirSync(dir, { recursive: true });
  const body = 'hello pinned world\n';
  const src = join(dir, 'src.txt');
  writeFileSync(src, body);
  const sum = createHash('sha256').update(body).digest('hex');
  const env = { PATH: process.env.PATH };

  test('keeps a file whose checksum matches', () => {
    const dest = join(dir, 'ok', 'a.txt');
    const r = sh('fetch-verified.sh', [`file://${src}`, sum, dest], { env });
    assert.equal(r.code, 0, r.out);
    assert.equal(readFileSync(dest, 'utf8'), body);
    assert.match(r.out, /verified a\.txt/);
  });

  test('deletes a file whose checksum differs and says both sums', () => {
    const dest = join(dir, 'bad.txt');
    const wrong = '0'.repeat(64);
    const r = sh('fetch-verified.sh', [`file://${src}`, wrong, dest], { env });
    assert.equal(r.code, 1);
    assert.ok(!existsSync(dest), 'the unverified file must not stay');
    assert.match(r.out, /checksum mismatch/);
    assert.ok(r.out.includes(wrong) && r.out.includes(sum));
  });

  test('a failed download leaves nothing behind', () => {
    const dest = join(dir, 'gone.txt');
    const r = sh('fetch-verified.sh', [`file://${join(dir, 'nope')}`, sum, dest], { env });
    assert.equal(r.code, 1);
    assert.ok(!existsSync(dest));
    assert.match(r.out, /download failed/);
  });

  test('refuses a malformed checksum or the wrong number of arguments before downloading', () => {
    for (const bad of ['', 'ABC', 'z'.repeat(64), 'a'.repeat(63), 'A'.repeat(64)]) {
      const r = sh('fetch-verified.sh', [`file://${src}`, bad, join(dir, 'x.txt')], { env });
      assert.equal(r.code, 2, `'${bad}': ${r.out}`);
      assert.ok(!existsSync(join(dir, 'x.txt')));
    }
    assert.equal(sh('fetch-verified.sh', ['only-one'], { env }).code, 2);
  });
});

describe('verify.sh', () => {
  // A machine of shims: only what the script calls, so a missing tool is really missing.
  function machine(over = {}) {
    const m = mkdtempSync(join(scratch, 'm-'));
    const bin = join(m, 'bin');
    mkdirSync(bin);
    for (const real of ['head', 'ls', 'date', 'cat', 'dirname']) {
      for (const d of ['/usr/bin', '/bin']) if (existsSync(join(d, real))) { symlinkSync(join(d, real), join(bin, real)); break; }
    }
    const put = (name, text) => { writeFileSync(join(bin, name), `#!/bin/bash\n${text}\n`); chmodSync(join(bin, name), 0o755); };
    const tools = {
      rustc: 'echo "rustc 1.99.1 (x 2026-10-13)"', cargo: 'case "$1" in clippy) echo "clippy 0.1.99";; nextest) echo "cargo-nextest 0.9.149";; llvm-cov) echo "cargo-llvm-cov 0.9.1";; deny) echo "cargo-deny 0.20.2";; hakari) echo "cargo-hakari 0.9.39";; machete) echo "0.9.2";; *) echo "cargo 1.99.1";; esac',
      rustfmt: 'echo "rustfmt 1.10.0-stable"', sccache: 'echo "sccache 0.18.0"', node: 'echo "v24.19.0"', npm: 'echo "11.17.0"', 'xvfb-run': 'true',
      'pkg-config': 'case "$2" in webkit2gtk-4.1) [ "$1" = "--modversion" ] && echo 2.52.6; exit 0;; gtk+-3.0) exit 0;; esac; exit 1',
      ...over.tools,
    };
    for (const [n, t] of Object.entries(tools)) if (t !== null) put(n, t);
    const pw = join(m, 'pw');
    mkdirSync(join(pw, 'chromium-1248'), { recursive: true });
    mkdirSync(join(pw, 'webkit-2370'), { recursive: true });
    const duck = join(m, 'duck');
    mkdirSync(duck);
    if (!over.noDuck) writeFileSync(join(duck, 'libduckdb.so'), '');
    const chrome = join(m, 'chrome');
    put('chrome', over.chrome ?? 'echo "Google Chrome for Testing 156.0.8078.47 "');
    writeFileSync(chrome, readFileSync(join(bin, 'chrome')));
    chmodSync(chrome, 0o755);
    const env = {
      PATH: bin, PLAYWRIGHT_BROWSERS_PATH: pw, DUCKDB_LIB_DIR: duck, CHROMIUM_TEST_PATH: chrome,
      EXPECT_NODE_MAJOR: '24', EXPECT_SCCACHE: '0.18.0', EXPECT_PLAYWRIGHT: '1.64.1', EXPECT_DUCKDB: '1.5.6', EXPECT_CHROME: '156.0.8078.47', ...over.env,
    };
    return { m, env, manifest: join(m, 'release') };
  }
  const run = (mc) => sh('verify.sh', ['--manifest', mc.manifest], { env: mc.env });

  test('a complete machine passes and writes the manifest with every version', () => {
    const mc = machine();
    const r = run(mc);
    assert.equal(r.code, 0, r.out);
    const text = readFileSync(mc.manifest, 'utf8');
    for (const s of ['rustc 1.99.1', 'cargo-nextest 0.9.149', 'cargo-hakari 0.9.39', 'cargo-machete: 0.9.2', 'sccache: sccache 0.18.0', 'node: v24.19.0', 'chrome for testing: Google Chrome for Testing 156.0.8078.47', 'libduckdb 1.5.6', 'playwright chromium', 'webkit2gtk-4.1: 2.52.6']) {
      assert.ok(text.includes(s), `${s}\n${text}`);
    }
  });

  test('a missing tool fails the build and is named, without writing a manifest', () => {
    const mc = machine({ tools: { sccache: null, 'xvfb-run': null } });
    const r = run(mc);
    assert.equal(r.code, 1);
    assert.match(r.out, /sccache is missing/);
    assert.match(r.out, /xvfb-run \(xvfb\) is missing/);
    assert.ok(!existsSync(mc.manifest));
  });

  test('versions off their pin fail: sccache, Node major, Chrome', () => {
    assert.match(run(machine({ tools: { sccache: 'echo "sccache 0.19.0"' } })).out, /sccache is 'sccache 0\.19\.0', the image pins 0\.18\.0/);
    assert.match(run(machine({ tools: { node: 'echo "v22.1.0"' } })).out, /node is v22\.1\.0, \.nvmrc wants major 24/);
    const r = run(machine({ chrome: 'echo "Google Chrome for Testing 150.0.1.2"' }));
    assert.equal(r.code, 1);
    assert.match(r.out, /Chrome for Testing is 'Google Chrome for Testing 150\.0\.1\.2', the image pins 156\.0\.8078\.47/);
  });

  test('missing Chrome, libduckdb, browsers and Tauri libraries are each reported', () => {
    const mc = machine({ noDuck: true, env: { CHROMIUM_TEST_PATH: '/nonexistent/chrome' }, tools: { 'pkg-config': 'exit 1' } });
    const r = run(mc);
    assert.equal(r.code, 1);
    assert.match(r.out, /Chrome for Testing is missing/);
    assert.match(r.out, /libduckdb\.so is missing/);
    assert.match(r.out, /webkit2gtk-4\.1 \(Tauri's Linux libraries\) is missing/);
    assert.match(r.out, /gtk\+-3\.0/);
    const nobrowsers = machine({ env: { PLAYWRIGHT_BROWSERS_PATH: join(scratch, 'empty-pw') } });
    assert.match(run(nobrowsers).out, /Playwright chromium is missing/);
  });

  test('an expectation that is not set is not checked', () => {
    const mc = machine({ env: { EXPECT_CHROME: '', EXPECT_DUCKDB: '', EXPECT_SCCACHE: '', EXPECT_NODE_MAJOR: '' }, tools: { sccache: 'echo "sccache 9.9.9"', node: 'echo v1.0.0' } });
    assert.equal(run(mc).code, 0);
  });
});
