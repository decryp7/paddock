// Tests of scripts/affected.mjs (ADR 0014 §6): the rules on this tree's real graph, the pure parts (lock diff, base choice,
// matrix legs) and the contracts the script relies on (doc inputs, app CI files, CODEOWNERS, Renovate).
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, posix } from 'node:path';
import { describe, test } from 'node:test';
import { affected, appOfWorkflow, cargoGraph, chooseBase, DOC_INPUTS, DOCS, LINUX, lockDiff, matrix, npmGraph, onlyApp, owners, readApps, ROOT, summary, trackedFiles } from './affected.mjs';

const tracked = trackedFiles();
const graph = { cargo: cargoGraph(tracked), npm: npmGraph(tracked), apps: readApps(tracked) };
const run = (...files) => affected(files, graph);
const APPS = [...graph.apps.keys()].sort();
/** The apps that are legs of ci.yml (not those with a workflow of their own, such as Debrief). */
const LEGS = APPS.filter((a) => !graph.apps.get(a).workflow && graph.apps.get(a).cargo);

describe('the graph of this tree', () => {
  test('a shared crate marks crates and every app whose Cargo graph reaches it, Rust half only', () => {
    const r = run('crates/paddock-jobs/src/lib.rs');
    assert.ok(r.crates && !r.ui);
    // Debrief's store worker depends on paddock-proc by path (the macOS memory footprint), so the shared crates reach Debrief too.
    assert.deepEqual(r.apps, ['debrief', 'shell-demo', 'workbench-demo']);
    assert.deepEqual(r.kinds['shell-demo'], ['rust']);
    assert.deepEqual(r.kinds.debrief, ['rust']);
    assert.ok(r.packages.crates.includes('paddock-jobs'));
    assert.ok(r.packages['apps/shell-demo/src-tauri'].includes('paddock-shell-demo'));
  });

  test('a crate nothing else uses marks only crates', () => {
    const leaf = [...graph.cargo].find(([d, c]) => c.workspace === 'crates' && ![...graph.cargo.values()].some((o) => o.deps.includes(d)) && d.startsWith('crates/paddock-'));
    assert.ok(leaf, 'a leaf crate exists');
    const r = run(`${leaf[0]}/src/lib.rs`);
    assert.ok(r.crates);
    assert.deepEqual(r.apps.filter((a) => (r.packages[graph.apps.get(a).dir + '/src-tauri'] ?? []).length), r.apps);
  });

  test('an app crate marks only its app, with its reverse dependencies as packages', () => {
    const r = run('apps/shell-demo/src-tauri/agent/src/main.rs');
    assert.deepEqual([r.apps, r.crates, r.ui], [['shell-demo'], false, false]);
    assert.deepEqual(r.packages['apps/shell-demo/src-tauri'], ['paddock-shell-demo', 'shell-demo-agent']);
  });

  // Paddock 234f6911: a ui/ shell change broke the shell demo job, which has no npm or Cargo edge to ui/ (spike §2).
  test('a ui change marks the Tauri shells that embed a ui build (frontendDist), both halves', () => {
    const r = run('ui/projects/paddock-ui/shell/src/public-api.ts');
    assert.ok(r.ui);
    // Spotter maps @decryp7/paddock-ui to ui/dist in its tsconfig (a path edge, no npm edge).
    assert.deepEqual(r.apps, ['shell-demo', 'spotter', 'workbench-demo']);
    assert.deepEqual(r.kinds.spotter, ['ui']);
    assert.deepEqual(r.kinds['shell-demo'], ['rust', 'ui']);
  });

  test("ui's ESLint config reaches ../eslint, so a preset change runs ui's lint", () => {
    const r = run('eslint/src/preset.cjs');
    assert.ok(r.eslint && r.ui);
  });

  test('docs mark nothing but the link check; doc inputs read by code mark their projects', () => {
    const d = run('docs/adr/0014-monorepo-under-paddock.md', 'crates/paddock-jobs/README.md', 'README.md', 'diagrams/paddock.xml');
    assert.deepEqual([d.apps, d.crates, d.ui, d.docs], [[], false, false, true]);
    const c = run('apps/shell-demo/CHANGELOG.md');
    assert.deepEqual([c.apps, c.kinds['shell-demo']], [['shell-demo'], ['rust', 'ui']]);
    const rules = run('docs/rules.md');
    assert.deepEqual([rules.conformance, rules.tools, rules.docs, rules.apps], [true, true, false, []]);
  });

  test('global inputs mark everything; an app release workflow marks one app', () => {
    for (const f of ['.gitea/workflows/ci.yml', '.gitea/actions/setup-rust/action.yml', 'package.json', '.cargo/config.toml', 'scripts/affected.mjs', 'conformance/release/latest-json.mjs']) {
      const r = run(f);
      assert.ok(r.all && r.crates && r.ui && r.eslint && r.conformance && r.tools, f);
      assert.deepEqual(r.apps, APPS, f);
    }
    assert.deepEqual(run('.gitea/workflows/shell-demo-release.yml').apps, ['shell-demo']);
    assert.ok(!run('.gitea/workflows/shell-demo-release.yml').crates);
  });

  test('an unknown root file marks everything (fail safe); editor metadata marks nothing', () => {
    assert.ok(run('Makefile').all);
    const m = run('.gitignore', 'ui/.gitignore', '.editorconfig');
    assert.deepEqual([m.all, m.apps, m.ui, m.docs], [false, [], false, false]);
  });

  test("an app's icons mark that app; the icon pipeline marks every app; conformance's scripts mark every app", () => {
    assert.deepEqual(run('icons/shell-demo/shell-demo-macos-1024.png').apps, ['shell-demo']);
    assert.deepEqual(run('icons/tools/build.mjs').apps, APPS);
    assert.deepEqual(run('conformance/src/checks/C03-icons.mjs').apps, APPS);
    assert.deepEqual(run('conformance/test/core.test.mjs').apps, []);
  });

  test('scripts mark the tools and ui jobs (test:scripts), the image folder the image and ui jobs', () => {
    const s = run('scripts/publish.mjs');
    assert.deepEqual([s.tools, s.ui, s.crates], [true, true, false]);
    const i = run('ci-image/Dockerfile');
    assert.deepEqual([i.ciImage, i.ui, i.apps], [true, true, []]);
  });

  test('every project of a changed file is in the plan, and no app outside apps/ appears', () => {
    for (const f of tracked) {
      const own = owners(f);
      if (!own) continue;
      const r = run(f);
      for (const a of r.apps) assert.ok(graph.apps.has(a), `${f} names unknown app ${a}`);
    }
  });
});

