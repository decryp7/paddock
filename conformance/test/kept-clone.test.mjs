// The kept clone of a host job (conformance/ci/kept-clone.sh, .gitea/actions/kept-clone; ADR 0025, "Kept clones"): one git clone
// per job slot, brought to the job's exact commit by a fetch, cleaned, left without the paths its profile excludes. And the
// profile file's contract: nothing a lean job reads is excluded.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { parseSteps } from '../src/util/workflows.mjs';
import { PADDOCK } from './helpers.mjs';

const read = (p) => readFileSync(p, 'utf8');
const SCRIPT = join(PADDOCK, 'conformance', 'ci', 'kept-clone.sh');
const PROFILES = join(PADDOCK, 'conformance', 'ci', 'checkout-profiles.txt');

/** The declared lines of the profile file: [{ kind, args: [...], rest }]. */
function declared(file = PROFILES) {
  return read(file).split('\n').filter((l) => l.trim() && !l.startsWith('#')).map((l) => {
    const [kind, ...parts] = l.trim().split(/\s+/);
    return { kind, parts, line: l };
  });
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Behaviour, against a local origin that allows partial clones and fetches by commit like the Gitea server does.

const ENV = (root) => ({ PATH: process.env.PATH, HOME: join(root, 'home'), GIT_CONFIG_NOSYSTEM: '1', GIT_TERMINAL_PROMPT: '0' });
function git(cwd, root, ...args) {
  const r = spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'init.defaultBranch=main', '-c', 'protocol.file.allow=always', ...args], { cwd, encoding: 'utf8', env: ENV(root) });
  assert.equal(r.status, 0, `git ${args.join(' ')}: ${r.stderr}`);
  return r.stdout.trim();
}

/** An origin with a commit that has a big folder to leave out, a work repo to commit with, and a fixture profile file. */
function fixture() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-kept-')));
  mkdirSync(join(root, 'home'));
  const work = join(root, 'work-repo');
  mkdirSync(work);
  git(work, root, 'init', '-q');
  const put = (rel, text) => { mkdirSync(dirname(join(work, rel)), { recursive: true }); writeFileSync(join(work, rel), text); };
  put('crates/a/lib.rs', 'one');
  put('big/shot.png', 'png');
  put('apps/x/docs/screenshots/s.png', 'png');
  put('apps/x/docs/guide.md', 'guide');
  put('.gitignore', 'target/\n');
  git(work, root, 'add', '-A');
  git(work, root, 'commit', '-qm', 'one');
  const origin = join(root, 'origin.git');
  git(root, root, 'clone', '-q', '--bare', work, origin);
  git(origin, root, 'config', 'uploadpack.allowFilter', 'true');
  git(origin, root, 'config', 'uploadpack.allowAnySHA1InWant', 'true');
  git(work, root, 'remote', 'add', 'origin', `file://${origin}`);
  writeFileSync(join(root, 'profiles.txt'), 'exclude lean big/ large\nexclude lean apps/*/docs/screenshots shots\nexclude full nothing/ nothing\nbootstrap .gitea\n');
  return { root, work, origin, put, commit: (msg) => { git(work, root, 'add', '-A'); git(work, root, 'commit', '-qm', msg); git(work, root, 'push', '-q', 'origin', 'HEAD:refs/heads/main'); return git(work, root, 'rev-parse', 'HEAD'); } };
}

