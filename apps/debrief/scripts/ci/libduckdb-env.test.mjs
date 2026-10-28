// scripts/ci/libduckdb-env.sh, run in a scratch copy of the app's layout with a fake runner
// environment (no network: the libduckdb download is a shim).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const APP = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** A scratch app folder with the script, .cargo/config.toml and a one-crate workspace. (The build folder is the shared
 * build-folder action's, tested in conformance/test/ci-actions.test.mjs.) */
function scratch() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'debrief-ci-folder-')));
  const app = join(root, 'apps', 'debrief');
  mkdirSync(join(root, 'conformance', 'ci'), { recursive: true });
  copyFileSync(join(APP, '..', '..', 'conformance', 'ci', 'ci-home.sh'), join(root, 'conformance', 'ci', 'ci-home.sh'));
  mkdirSync(join(app, 'scripts', 'ci'), { recursive: true });
  mkdirSync(join(app, '.cargo'));
  mkdirSync(join(app, 'src'));
  for (const s of ['libduckdb-env.sh']) copyFileSync(join(APP, 'scripts', 'ci', s), join(app, 'scripts', 'ci', s));
  copyFileSync(join(APP, '.cargo', 'config.toml'), join(app, '.cargo', 'config.toml'));
  writeFileSync(join(app, 'Cargo.toml'), '[package]\nname = "scratch"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n');
  writeFileSync(join(app, 'src', 'lib.rs'), '');
  writeFileSync(join(root, 'github-env'), '');
  writeFileSync(join(root, 'github-path'), '');
  return { root, app, done: () => rmSync(root, { recursive: true, force: true }) };
}

function run(s, script, env) {
  const r = spawnSync('bash', [join(s.app, 'scripts', 'ci', script)], {
    env: { PATH: process.env.PATH, HOME: join(s.root, 'home'), GITHUB_ENV: join(s.root, 'github-env'), GITHUB_PATH: join(s.root, 'github-path'), GITHUB_JOB: 'tauri-smoke', ...env },
    encoding: 'utf8',
  });
  assert.equal(r.status, 0, r.stderr + r.stdout);
  return { env: readFileSync(join(s.root, 'github-env'), 'utf8'), path: readFileSync(join(s.root, 'github-path'), 'utf8'), stdout: r.stdout };
}

/** A fake fetch-libduckdb.sh: pins `version` and prints the exports the real one prints, for a folder it makes. */
function fakeFetch(s, version) {
  const f = join(s.root, 'fetch-libduckdb.sh');
  writeFileSync(f, `#!/bin/sh\nVERSION=${version}\nmkdir -p "$1/x"\necho "$1" > "${join(s.root, 'fetched')}"\necho "export DUCKDB_LIB_DIR='$1/x' DUCKDB_INCLUDE_DIR='$1/x'"\necho "export DYLD_LIBRARY_PATH='$1/x':\\"\\\${DYLD_LIBRARY_PATH:-}\\""\necho "export LD_LIBRARY_PATH='$1/x':\\"\\\${LD_LIBRARY_PATH:-}\\""\n`);
  return f;
}

function imageLib(s, version) {
  const dir = join(s.root, 'opt-libduckdb');
  mkdirSync(dir, { recursive: true });
  const [ma, mi, pa] = version.split('.');
  writeFileSync(join(dir, 'duckdb.h'), `#define DUCKDB_API_VERSION_MAJOR ${ma}\n#define DUCKDB_API_VERSION_MINOR ${mi}\n#define DUCKDB_API_VERSION_PATCH ${pa}\n`);
  return dir;
}

test('the CI image libduckdb is used when it has the pinned version, without a download', () => {
  const s = scratch();
  try {
    const lib = imageLib(s, '1.5.6');
    const { env } = run(s, 'libduckdb-env.sh', { RUNNER_OS: 'Linux', PADDOCK_CI_IMAGE: '1', DUCKDB_LIB_DIR: lib, DUCKDB_INCLUDE_DIR: lib, DEBRIEF_FETCH_LIBDUCKDB: fakeFetch(s, '1.5.6') });
    assert.ok(!existsSync(join(s.root, 'fetched')), 'nothing fetched');
    assert.match(env, new RegExp(`^DUCKDB_LIB_DIR=${lib}$`, 'm'));
    assert.match(env, new RegExp(`^LD_LIBRARY_PATH=${lib}$`, 'm'));
  } finally {
    s.done();
  }
});

test('an image with another libduckdb version, or a host runner, fetches the pinned one into a kept folder', () => {
  const s = scratch();
  try {
    const lib = imageLib(s, '1.4.0');
    const linux = run(s, 'libduckdb-env.sh', { RUNNER_OS: 'Linux', RUNNER_TEMP: join(s.root, 'tmp'), LD_LIBRARY_PATH: '/before', PADDOCK_CI_IMAGE: '1', DUCKDB_LIB_DIR: lib, DEBRIEF_FETCH_LIBDUCKDB: fakeFetch(s, '1.5.6') });
    assert.match(linux.stdout, /::notice::the CI image's libduckdb is not 1\.5\.6/);
    assert.match(linux.env, new RegExp(`^DUCKDB_LIB_DIR=${join(s.root, 'tmp', 'libduckdb')}/x$`, 'm'));
    writeFileSync(join(s.root, 'github-env'), '');
    const mac = run(s, 'libduckdb-env.sh', { RUNNER_OS: 'macOS', DEBRIEF_FETCH_LIBDUCKDB: fakeFetch(s, '1.5.6') });
    const kept = join(s.root, 'home', '.paddock-ci', 'libduckdb', 'tauri-smoke');
    assert.equal(readFileSync(join(s.root, 'fetched'), 'utf8').trim(), kept, 'one kept folder per job on a host');
    // (macOS strips DYLD_* from a protected shell's environment, so the job's own entries are checked on Linux above.)
    assert.match(mac.env, new RegExp(`^DYLD_LIBRARY_PATH=${kept}/x$`, 'm'), 'the library once, no empty entry');
    assert.match(linux.env, new RegExp(`^LD_LIBRARY_PATH=${join(s.root, 'tmp', 'libduckdb')}/x:/before$`, 'm'), 'the library once, the job\'s own entries kept');
  } finally {
    s.done();
  }
});