describe('lockDiff (the root package-lock.json)', () => {
  const lock = (x, y = '1.0.0') =>
    JSON.stringify({
      packages: {
        '': { workspaces: ['ui', 'apps/a/app', 'apps/b/app'] },
        ui: { dependencies: { z: '1' } },
        'apps/a/app': { dependencies: { x: '1', ui: '*' } },
        'apps/b/app': { dependencies: { y: '1' } },
        'apps/b/app/node_modules/x': { version: '2.0.0' },
        'node_modules/ui': { resolved: 'ui', link: true },
        'node_modules/x': { version: x },
        'node_modules/y': { version: y },
        'node_modules/z': { version: '1.0.0' },
      },
    });

  test('only workspaces whose resolved packages changed', () => {
    assert.deepEqual(lockDiff(lock('1.0.0'), lock('1.0.1')), ['apps/a/app']);
    assert.deepEqual(lockDiff(lock('1.0.0', '1.0.0'), lock('1.0.0', '1.1.0')), ['apps/b/app']);
    assert.deepEqual(lockDiff(lock('1.0.0'), lock('1.0.0')), []);
  });

  test('a nested copy shadows the hoisted one (node lookup order), and a linked workspace is followed', () => {
    const before = JSON.parse(lock('1.0.0'));
    const after = structuredClone(before);
    after.packages['node_modules/z'].version = '1.0.1'; // ui's dependency; app a reaches it through the ui link
    assert.deepEqual(lockDiff(JSON.stringify(before), JSON.stringify(after)), ['apps/a/app', 'ui']);
  });

  test('a lock that is new or removed marks every workspace in it', () => {
    assert.deepEqual(lockDiff(null, lock('1.0.0')), ['apps/a/app', 'apps/b/app', 'ui']);
  });

  test('a root lock change marks its workspaces through the plan, Rust half untouched', () => {
    const r = affected(['package-lock.json'], graph, { lockBefore: lock('1.0.0'), lockAfter: lock('1.0.1') });
    assert.deepEqual([r.ui, r.crates, r.all], [false, false, false]);
  });
});