/** A job: a bootstrap clone of the origin at `sha` in a fresh hashed work folder, then the script. */
function job(fx, sha, { os = 'macOS', slot = 's1', env = {}, hash = Math.random().toString(16).slice(2, 10), profile = 'lean', url } = {}) {
  const ws = join(fx.root, 'wd', hash, 'hostexecutor');
  mkdirSync(dirname(ws), { recursive: true });
  const remote = url ?? `file://${fx.origin}`;
  git(fx.root, fx.root, 'clone', '-q', '--no-checkout', remote.startsWith('file://') && existsSync(fx.origin) ? `file://${fx.origin}` : fx.origin, ws);
  git(ws, fx.root, 'remote', 'set-url', 'origin', remote);
  git(ws, fx.root, 'config', 'http.https://example.invalid/.extraheader', 'AUTHORIZATION: basic SECRETTOKEN');
  git(ws, fx.root, '-c', 'advice.detachedHead=false', 'checkout', '-q', '--detach', sha);
  const ghEnv = join(fx.root, `github-env-${hash}`);
  writeFileSync(ghEnv, '');
  const r = spawnSync('bash', [SCRIPT], {
    cwd: ws,
    encoding: 'utf8',
    env: { ...ENV(fx.root), RUNNER_OS: os, PADDOCK_CI_SLOT: slot, GITHUB_WORKSPACE: ws, GITHUB_ENV: ghEnv, GITHUB_ACTION_PATH: join(PADDOCK, '.gitea', 'actions', 'kept-clone'), NAME: 'decryp7/paddock-crate', PROFILE: profile, KEPT_CLONE_PROFILES: join(fx.root, 'profiles.txt'), ...env },
  });
  return { ...r, ws, ghEnv, out: r.stdout + r.stderr, stable: (s = slot) => join(fx.root, 'home', '.paddock-ci', 'src', `decryp7_paddock-crate-${s}`) };
}

test('kept-clone creates the slot\'s clone at the job\'s exact commit without the profile\'s excluded paths, and says so', () => {
  const fx = fixture();
  const sha = git(fx.work, fx.root, 'rev-parse', 'HEAD');
  const r = job(fx, sha);
  assert.equal(r.status, 0, r.out);
  const k = r.stable();
  assert.equal(git(k, fx.root, 'rev-parse', 'HEAD'), sha);
  assert.equal(read(join(k, 'crates/a/lib.rs')), 'one');
  assert.equal(read(join(k, 'apps/x/docs/guide.md')), 'guide', 'the text docs stay');
  assert.ok(!existsSync(join(k, 'big')), 'an excluded folder is not checked out');
  assert.ok(!existsSync(join(k, 'apps/x/docs/screenshots')), 'a pattern with * is excluded');
  assert.match(readFileSync(r.ghEnv, 'utf8'), new RegExp(`^PADDOCK_WORKSPACE=${k}$`, 'm'));
  for (const phase of ['resolve the job slot\'s kept folder', 'create the clone', 'fetch', 'check out', 'verify']) assert.match(r.out, new RegExp(`kept-clone: ${phase}[^\\n]*: done in \\d+ s`), phase);
  assert.match(r.out, new RegExp(`kept-clone: ready: ${sha} \\(created, profile lean, \\d+ files\\) in \\d+ s`));
});

test('kept-clone never prints the checkout\'s credentials and never keeps them in the clone', () => {
  const fx = fixture();
  const r = job(fx, git(fx.work, fx.root, 'rev-parse', 'HEAD'));
  assert.equal(r.status, 0, r.out);
  assert.doesNotMatch(r.out, /SECRETTOKEN/);
  assert.doesNotMatch(read(join(r.stable(), '.git', 'config')), /SECRETTOKEN|extraheader/);
});

