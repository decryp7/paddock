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

function spawn(s, script, env) {
  return spawnSync('bash', [join(s.app, 'scripts', 'ci', script)], {
    env: { PATH: process.env.PATH, HOME: join(s.root, 'home'), GITHUB_ENV: join(s.root, 'github-env'), GITHUB_PATH: join(s.root, 'github-path'), GITHUB_JOB: 'tauri-smoke', ...env },
    encoding: 'utf8',
  });
}

function run(s, script, env) {
  const r = spawn(s, script, env);
  assert.equal(r.status, 0, r.stderr + r.stdout);
  return { env: readFileSync(join(s.root, 'github-env'), 'utf8'), path: readFileSync(join(s.root, 'github-path'), 'utf8'), stdout: r.stdout };
}

/** Header lines saying libduckdb `version`, as the release archive's duckdb.h (and duckdb_v2.h from 2.0 on) say it. */
function headers(version) {
  const [ma, mi, pa] = version.split('.');
  if (Number(ma) >= 2) {
    // DuckDB 2.x: duckdb.h keeps the stable C API's version (1.5.6); the library's own is in duckdb_v2.h.
    return {
      'duckdb.h': '#define DUCKDB_API_VERSION_MAJOR 1\n#define DUCKDB_API_VERSION_MINOR 5\n#define DUCKDB_API_VERSION_PATCH 6\n',
      'duckdb_v2.h': `#define DUCKDB_V2_API_VERSION_MAJOR ${ma}\n#define DUCKDB_V2_API_VERSION_MINOR ${mi}\n#define DUCKDB_V2_API_VERSION_PATCH ${pa}\n`,
    };
  }
  return { 'duckdb.h': `#define DUCKDB_API_VERSION_MAJOR ${ma}\n#define DUCKDB_API_VERSION_MINOR ${mi}\n#define DUCKDB_API_VERSION_PATCH ${pa}\n` };
}

/** A fake fetch-libduckdb.sh: pins `version` and prints the exports the real one prints, for a folder it makes holding the
 * headers of `unpacks` (the pinned version unless a test says otherwise). With `fails`, it fails as the real one does when
 * the download fails (curl's error on stderr, nothing on stdout, a non-zero status). */
function fakeFetch(s, version, { unpacks = version, fails = false } = {}) {
  const f = join(s.root, 'fetch-libduckdb.sh');
  if (fails) {
    writeFileSync(f, `#!/bin/sh\nVERSION=${version}\necho "curl: (22) The requested URL returned error: 504" >&2\nexit 22\n`);
    return f;
  }
  const make = Object.entries(headers(unpacks))
    .map(([name, text]) => `printf '%s' '${text.replace(/\n/g, '\\n')}' | sed 's/\\\\n/\\n/g' > "$1/x/${name}"`)
    .join('\n');
  writeFileSync(f, `#!/bin/sh\nVERSION=${version}\nmkdir -p "$1/x"\n${make}\necho "$1" > "${join(s.root, 'fetched')}"\necho "export DUCKDB_LIB_DIR='$1/x' DUCKDB_INCLUDE_DIR='$1/x'"\necho "export DYLD_LIBRARY_PATH='$1/x':\\"\\\${DYLD_LIBRARY_PATH:-}\\""\necho "export LD_LIBRARY_PATH='$1/x':\\"\\\${LD_LIBRARY_PATH:-}\\""\n`);
  return f;
}

function imageLib(s, version) {
  const dir = join(s.root, 'opt-libduckdb');
  mkdirSync(dir, { recursive: true });
  for (const [name, text] of Object.entries(headers(version))) writeFileSync(join(dir, name), text);
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

test('a DuckDB 2.x image is recognised by duckdb_v2.h, not by the 1.5.6 in its duckdb.h', () => {
  const s = scratch();
  try {
    const lib = imageLib(s, '2.0.0');
    const { env } = run(s, 'libduckdb-env.sh', { RUNNER_OS: 'Linux', PADDOCK_CI_IMAGE: '1', DUCKDB_LIB_DIR: lib, DUCKDB_INCLUDE_DIR: lib, DEBRIEF_FETCH_LIBDUCKDB: fakeFetch(s, '2.0.0') });
    assert.match(env, new RegExp(`^DUCKDB_LIB_DIR=${lib}$`, 'm'));
    assert.ok(!existsSync(join(s.root, 'fetched')), 'nothing fetched');
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
    const isolation = join(APP, '..', '..', 'conformance', 'ci', 'job-isolation.sh');
    const mac = run(s, 'libduckdb-env.sh', { RUNNER_OS: 'macOS', PADDOCK_CI_SLOT: 's2', DEBRIEF_JOB_ISOLATION: isolation, DEBRIEF_FETCH_LIBDUCKDB: fakeFetch(s, '1.5.6') });
    const kept = join(s.root, 'home', '.paddock-ci', 'libduckdb', 'tauri-smoke-s2');
    assert.equal(readFileSync(join(s.root, 'fetched'), 'utf8').trim(), kept, 'one kept folder per job and job slot on a host');
    // (macOS strips DYLD_* from a protected shell's environment, so the job's own entries are checked on Linux above.)
    assert.match(mac.env, new RegExp(`^DYLD_LIBRARY_PATH=${kept}/x$`, 'm'), 'the library once, no empty entry');
    assert.match(linux.env, new RegExp(`^LD_LIBRARY_PATH=${join(s.root, 'tmp', 'libduckdb')}/x:/before$`, 'm'), 'the library once, the job\'s own entries kept');
  } finally {
    s.done();
  }
});

// CI run 7822 (2026-11-09): the download answered 504 four times, the failure was swallowed by `eval "$(...)"` and the job
// went on with the image's DuckDB 1.x in DUCKDB_LIB_DIR, so 235 tests failed on "async_threads" not being recognised.
test('a failed download fails the step with an error instead of falling back to the image libduckdb', () => {
  const s = scratch();
  try {
    const lib = imageLib(s, '1.5.6');
    const r = spawn(s, 'libduckdb-env.sh', { RUNNER_OS: 'Linux', RUNNER_TEMP: join(s.root, 'tmp'), PADDOCK_CI_IMAGE: '1', DUCKDB_LIB_DIR: lib, DEBRIEF_FETCH_LIBDUCKDB: fakeFetch(s, '2.0.0', { fails: true }) });
    assert.notEqual(r.status, 0, r.stdout);
    assert.match(r.stdout, /::error::.*libduckdb 2\.0\.0/);
    assert.doesNotMatch(readFileSync(join(s.root, 'github-env'), 'utf8'), /DUCKDB_LIB_DIR/, 'nothing written for later steps');
  } finally {
    s.done();
  }
});

test('a fetched libduckdb whose headers name another version fails the step', () => {
  const s = scratch();
  try {
    const r = spawn(s, 'libduckdb-env.sh', { RUNNER_OS: 'Linux', RUNNER_TEMP: join(s.root, 'tmp'), DEBRIEF_FETCH_LIBDUCKDB: fakeFetch(s, '2.0.0', { unpacks: '1.5.6' }) });
    assert.notEqual(r.status, 0, r.stdout);
    assert.match(r.stdout, /::error::.*1\.5\.6.*2\.0\.0/);
    assert.doesNotMatch(readFileSync(join(s.root, 'github-env'), 'utf8'), /DUCKDB_LIB_DIR/);
  } finally {
    s.done();
  }
});