describe('chooseBase', () => {
  const contains = (sha) => sha !== 'elsewhere';
  const mergeBase = (b) => (b === 'main' ? 'mb123' : null);
  const run_ = (over) => ({ id: 1, conclusion: 'success', event: 'push', head_branch: 'main', path: 'ci.yml@refs/heads/main', head_sha: 'green1', ...over });

  test('a pull request diffs from its merge base', () => {
    assert.equal(chooseBase({ event: 'pull_request', baseRef: 'main', contains, mergeBase }).base, 'mb123');
    assert.equal(chooseBase({ event: 'pull_request', baseRef: 'gone', contains, mergeBase }).base, null);
  });

  test('a push to main diffs from the newest green ci push run that HEAD contains', () => {
    const runs = [
      run_({ head_sha: 'red', conclusion: 'failure' }),
      run_({ head_sha: 'other', path: 'canary.yml@refs/heads/main' }),
      run_({ head_sha: 'disp', event: 'workflow_dispatch' }),
      run_({ head_sha: 'elsewhere' }),
      run_({ head_sha: 'green1', id: 7 }),
      run_({ head_sha: 'green0' }),
    ];
    const r = chooseBase({ event: 'push', ref: 'refs/heads/main', runs, contains, mergeBase });
    assert.equal(r.base, 'green1');
    assert.match(r.reason, /run 7/);
  });

  test('a push to main with no green run, or unreadable runs, tests everything', () => {
    assert.equal(chooseBase({ event: 'push', ref: 'refs/heads/main', runs: [run_({ conclusion: 'failure' })], contains, mergeBase }).base, null);
    assert.equal(chooseBase({ event: 'push', ref: 'refs/heads/main', runs: null, contains, mergeBase }).base, null);
  });

  test('a manual run uses its base input or tests everything; tags and schedules test everything', () => {
    assert.equal(chooseBase({ event: 'workflow_dispatch', inputBase: 'abc', contains, mergeBase }).base, 'abc');
    assert.equal(chooseBase({ event: 'workflow_dispatch', inputBase: '', contains, mergeBase }).base, null);
    assert.equal(chooseBase({ event: 'push', ref: 'refs/tags/v0.4.0', contains, mergeBase }).base, null);
    assert.equal(chooseBase({ event: 'schedule', ref: 'refs/heads/main', contains, mergeBase }).base, null);
  });
});