test('kept-clone reuses the clone on the next job: a fetch of the new commit, and a clean tree whatever the last job left', () => {
  const fx = fixture();
  const first = job(fx, git(fx.work, fx.root, 'rev-parse', 'HEAD'));
  const k = first.stable();
  // What a job leaves behind: untracked and ignored files, a modified tracked file, a deleted one, an untracked folder in an excluded place.
  writeFileSync(join(k, 'left-over.rs'), '');
  mkdirSync(join(k, 'target'));
  writeFileSync(join(k, 'target', 'build.o'), '');
  writeFileSync(join(k, 'crates/a/lib.rs'), 'edited');
  mkdirSync(join(k, 'big'));
  writeFileSync(join(k, 'big', 'junk'), '');
  fx.put('crates/a/lib.rs', 'two');
  fx.put('crates/a/new.rs', 'new');
  const sha2 = fx.commit('two');
  const second = job(fx, sha2);
  assert.equal(second.status, 0, second.out);
  assert.match(second.out, /kept-clone: ready: [0-9a-f]{40} \(reused, profile lean/);
  assert.doesNotMatch(second.out, /create the clone/);
  assert.equal(git(k, fx.root, 'rev-parse', 'HEAD'), sha2);
  assert.equal(read(join(k, 'crates/a/lib.rs')), 'two');
  assert.equal(read(join(k, 'crates/a/new.rs')), 'new');
  for (const gone of ['left-over.rs', 'target', 'big']) assert.ok(!existsSync(join(k, gone)), `${gone} is gone`);
  assert.equal(git(k, fx.root, 'status', '--porcelain', '--ignored'), '');
});

test('kept-clone leaks nothing between branches: a file only the other branch has is gone, a run of another commit is exact', () => {
  const fx = fixture();
  const base = git(fx.work, fx.root, 'rev-parse', 'HEAD');
  git(fx.work, fx.root, 'checkout', '-q', '-b', 'topic');
  fx.put('topic-only.rs', 'x');
  git(fx.work, fx.root, 'add', '-A');
  git(fx.work, fx.root, 'commit', '-qm', 'topic');
  git(fx.work, fx.root, 'push', '-q', 'origin', 'topic');
  const topic = git(fx.work, fx.root, 'rev-parse', 'HEAD');
  const a = job(fx, topic);
  assert.ok(existsSync(join(a.stable(), 'topic-only.rs')));
  const b = job(fx, base);
  assert.equal(b.status, 0, b.out);
  assert.ok(!existsSync(join(b.stable(), 'topic-only.rs')));
  assert.equal(git(b.stable(), fx.root, 'rev-parse', 'HEAD'), base);
});

test('kept-clone keeps one clone per slot: a concurrent job in another slot has its own folder and the first is untouched', () => {
  const fx = fixture();
  const sha = git(fx.work, fx.root, 'rev-parse', 'HEAD');
  const one = job(fx, sha, { slot: 's1' });
  fx.put('crates/a/lib.rs', 'two');
  const sha2 = fx.commit('two');
  const two = job(fx, sha2, { slot: 's2' });
  assert.equal(two.status, 0, two.out);
  assert.notEqual(one.stable('s1'), two.stable('s2'));
  assert.equal(git(one.stable('s1'), fx.root, 'rev-parse', 'HEAD'), sha, 'slot s1 still at its own commit');
  assert.equal(read(join(one.stable('s1'), 'crates/a/lib.rs')), 'one');
  assert.equal(git(two.stable('s2'), fx.root, 'rev-parse', 'HEAD'), sha2);
  assert.match(two.out, /kept-clone: ready: [0-9a-f]{40} \(created/, 'the second slot made its own clone');
});

test('kept-clone on macOS leaves a link at the work folder, on Windows the work folder stays (act holds it open)', () => {
  const fx = fixture();
  const sha = git(fx.work, fx.root, 'rev-parse', 'HEAD');
  const mac = job(fx, sha, { os: 'macOS' });
  assert.equal(mac.status, 0, mac.out);
  assert.ok(lstatSync(mac.ws).isSymbolicLink());
  assert.equal(realpathSync(mac.ws), mac.stable());
  const win = job(fx, sha, { os: 'Windows', slot: 's2' });
  assert.equal(win.status, 0, win.out);
  assert.ok(!lstatSync(win.ws).isSymbolicLink());
  assert.match(readFileSync(win.ghEnv, 'utf8'), new RegExp(`^PADDOCK_WORKSPACE=${win.stable('s2')}$`, 'm'));
});

// Run 7808: on a freshly created clone `reset --hard` checked out the whole default-branch tree (the sparse patterns were not set yet):
// 50 to 110 s on the Windows runner. And a clone left by the old layout (a plain shallow checkout) was reused: its first fetch
// downloaded everything. Only a clone this script made (marker) is reused, and a new clone is not reset or cleaned before its patterns.
test('kept-clone does not clean a clone it has just created, and does not reuse a clone it did not make', () => {
  const fx = fixture();
  const sha = git(fx.work, fx.root, 'rev-parse', 'HEAD');
  const first = job(fx, sha);
  assert.doesNotMatch(first.out, /kept-clone: clean/, 'a new clone has nothing to clean');
  const k = first.stable();
  assert.ok(existsSync(join(k, '.git', 'paddock-kept-clone')), 'the clone carries the marker');
  rmSync(join(k, '.git', 'paddock-kept-clone'));
  const legacy = job(fx, sha);
  assert.equal(legacy.status, 0, legacy.out);
  assert.match(legacy.out, /\(created, profile lean/, 'a clone without the marker is made again');
  const warm = job(fx, sha);
  assert.match(warm.out, /\(reused, profile lean/);
  assert.match(warm.out, /kept-clone: clean: done/);
});

test('kept-clone replaces a clone that is broken or too big, and a clone of another repository', () => {
  const fx = fixture();
  const sha = git(fx.work, fx.root, 'rev-parse', 'HEAD');
  const first = job(fx, sha);
  const k = first.stable();
  writeFileSync(join(k, '.git', 'HEAD'), 'garbage\n');
  const broken = job(fx, sha);
  assert.equal(broken.status, 0, broken.out);
  assert.match(broken.out, /\(created, profile lean/);
  assert.equal(git(k, fx.root, 'rev-parse', 'HEAD'), sha);
  const big = job(fx, sha, { env: { PADDOCK_KEPT_CLONE_MAX_MIB: '0' } });
  assert.equal(big.status, 0, big.out);
  assert.match(big.out, /larger than 0 MiB; starting again/);
  assert.match(big.out, /\(created, profile lean/);
});

test('kept-clone falls back to the work folder with one warning when the clone cannot be made, and fails the job on nothing it can repair', () => {
  const fx = fixture();
  const sha = git(fx.work, fx.root, 'rev-parse', 'HEAD');
  const r = job(fx, sha, { url: `file://${fx.root}/no-such-origin.git`, env: {} });
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /::warning::kept-clone: [^\n]*building from the work folder/);
  assert.ok(!existsSync(r.ghEnv) || !/PADDOCK_WORKSPACE/.test(read(r.ghEnv)), 'later steps build in the work folder');
  assert.ok(!lstatSync(r.ws).isSymbolicLink());
  assert.equal(read(join(r.ws, 'crates/a/lib.rs')), 'one', 'the work folder has the commit already (the plain checkout)');
});

test('kept-clone leaves Linux containers alone, and a job without a slot builds in place with a warning', () => {
  const fx = fixture();
  const sha = git(fx.work, fx.root, 'rev-parse', 'HEAD');
  const linux = job(fx, sha, { os: 'Linux' });
  assert.equal(linux.status, 0, linux.out);
  assert.ok(!existsSync(linux.stable()));
  const none = job(fx, sha, { slot: '' });
  assert.equal(none.status, 0, none.out);
  assert.match(none.out, /::warning::kept-clone: no kept folder of this job's slot/);
});

test('kept-clone with the full profile leaves nothing out (and a profile file with no exclusion for it is fine)', () => {
  const fx = fixture();
  writeFileSync(join(fx.root, 'profiles.txt'), 'exclude lean big/ large\n');
  const r = job(fx, git(fx.work, fx.root, 'rev-parse', 'HEAD'), { profile: 'full' });
  assert.equal(r.status, 0, r.out);
  assert.equal(read(join(r.stable(), 'big/shot.png')), 'png');
  assert.match(r.out, /profile full/);
});

test('kept-clone fails with one ::error:: naming the profile when it does not exist', () => {
  const fx = fixture();
  const r = job(fx, git(fx.work, fx.root, 'rev-parse', 'HEAD'), { profile: 'nope' });
  assert.notEqual(r.status, 0);
  const errors = r.out.split('\n').filter((l) => l.startsWith('::error::'));
  assert.equal(errors.length, 1, r.out);
  assert.match(errors[0], /kept-clone: profile 'nope' is not declared in .*Next step:/);
});

// ---------------------------------------------------------------------------------------------------------------------------------
// The profile file and the workflow that uses it.

const CI = join(PADDOCK, '.gitea', 'workflows', 'ci.yml');
const tracked = () => spawnSync('git', ['ls-files', '-z'], { cwd: PADDOCK, encoding: 'utf8', maxBuffer: 1 << 30 }).stdout.split('\0').filter(Boolean);
const toRe = (p) => new RegExp(`^${p.split('*').map((s) => s.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('[^/]+')}(/|$)`);

test('checkout profiles: every exclusion names tracked files, every bootstrap folder exists', () => {
  const files = tracked();
  for (const d of declared()) {
    if (d.kind === 'exclude') assert.ok(files.some((f) => toRe(d.parts[1].replace(/\/$/, '')).test(f)), `exclude ${d.parts[1]}: no tracked file below it (stale entry)`);
    if (d.kind === 'bootstrap') assert.ok(files.some((f) => f.startsWith(`${d.parts[0]}/`)), `bootstrap ${d.parts[0]}: nothing tracked below it`);
    if (d.kind === 'mention') assert.ok(files.some((f) => f === d.parts[0] || (d.parts[0].endsWith('/') && f.startsWith(d.parts[0]))), `mention ${d.parts[0]}: no such file (stale entry)`);
    assert.ok(['bootstrap', 'exclude', 'mention'].includes(d.kind), `unknown kind in: ${d.line}`);
  }
});

test('checkout profiles: the bootstrap covers the inputs of paddock-dev, which job-isolation builds before the clone exists', () => {
  const boot = declared().filter((d) => d.kind === 'bootstrap').map((d) => d.parts[0]);
  const inputs = read(join(PADDOCK, 'tools', 'paddock-dev', 'inputs.txt')).split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('#'));
  // Cone mode also checks out the files directly in every parent folder of a listed folder (crates/Cargo.toml beside crates/paddock-slots).
  const covered = (i) => boot.some((b) => i === b || i.startsWith(`${b}/`) || (!i.slice(i.indexOf('/') + 1).includes('/') && i.includes('/') && b.startsWith(`${i.slice(0, i.indexOf('/'))}/`)));
  for (const i of inputs) assert.ok(covered(i), `inputs.txt names ${i}: the bootstrap must contain it`);
  for (const need of ['.gitea', 'conformance', 'tools']) assert.ok(boot.includes(need), `${need}: the actions, their scripts and ./dev`);
});

// Run 7816 (macOS): cargo loads every path dependency's manifest, dev-dependencies included (paddock-testkit), to build paddock-dev.
test('checkout profiles: the bootstrap holds every path dependency, dev-dependencies too, of paddock-dev, transitively', () => {
  const boot = declared().filter((d) => d.kind === 'bootstrap').map((d) => d.parts[0]);
  const seen = new Set();
  const walk = (dir) => {
    if (seen.has(dir)) return;
    seen.add(dir);
    for (const m of read(join(PADDOCK, dir, 'Cargo.toml')).matchAll(/path\s*=\s*"([^"]+)"/g)) {
      const dep = join(dir, m[1]).split('\\').join('/');
      if ((dep.startsWith('crates/') || dep.startsWith('tools/')) && existsSync(join(PADDOCK, dep, 'Cargo.toml'))) walk(dep);
    }
  };
  walk('tools/paddock-dev');
  for (const dir of seen) assert.ok(boot.some((b) => dir === b || dir.startsWith(`${b}/`)), `${dir} is a path dependency of paddock-dev: add \`bootstrap ${dir}\``);
});

test('checkout profiles: nothing outside the excluded paths mentions one, unless the file is declared and reasoned (mention)', () => {
  const excluded = declared().filter((d) => d.kind === 'exclude' && d.parts[0] === 'lean').map((d) => d.parts[1].replace(/\/$/, ''));
  const res = excluded.map(toRe);
  const tails = [...new Set(excluded.map((p) => p.split('/').slice(-2).join('/')))].map((t) => ({ t, re: new RegExp(`${t.replace(/[.+?^${}()|[\]\\]/g, '\\$&')}(?![A-Za-z0-9_.-])`) }));
  const mentions = declared().filter((d) => d.kind === 'mention').map((d) => d.parts[0]);
  const code = /\.(rs|ts|mjs|cjs|js|json|toml|ya?ml|sh|ps1|cmd|scss|css|html|txt)$/;
  const unexplained = [];
  const used = new Set();
  for (const f of tracked()) {
    if (res.some((r) => r.test(f)) || !code.test(f) || /package-lock\.json$|Cargo\.lock$|checkout-profiles\.txt$/.test(f)) continue;
    let text;
    try { text = read(join(PADDOCK, f)); } catch { continue; }
    const hit = tails.find(({ re }) => re.test(text));
    if (!hit) continue;
    const m = mentions.find((x) => f === x || (x.endsWith('/') && f.startsWith(x)));
    if (m) used.add(m); else unexplained.push(`${f} (${hit.t})`);
  }
  assert.deepEqual(unexplained, [], 'these files name a path the lean profile leaves out: if a job on a lean checkout reads it, take the path off the exclude list; otherwise add a `mention` line saying why not');
  for (const m of mentions) assert.ok(used.has(m), `mention ${m}: no file names an excluded path any more (stale entry)`);
});

test('checkout profiles: ci.yml\'s host jobs use the kept clone with a declared profile, and its bootstrap checkout is the declared one', () => {
  const ci = read(CI);
  const profiles = new Set(declared().filter((d) => d.kind === 'exclude').map((d) => d.parts[0]));
  const boot = declared().filter((d) => d.kind === 'bootstrap').map((d) => d.parts[0]);
  const jobs = ['crate', 'app', 'paddock-dev'];
  for (const id of jobs) {
    const text = jobBlock(ci, id);
    const steps = parseSteps(text);
    const clone = steps.find((s) => /uses: \.\/\.gitea\/actions\/kept-clone/.test(s.text));
    assert.ok(clone, `${id}: uses the kept-clone action`);
    assert.match(clone.text, /if: runner\.os != 'Linux'/, `${id}: host runners only`);
    assert.ok(!/actions\/stable-workspace/.test(text), `${id}: the kept clone replaces stable-workspace`);
    // Run 7816: a clone per job name made every job's clone new (14 to 36 s to create, 25 to 30 s to fill). A slot is held by one
    // job at a time, so ONE clone per slot serves every job; the profile is re-applied each time.
    assert.match(clone.text, /name: paddock-checkout-\$\{\{ runner\.os \}\}\n/, `${id}: the same kept clone for every job of the slot`);
    const profile = /profile: (\S+)/.exec(clone.text)?.[1];
    assert.ok(profiles.has(profile) || profile === 'full', `${id}: profile ${profile} is declared in checkout-profiles.txt`);
    const plain = steps.find((st) => /uses: actions\/checkout@v4/.test(st.text) && /if: runner\.os == 'Linux'/.test(st.text));
    assert.ok(plain, `${id}: Linux keeps the plain checkout`);
    const co = steps.find((st) => /uses: actions\/checkout@v4/.test(st.text) && /if: runner\.os != 'Linux'/.test(st.text));
    assert.ok(co, `${id}: the hosts check out sparse`);
    const sparse = /sparse-checkout: \|\n((?:\s{12}\S[^\n]*\n)+)/.exec(co.text + '\n');
    assert.ok(sparse, `${id}: a sparse-checkout list`);
    assert.deepEqual(sparse[1].split('\n').map((l) => l.trim()).filter(Boolean), boot, `${id}: the sparse set is the declared bootstrap`);
    assert.match(co.text, /filter: blob:none/, `${id}: a partial clone on the hosts (the full tree is what costs)`);
    // The work folder is only the bootstrap: every later read of the tree goes through the stable workspace.
    const bare = text.split('\n').filter((l) => /github\.workspace/.test(l) && !/PADDOCK_WORKSPACE \|\| github\.workspace/.test(l) && !/^\s*#/.test(l));
    assert.deepEqual(bare, [], `${id}: github.workspace without PADDOCK_WORKSPACE would read the bootstrap tree on Windows`);
  }
});

function jobBlock(text, id) {
  const lines = text.split('\n');
  const at = lines.findIndex((l) => l === `  ${id}:`);
  assert.ok(at >= 0, `job ${id}`);
  let end = lines.length;
  for (let i = at + 1; i < lines.length; i++) if (/^  [A-Za-z0-9_-]+:\s*$/.test(lines[i])) { end = i; break; }
  return lines.slice(at, end).join('\n');
}