describe('matrix legs', () => {
  test('one leg per affected app with its CI contract and halves', () => {
    const [leg] = matrix(run('apps/shell-demo/src-tauri/src/lib.rs'), graph);
    assert.equal(leg.app, 'shell-demo');
    assert.equal(leg.cargo, 'apps/shell-demo/src-tauri');
    assert.equal(leg.nextest_config, 'crates/.config/nextest.toml');
    assert.match(leg.prepare, /npm run shell-demo:build -w ui/);
    assert.match(leg.checks, /demo-release\.mjs shell-demo version/);
    assert.deepEqual([leg.rust, leg.ui, leg.filter], [true, false, '']);
  });

  test('a pull request narrows nextest to the reached crates of the workspace; everything never narrows', () => {
    const [leg] = matrix(run('apps/shell-demo/src-tauri/agent/src/main.rs'), graph, { narrow: true });
    assert.equal(leg.filter, '');
    const lib = matrix(run('apps/shell-demo/src-tauri/src/lib.rs'), graph, { narrow: true })[0];
    assert.equal(lib.filter, 'package(=paddock-shell-demo)');
    for (const l of matrix(affected([], graph, { all: true }), graph, { narrow: true })) assert.equal(l.filter, '');
  });

  test('everything gives every app both halves on Linux, and the summary names each leg', () => {
    const plan = affected([], graph, { all: true });
    const legs = matrix(plan, graph);
    assert.deepEqual(legs.filter((l) => l.os === LINUX && !l.e2e_shard).map((l) => [l.app, l.rust, l.ui]), LEGS.map((a) => [a, true, true]));
    const md = summary(plan, 'everything', legs);
    for (const a of LEGS) assert.match(md, new RegExp(`app ${a} \\| rust \\+ ui`));
  });

  test("an app's `os` adds a host leg per OS with the Rust gates only; a UI-only change adds none (Pitcrew)", () => {
    const rust = matrix(run('apps/pitcrew/crates/pitcrew-core/src/lib.rs'), graph);
    assert.deepEqual(rust.map((l) => [l.os, l.rust, l.ui]), [['ubuntu-latest', true, false], ['windows-latest', true, false], ['macos-latest', true, false]]);
    for (const host of rust.filter((l) => l.os !== LINUX)) assert.deepEqual([host.prepare, host.checks, host.ui_checks, host.angular_dir], ['', '', '', ''], 'the host legs carry no commands');
    assert.ok(rust.every((l) => l.cargo === 'apps/pitcrew' && l.processes === 'fake_runner'), 'the Cargo folder is normalised and the test processes are named');
    const ui = matrix(run('apps/pitcrew/app/src/main.ts'), graph);
    assert.deepEqual(ui.map((l) => [l.os, l.rust, l.ui]), [['ubuntu-latest', false, true]]);
    assert.equal(ui[0].angular_dir, 'apps/pitcrew/app');
    assert.match(ui[0].ui_checks, /ng build/);
  });

  test('an `os` list without Linux is refused: Linux carries the commands', () => {
    const apps = new Map([['x', { dir: 'apps/x', cargo: '.', os: ['macos-latest'] }]]);
    assert.throws(() => matrix({ apps: ['x'], kinds: { x: ['rust'] }, packages: {} }, { cargo: new Map(), apps }), /must include ubuntu-latest/);
  });

  test("an app's test browser and display reach every leg; the sandbox flag and the display are Linux-only", () => {
    const testBrowser = { envPrefix: 'DEMO', catalogue: 'apps/x/chromium.toml', noSandboxOnLinux: true };
    const apps = new Map([['x', { dir: 'apps/x', cargo: '.', os: [LINUX, 'windows-latest', 'macos-latest'], display: true, testBrowser }]]);
    const legs = matrix({ apps: ['x'], kinds: { x: ['rust'] }, packages: {} }, { cargo: new Map(), apps });
    assert.deepEqual(legs.map((l) => [l.os, l.test_browser_prefix, l.test_browser_catalogue, l.test_browser_no_sandbox, l.display]), [
      [LINUX, 'DEMO', 'apps/x/chromium.toml', 'true', 'true'],
      ['windows-latest', 'DEMO', 'apps/x/chromium.toml', 'false', 'false'],
      ['macos-latest', 'DEMO', 'apps/x/chromium.toml', 'false', 'false'],
    ]);
    const plain = matrix({ apps: ['x'], kinds: { x: ['rust'] }, packages: {} }, { cargo: new Map(), apps: new Map([['x', { dir: 'apps/x', cargo: '.' }]]) });
    assert.deepEqual([plain[0].test_browser_prefix, plain[0].test_browser_no_sandbox, plain[0].display], ['', 'false', 'false']);
  });

  test("`e2e` with `e2eShards` makes one Linux leg per shard and keeps the other UI lines in the main leg; the report folder reaches the Linux UI leg", () => {
    const apps = new Map([['x', { dir: 'apps/x', cargo: '.', ui: ['npm test'], e2e: ['npx playwright test --shard=$E2E_SHARD'], e2eShards: 2, e2eArtifacts: 'apps/x/app/results' }]]);
    const legs = matrix({ apps: ['x'], kinds: { x: ['rust', 'ui'] }, packages: {} }, { cargo: new Map(), apps });
    assert.deepEqual(legs.map((l) => [l.os, l.rust, l.ui, l.ui_checks, l.e2e_shard, l.e2e_checks, l.e2e_artifacts]), [
      [LINUX, true, true, 'npm test', '', '', 'apps/x/app/results'],
      [LINUX, false, false, '', '1/2', 'npx playwright test --shard=$E2E_SHARD', 'apps/x/app/results'],
      [LINUX, false, false, '', '2/2', 'npx playwright test --shard=$E2E_SHARD', 'apps/x/app/results'],
    ]);
    const rustOnly = matrix({ apps: ['x'], kinds: { x: ['rust'] }, packages: {} }, { cargo: new Map(), apps });
    assert.equal(rustOnly.length, 1, 'a change that does not reach the UI runs no e2e leg');
    const one = new Map([['x', { dir: 'apps/x', cargo: '.', e2e: ['npx playwright test'] }]]);
    assert.deepEqual(matrix({ apps: ['x'], kinds: { x: ['ui'] }, packages: {} }, { cargo: new Map(), apps: one }).map((l) => l.e2e_shard), ['', '1/1']);
  });

  test("Spotter's WebKit lines run on a dispatch with `webkit` as well as on main, tags and the nightly, and never on a plain pull request", () => {
    const line = graph.apps.get('spotter').ui.find((l) => l.includes('webkit'));
    const run = (env) => execFileSync('bash', ['-c', 'npx() { echo webkit-ran; }; ' + line], { env: { PATH: process.env.PATH, ...env }, encoding: 'utf8' }).trim();
    assert.equal(run({ GITHUB_EVENT_NAME: 'pull_request', GITHUB_REF: 'refs/pull/1/merge' }), '');
    assert.equal(run({ GITHUB_EVENT_NAME: 'workflow_dispatch', GITHUB_REF: 'refs/heads/topic' }), '');
    assert.equal(run({ GITHUB_EVENT_NAME: 'workflow_dispatch', GITHUB_REF: 'refs/heads/topic', PADDOCK_WEBKIT: '1' }), 'webkit-ran');
    assert.equal(run({ GITHUB_EVENT_NAME: 'push', GITHUB_REF: 'refs/heads/main' }), 'webkit-ran');
    assert.equal(run({ GITHUB_EVENT_NAME: 'push', GITHUB_REF: 'refs/tags/spotter-v1.0.0' }), 'webkit-ran');
    assert.equal(run({ GITHUB_EVENT_NAME: 'schedule', GITHUB_REF: 'refs/heads/main' }), 'webkit-ran');
    const e2e = graph.apps.get('spotter');
    assert.equal(e2e.e2eShards, 2);
    assert.match(e2e.e2e.join('\n'), /--shard=\$E2E_SHARD/);
  });

  test("onlyApp (a tag's gates, ci.yml's `app` input) keeps one app, both halves, and no other project", () => {
    const only = onlyApp(affected([], graph, { all: true }), 'pitcrew');
    assert.deepEqual([only.apps, only.kinds, only.crates, only.ui, only.eslint, only.conformance, only.tools, only.icons, only.all], [['pitcrew'], { pitcrew: ['rust', 'ui'] }, false, false, false, false, false, false, false]);
    assert.deepEqual(matrix(only, graph, { narrow: true }).map((l) => [l.os, l.rust, l.ui, l.filter]), [['ubuntu-latest', true, true, ''], ['windows-latest', true, false, ''], ['macos-latest', true, false, '']]);
  });
});

describe('contracts', () => {
  test('every folder under apps/ has a paddock.ci.json whose Cargo workspace and commands exist', () => {
    for (const d of readdirSync(join(ROOT, 'apps'), { withFileTypes: true }).filter((e) => e.isDirectory())) {
      if (!tracked.some((f) => f.startsWith(`apps/${d.name}/`))) continue;
      const app = graph.apps.get(d.name);
      assert.ok(app, `apps/${d.name}/paddock.ci.json is missing (the CI contract of its matrix leg)`);
      if (app.cargo) assert.ok(existsSync(join(ROOT, app.dir, app.cargo, 'Cargo.toml')), `${d.name}: cargo folder ${app.cargo} has no Cargo.toml`);
      if (app.nextestConfig) assert.ok(existsSync(join(ROOT, app.nextestConfig)), `${d.name}: ${app.nextestConfig} is missing`);
      for (const k of ['prepare', 'checks', 'ui']) assert.ok(Array.isArray(app[k] ?? []), `${d.name}: ${k} is a list of commands`);
    }
  });

  test("an app's process patterns (host-cleanup `pkill -f`) name its own temp folder and never match the real runner", () => {
    const legs = matrix(run('apps/pitcrew/crates/pitcrew-core/src/lib.rs'), graph);
    const patterns = graph.apps.get('pitcrew').processPatterns;
    assert.ok(patterns?.length, 'Pitcrew names the fake runner copy the tests start');
    assert.ok(legs.every((l) => l.process_patterns === patterns.join('\n')), 'every leg hands the patterns to host-cleanup');
    const ours = ['/var/folders/ab/T/pitcrew-it-stop-4242/runner/bin/gitea-runner daemon', 'C:\\Users\\x\\AppData\\Local\\Temp\\pitcrew-it-stop-7\\runner\\bin\\gitea-runner.exe daemon'];
    const real = ['gitea-runner daemon', '/usr/local/bin/gitea-runner daemon', '/Users/x/runners/main/bin/gitea-runner daemon', '/opt/act_runner/act_runner daemon', '/Users/x/pitcrew/runner/bin/gitea-runner daemon', '/tmp/pitcrew-it/gitea-runner', '/tmp/other-1/runner/bin/gitea-runner'];
    for (const pat of patterns) {
      const re = new RegExp(pat);
      for (const line of ours) assert.match(line, re, `${pat} misses the test copy`);
      for (const line of real) assert.doesNotMatch(line, re, `${pat} would kill ${line}`);
    }
  });

  test('every markdown file that code reads is a declared doc input', () => {
    // include_str!("rel.md") is resolved from its file; quoted repo paths ('docs/x.md', "apps/a/README.md") are taken as is.
    const re = String.raw`include_str!\("[^"]+\.md"\)|['"](docs|apps|crates|ui)/[A-Za-z0-9_./-]+\.md['"]`;
    const hits = execFileSync('git', ['grep', '-E', re, '--', '*.rs', '*.mjs', '*.ts', '*.cjs'], { cwd: ROOT, encoding: 'utf8' });
    const read = new Set();
    for (const line of hits.split('\n').filter(Boolean)) {
      const at = line.indexOf(':');
      const [file, text] = [line.slice(0, at), line.slice(at + 1)];
      if (/^\s*(\/\/|\*|#)/.test(text)) continue; // a comment names the file, nothing reads it
      if (DOCS.some((r) => r.test(file))) continue; // a docs tool (docs/images/src) reading docs is docs
      if (file === 'scripts/affected.test.mjs') continue; // this file names docs as test data
      for (const m of text.matchAll(new RegExp(re, 'g'))) {
        const inc = /include_str!\("([^"]+)"\)/.exec(m[0]);
        const path = inc ? posix.normalize(posix.join(posix.dirname(file), inc[1])) : m[0].slice(1, -1);
        if (tracked.includes(path) && DOCS.some((r) => r.test(path))) read.add(path);
      }
    }
    assert.ok(read.has('apps/shell-demo/CHANGELOG.md'), 'the grep finds the shell demo What\'s New');
    const undeclared = [...read].filter((f) => !DOC_INPUTS.some((d) => d.re.test(f)));
    assert.deepEqual(undeclared, []);
    // Read through join(): the rules and blocks that conformance sync, the release notes and new-app read.
    for (const f of ['docs/rules.md', 'docs/blocks/first-launch.md', 'docs/blocks/readme-skeleton.md']) assert.ok(DOC_INPUTS.some((d) => d.re.test(f)), f);
  });

  test('CODEOWNERS is in Gitea form (Go regular expressions, dots escaped) and owns every top-level path', () => {
    const lines = readFileSync(join(ROOT, 'CODEOWNERS'), 'utf8').split('\n').filter((l) => l.trim() && !l.startsWith('#'));
    const rules = lines.map((l) => {
      const [pattern, ...who] = l.trim().split(/\s+/);
      assert.ok(who.length && who.every((w) => w.startsWith('@')), `owners in "${l}"`);
      assert.ok(!/(^|[^\\])\.(?![*+?])/.test(pattern), `unescaped dot in ${pattern}`);
      assert.ok(!pattern.includes('**'), `${pattern} is a glob, not a Go regular expression`);
      return new RegExp(`^${pattern}$`);
    });
    for (const f of tracked) assert.ok(rules.some((r) => r.test(f)), `${f} has no owner`);
    for (const f of ['apps/shell-demo/src-tauri/src/lib.rs', '.gitea/workflows/ci.yml', 'crates/paddock-jobs/src/lib.rs']) assert.ok(rules.some((r) => r.test(f)));
  });

  test('renovate.json at the root covers every workspace and keeps build output out', () => {
    const r = JSON.parse(readFileSync(join(ROOT, 'renovate.json'), 'utf8'));
    assert.ok(r.ignorePaths.includes('**/node_modules/**'));
    assert.ok(r.ignorePaths.includes('apps/*/target/**'));
    const groups = r.packageRules.map((p) => p.groupName).filter(Boolean);
    for (const g of ['Angular', 'Tauri']) assert.ok(groups.includes(g), `group ${g}`);
    assert.equal(r.lockFileMaintenance?.enabled, true);
    assert.equal(r.lockFileMaintenance?.additionalBranchPrefix, '{{packageFileDir}}-');
  });

  test('one npm workspace: no package has a lockfile of its own and the root lists the workspaces', () => {
    const extra = tracked.filter((f) => f.endsWith('package-lock.json') && f !== 'package-lock.json' && !f.startsWith('templates/'));
    assert.deepEqual(extra, []);
    const root = JSON.parse(readFileSync(join(ROOT, 'package.json'), 'utf8'));
    for (const w of ['ui', 'apps/shell-demo', 'apps/*/app']) assert.ok(root.workspaces.includes(w), w);
    const names = root.workspaces.filter((w) => !w.includes('*')).map((w) => JSON.parse(readFileSync(join(ROOT, w, 'package.json'), 'utf8')).name);
    assert.equal(new Set(names).size, names.length, 'workspace package names are unique');
  });

  test('ci.yml passes every field of a matrix leg to app-gates, and app-gates declares each one', () => {
    const ci = readFileSync(join(ROOT, '.gitea/workflows/ci.yml'), 'utf8');
    const gates = readFileSync(join(ROOT, '.gitea/actions/app-gates/action.yml'), 'utf8');
    const [leg] = matrix(affected([], graph, { all: true }), graph);
    const job = ci.slice(ci.indexOf('\n  app:\n'));
    for (const key of Object.keys(leg).filter((k) => k !== 'dir')) {
      assert.match(job, new RegExp(`\\$\\{\\{ matrix\\.${key}( \\|\\| '[^']*')? \\}\\}`), `ci.yml passes matrix.${key}`);
    }
    for (const input of ['app', 'cargo', 'nextest-config', 'prepare', 'checks', 'ui-checks', 'rust', 'ui', 'filter']) assert.match(gates, new RegExp(`\\n  ${input}:\\n`), input);
    assert.match(job, /include: \$\{\{ fromJSON\(needs\.plan\.outputs\.apps\) \}\}/);
  });

  test('the root AGENTS.md links the rules (not a copy) and carries every common doc block verbatim', () => {
    const agents = readFileSync(join(ROOT, 'AGENTS.md'), 'utf8');
    assert.match(agents, /\[docs\/rules\.md\]\(docs\/rules\.md\)/);
    assert.ok(!agents.includes('1. **One app at a time.**'), 'the rules are linked, not copied (ADR 0014 §9)');
    const blocks = JSON.parse(readFileSync(join(ROOT, 'docs/blocks/blocks.json'), 'utf8'));
    for (const name of blocks.defaultTargets['AGENTS.md'].filter((n) => n !== 'rules')) {
      const file = blocks.blocks.find((b) => b.name === name).file;
      const text = readFileSync(join(ROOT, 'docs/blocks', file), 'utf8').trim();
      assert.ok(agents.includes(text), `docs/blocks/${file} is in AGENTS.md as it is`);
    }
  });

  test("an app's own workflows mark only that app, and an app with a workflow of its own is no leg of ci.yml", () => {
    const names = ['debrief', 'shell-demo', 'shell'];
    assert.equal(appOfWorkflow('.gitea/workflows/debrief.yml', names), 'debrief');
    assert.equal(appOfWorkflow('.gitea/workflows/debrief-fuzz.yml', names), 'debrief');
    assert.equal(appOfWorkflow('.gitea/workflows/release-debrief.yml', names), 'debrief');
    assert.equal(appOfWorkflow('.gitea/workflows/shell-demo-release.yml', names), 'shell-demo', 'the longest app name wins');
    assert.equal(appOfWorkflow('.gitea/workflows/ci.yml', names), null);
    assert.equal(appOfWorkflow('.gitea/actions/x/action.yml', names), null);
    const d = run('.gitea/workflows/debrief.yml');
    assert.deepEqual([d.apps, d.crates, d.ui, d.all], [['debrief'], false, false, false]);
    for (const [name, app] of graph.apps) {
      if (!app.workflow) continue;
      assert.ok(existsSync(join(ROOT, '.gitea/workflows', app.workflow)), `${name}: ${app.workflow} exists`);
      assert.match(readFileSync(join(ROOT, '.gitea/workflows', app.workflow), 'utf8'), new RegExp(`affected\\.mjs --ci --app ${name}`), `${app.workflow} asks the plan`);
      assert.ok(!matrix(affected([], graph, { all: true }), graph).some((l) => l.app === name), `${name} is no leg of ci.yml`);
    }
  });

  test('Pitcrew is a leg of ci.yml; its release workflow marks only Pitcrew and calls the kit', () => {
    assert.ok(LEGS.includes('pitcrew') && !graph.apps.get('pitcrew').workflow);
    const wf = run('.gitea/workflows/pitcrew-release.yml');
    assert.deepEqual([wf.apps, wf.crates, wf.ui, wf.all], [['pitcrew'], false, false, false]);
    const text = readFileSync(join(ROOT, '.gitea/workflows/pitcrew-release.yml'), 'utf8');
    assert.match(text, /uses: \.\/\.gitea\/workflows\/ci\.yml\n\s+with:\n\s+app: pitcrew/);
    assert.match(text, /uses: \.\/\.gitea\/workflows\/app-release\.yml/);
    assert.match(text, /tags: \["pitcrew-v\*"\]/);
    assert.match(text, /uses: \.\/\.gitea\/actions\/plan\n\s+with:\n\s+app: pitcrew\n\s+tag-pattern: pitcrew-v\*/, 'the shared plan action asks affected.mjs for Pitcrew');
  });

  test('Debrief: its crates mark only Debrief, its UI only the UI half, and a shared crate reaches it only through the crates it depends on by path', () => {
    const core = run('apps/debrief/crates/debrief-core/src/lib.rs');
    assert.deepEqual([core.apps, core.kinds.debrief, core.crates], [['debrief'], ['rust'], false]);
    assert.ok(core.packages.apps_debrief === undefined && core.packages['apps/debrief'].includes('debrief-app'));
    assert.deepEqual(run('apps/debrief/app/src/main.ts').kinds.debrief, ['ui']);
    assert.ok(run('crates/paddock-proc/src/lib.rs').apps.includes('debrief'), 'debrief-store-worker uses paddock-proc by path');
    assert.deepEqual(run('crates/paddock-proc/src/lib.rs').kinds.debrief, ['rust']);
    assert.deepEqual(run('apps/debrief/docs/ai/sql-views.md').apps, ['debrief']);
    assert.deepEqual(run('apps/debrief/docs/adr/0001-x.md').apps, []);
  });

  test('a docs-only app (Pacenotes, Sighter) is no leg, and its docs mark only the link check', () => {
    for (const a of ['pacenotes', 'sighter']) {
      assert.ok(graph.apps.has(a), `${a} has its paddock.ci.json`);
      assert.ok(!matrix(affected([], graph, { all: true }), graph).some((l) => l.app === a));
      const d = run(`apps/${a}/docs/x.md`);
      assert.deepEqual([d.apps, d.docs], [[], true]);
    }
  });
});

