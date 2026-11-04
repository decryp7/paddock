// Tests of scripts/affected.mjs (ADR 0014 §6): the rules on this tree's real graph, the pure parts (lock diff, base choice,
// matrix legs) and the contracts the script relies on (doc inputs, app CI files, CODEOWNERS, Renovate).
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, posix, win32 } from 'node:path';
import { describe, test } from 'node:test';
import { affected, appOfWorkflow, cargoGraph, changedJobs, chooseBase, ciOutputs, commitLintRange, planFor, scriptImporters, workflowJobs, DOC_INPUTS, DOCS, LINUX, lockDiff, matrix, npmGraph, onlyApp, owners, parseArgs, readApps, repoRelative, ROOT, summary, trackedFiles, wantsWebkit } from './affected.mjs';

/**
 * A paddock.ci.json process pattern as host-cleanup runs it: {tmp}, {build} and {slot} filled in by the real
 * conformance/ci/job-isolation.sh for a sample job (its own runner folder 0123456789abcdef, slot s1, build folder
 * target-<app>-app-s1), as a RegExp; null when the script refuses it (not anchored on a folder of the job).
 */
const JOB = { tmp: '/Users/x/gitea-runner/work/0123456789abcdef/tmp', ci: '/Users/x/.paddock-ci', slot: 's1' };
function scoped(pat, app, os = 'macOS') {
  const win = os === 'Windows';
  const r = spawnSync('bash', [join(ROOT, 'conformance', 'ci', 'job-isolation.sh'), 'pattern', pat], {
    env: { PATH: '/usr/bin:/bin', RUNNER_OS: os, PADDOCK_CI_SLOT: JOB.slot, PADDOCK_JOB_TEMP: (win ? 'C:' : '') + JOB.tmp, PADDOCK_BUILD_FOLDER: `${win ? 'C:' : ''}${JOB.ci}/target-${app}-app-${JOB.slot}` },
    encoding: 'utf8',
  });
  return r.status === 0 ? new RegExp(r.stdout.trim()) : null;
}
/** Every pattern of an app, for macOS and Windows command lines. */
const scopedAll = (app, pats) => pats.flatMap((p) => [scoped(p, app), scoped(p, app, 'Windows')]);

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
    // Debrief's store worker and Spotter's renderer depend on paddock-proc by path (the memory figure and the supervised child),
    // so the shared crates reach Debrief and Spotter too; Dyno takes the shell crates by path.
    assert.deepEqual(r.apps, ['debrief', 'dyno', 'shell-demo', 'spotter', 'workbench-demo']);
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

  test('the e2e port and build guard (imported by relative path by every app config) mark the UI half of every app', () => {
    for (const f of ['ui/projects/paddock-e2e/src/env.ts', 'ui/projects/paddock-e2e/src/build-guard.ts']) {
      const r = run(f);
      for (const a of APPS) assert.ok(r.apps.includes(a), `${f} reaches ${a}`);
    }
  });

  // Paddock 234f6911: a ui/ shell change broke the shell demo job, which has no npm or Cargo edge to ui/ (spike §2).
  test('a ui change marks the Tauri shells that embed a ui build (frontendDist), both halves', () => {
    const r = run('ui/projects/paddock-ui/shell/src/public-api.ts');
    assert.ok(r.ui);
    // Spotter and Dyno map @decryp7/paddock-ui to ui/dist in their tsconfig (a path edge, no npm edge).
    assert.deepEqual(r.apps, ['dyno', 'shell-demo', 'spotter', 'workbench-demo']);
    assert.deepEqual(r.kinds.spotter, ['ui']);
    assert.deepEqual(r.kinds.dyno, ['ui']);
    assert.deepEqual(r.kinds['shell-demo'], ['rust', 'ui']);
  });

  test('a generated ui file marks the crate that checks it, so a stale one fails on the branch', () => {
    const r = run('ui/projects/paddock-ui/shell/generated/dialogs.ts');
    assert.ok(r.crates && r.ui);
    assert.ok(r.packages.crates.includes('paddock-dialogs'));
    // A generated file no crate names marks every crate of the workspace (fail safe).
    const unknown = run('ui/projects/paddock-ui/shell/generated/no-such-file.json');
    assert.ok(unknown.crates);
    assert.ok(unknown.packages.crates.length > 5);
    // Other files in ui/ (not generated) do not mark crates.
    assert.ok(!run('ui/projects/paddock-ui/shell/src/public-api.ts').crates);
  });

  test("an app's Cargo.toml and capabilities run conformance (C48 reads them), other app files do not", () => {
    for (const f of ['apps/shell-demo/src-tauri/capabilities/dialog.json', 'apps/shell-demo/src-tauri/Cargo.toml', 'apps/debrief/app/src-tauri/capabilities/dialog.json']) {
      const r = run(f);
      assert.ok(r.conformance, f);
      assert.deepEqual(r.apps, [f.split('/')[1]], f);
    }
    assert.ok(!run('apps/shell-demo/src-tauri/src/lib.rs').conformance);
    assert.ok(!run('apps/shell-demo/README.md').conformance);
  });

  test("a per-machine app's Tauri config, per-machine.json and installer files run C91 and the shared crates' sync test", () => {
    for (const f of [
      'apps/dyno/app/src-tauri/tauri.conf.json',
      'apps/dyno/app/src-tauri/tauri.windows.conf.json',
      'apps/dyno/app/src-tauri/windows/per-machine.json',
      'apps/dyno/app/src-tauri/windows/installer-hooks.nsh',
      'apps/dyno/app/src-tauri/windows/paddock-per-machine.generated.nsh',
    ]) {
      const r = run(f);
      assert.ok(r.conformance, `${f} runs C91`);
      assert.ok(r.crates, `${f} runs paddock-service's per_machine sync test`);
      assert.deepEqual(r.apps, ['dyno'], f);
    }
    // The shared include itself is read by the same check and test, and every adopting app is marked.
    const inc = run('crates/paddock-setup/nsis/paddock-per-machine.nsh');
    assert.ok(inc.conformance && inc.crates);
    // Other app files do not.
    const other = run('apps/dyno/app/src-tauri/src/lib.rs');
    assert.ok(!other.conformance);
    assert.ok(!run('apps/dyno/app/src-tauri/icons/icon.ico').conformance);
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
    for (const f of ['rust-toolchain.toml', 'package.json', '.npmrc', 'tsconfig/base.json']) {
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

  test('the developer tool is its own project: its folder and the ./dev wrapper mark it, and the shared crates it uses reach it', () => {
    const own = run('tools/paddock-dev/src/lib.rs');
    assert.deepEqual([own.paddockDev, own.tools, own.crates, own.apps], [true, false, false, []]);
    for (const f of ['dev', 'dev.cmd']) assert.deepEqual([run(f).paddockDev, run(f).all], [true, false], f);
    assert.ok(run('crates/paddock-slots/src/lib.rs').paddockDev, 'a path dependency of the tool reaches it');
    assert.ok(run('crates/paddock-cli/src/lib.rs').paddockDev);
    assert.ok(run('.cargo/config.toml').paddockDev, 'global inputs reach it');
    assert.equal(run('tools/bootstrap.sh').paddockDev, false);
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

// Synthetic .gitea trees: the per-job diff of ci.yml and the users of an action, without touching the real workflows.
const CI = (conf = 'node a', app = 'node b') => `name: ci
on:
  push:
jobs:
  plan:
    steps:
      - uses: ./.gitea/actions/plan-only
  ui:
    steps:
      - uses: ./.gitea/actions/node-caches
  conformance: # the checks
    steps:
      - run: ${conf}
  app:
    steps:
      - run: ${app}
      - uses: ./.gitea/actions/app-gates
  crates:
    steps:
      - uses: ./.gitea/actions/build-folder
      - uses: ./.gitea/actions/install-test-browser
`;
const giteaOf = (before, after, dirty = null) => {
  const trees = { before, after: { ...before, ...after } };
  return {
    appNames: APPS,
    files: () => Object.keys(trees.after),
    dirty,
    scripts: () => Object.keys(trees.after).filter((f) => /^conformance\/.*\.(mjs|sh)$/.test(f)),
    before: (f) => trees.before[f] ?? null,
    after: (f) => trees.after[f] ?? null,
  };
};
const ACTIONS = {
  '.gitea/actions/plan-only/action.yml': 'runs: x',
  '.gitea/actions/node-caches/action.yml': 'runs: x',
  '.gitea/actions/app-gates/action.yml': 'runs: y',
  '.gitea/actions/build-folder/action.yml': 'run: bash "${{ github.action_path }}/../../../conformance/ci/build-folder.sh"',
  '.gitea/actions/install-test-browser/action.yml': 'run: node "${{ github.action_path }}/../../../conformance/ci/install-test-browser.mjs"',
  '.gitea/actions/README.md': '- uses: decryp7/paddock/.gitea/actions/node-caches@v0.3.0 and .gitea/actions/unused',
  '.gitea/actions/unused/action.yml': 'runs: u',
  '.gitea/actions/only-commented/action.yml': 'runs: c',
  'conformance/ci/install-test-browser.mjs': "import { entryFor } from './runtime-catalogue.mjs';\nimport { isMain } from '../src/util/is-main.mjs';",
  'conformance/ci/runtime-catalogue.mjs': 'export const entryFor = 1;',
  'conformance/src/util/is-main.mjs': 'export const isMain = 1;',
  'conformance/ci/unrelated.mjs': 'export const x = 1;',
  '.gitea/actions/nested/action.yml': 'steps:\n  - uses: ./.paddock/.gitea/actions/build-folder',
};
const BASE_TREE = {
  '.gitea/workflows/ci.yml': CI(),
  '.gitea/workflows/dyno.yml': 'jobs:\n  gates:\n    steps:\n      - uses: ./.gitea/actions/node-caches\n',
  '.gitea/workflows/nightly.yml': 'jobs:\n  night:\n    steps:\n      - uses: ./.gitea/actions/nightly-only\n',
  '.gitea/actions/nightly-only/action.yml': 'runs: n',
  ...ACTIONS,
};
const runGitea = (files, gitea) => affected(files, graph, { gitea });
const CARGO_APPS = APPS.filter((a) => graph.apps.get(a).cargo);
const CI_FILE = '.gitea/workflows/ci.yml';

describe('.gitea changes are narrowed to the jobs they reach', () => {
  test('a change to the planner and its tests marks tools, ui and paddock-dev (its golden test), never an app', () => {
    for (const f of ['scripts/affected.test.mjs', 'scripts/affected.mjs']) {
      const r = run(f);
      assert.deepEqual([r.all, r.apps, r.crates, r.tools, r.ui, r.paddockDev], [false, [], false, true, true, true], f);
    }
  });

  test('a ci.yml change to one job marks that job only; a change to the app job marks the app legs', () => {
    const r = runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: CI('node a --only C06') }));
    assert.deepEqual([r.all, r.conformance, r.crates, r.ui, r.tools, r.apps], [false, true, false, false, false, []]);
    const a = runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: CI('node a', 'node c') }));
    assert.deepEqual([a.all, a.crates, a.apps], [false, false, APPS]);
  });

  test('a change to the plan job marks everything: it wires every other job', () => {
    const r = runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: CI().replace('plan-only', 'plan-only2') }));
    assert.ok(r.all && r.crates && r.ui);
    assert.deepEqual(r.apps, APPS);
  });

  test('a ci.yml change outside the jobs (the header), an unknown job or text that is not a workflow marks everything', () => {
    const hdr = CI().replace('  push:', '  pull_request:');
    assert.ok(runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: hdr })).all);
    assert.ok(runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: `${CI()}  brand-new:\n    steps: []\n` })).all);
    assert.ok(runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: 'this: [is, not\n  a workflow' })).all);
    // No way to read the revisions at all (no gitea reader): everything.
    assert.ok(affected([CI_FILE], graph).all);
    assert.equal(changedJobs(CI(), 'jobs:\nx: 1'), null);
    // Identical revisions (a change nobody can see): everything.
    assert.ok(runGitea([CI_FILE], giteaOf(BASE_TREE, {})).all);
  });

  test('a job split that could hide a job is refused: a comment after a key, odd two-space lines, anchors and aliases', () => {
    const base = CI();
    // The key with a trailing comment starts its own job (the edit below must not be attributed to the job before it).
    const r = runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: base.replace('node a', 'node a2') }));
    assert.deepEqual([r.all, r.conformance, r.ui, r.crates], [false, true, false, false]);
    for (const bad of [`${base}  "quoted": {}\n`, `${base}  crates2: { runs-on: x }\n`, `${base}anchors:\n  x: &rs 1\n`, base.replace('node b', 'node *rs'), base.replace('steps:\n      - uses: ./.gitea/actions/build-folder', 'steps: &rs\n      - uses: ./.gitea/actions/build-folder')]) {
      assert.equal(changedJobs(base, bad), null, bad);
      assert.ok(runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: bad })).all, bad);
    }
    assert.deepEqual(changedJobs(base, base.replace('  conformance: # the checks', '  conformance: # checks')), []); // only a comment on a key line
  });

  test('an action change marks only the ci.yml jobs and app workflows that use it, through nested actions and a .paddock prefix', () => {
    const g = giteaOf(BASE_TREE, {});
    const ui = runGitea(['.gitea/actions/node-caches/action.yml'], g);
    assert.deepEqual([ui.all, ui.ui, ui.crates, ui.apps], [false, true, false, ['dyno']]);
    const gates = runGitea(['.gitea/actions/app-gates/action.yml'], g);
    assert.deepEqual([gates.all, gates.crates, gates.apps], [false, false, APPS]);
    const bf = runGitea(['.gitea/actions/build-folder/action.yml'], g);
    assert.deepEqual([bf.all, bf.crates, bf.ui, bf.apps], [false, true, false, []]);
    // An action nobody uses cannot be traced (external consumers are possible): everything.
    assert.ok(runGitea(['.gitea/actions/nested/action.yml'], g).all);
    // A README that mentions an action is no use of it: the unused action stays unused (everything), node-caches keeps its users.
    assert.ok(runGitea(['.gitea/actions/unused/action.yml'], g).all);
    const nc = runGitea(['.gitea/actions/node-caches/action.yml'], g);
    assert.deepEqual([nc.all, nc.ui], [false, true]);
    const viaPrefix = runGitea(['.gitea/actions/build-folder/action.yml'], giteaOf(BASE_TREE, {}));
    assert.ok(viaPrefix.crates);
    // An action only the nightly runs is reported as such and marks tools.
    const night = affected(['.gitea/actions/nightly-only/action.yml'], graph, { gitea: g, explain: true });
    assert.deepEqual([night.all, night.tools, night.apps], [false, true, []]);
    assert.ok(night.why.tools.some((w) => /not a pull request/.test(w)));
  });

  test('a script of conformance/ci or conformance/release also marks the jobs of the actions that run it', () => {
    const g = giteaOf(BASE_TREE, {});
    const r = runGitea(['conformance/ci/build-folder.sh'], g);
    assert.deepEqual([r.all, r.crates, r.conformance, r.ui, r.eslint, r.apps], [false, true, true, false, false, APPS]);
    const other = runGitea(['conformance/release/latest-json.mjs'], g);
    assert.deepEqual([other.all, other.crates, other.ui], [false, false, false]);
    assert.deepEqual(other.apps, APPS);
    // Without a reader for the .gitea files the script cannot be traced: everything.
    assert.ok(affected(['conformance/ci/build-folder.sh'], graph).all);
  });

  test('a script is traced through the scripts that import it, transitively (helpers in conformance/src/util too)', () => {
    const g = giteaOf(BASE_TREE, {});
    for (const f of ['conformance/ci/runtime-catalogue.mjs', 'conformance/src/util/is-main.mjs', 'conformance/ci/install-test-browser.mjs']) {
      const r = runGitea([f], g);
      assert.deepEqual([r.all, r.crates, r.ui], [false, true, false], f);
    }
    assert.deepEqual(runGitea(['conformance/ci/unrelated.mjs'], g).crates, false);
  });

  test('importer tracing is transitive: a imports b imports c, a change to c reaches the action that runs a', () => {
    const tree = {
      ...BASE_TREE,
      '.gitea/actions/chain/action.yml': 'run: node conformance/ci/chain-a.mjs',
      '.gitea/workflows/ci.yml': CI().replace('install-test-browser', 'chain'),
      'conformance/ci/chain-a.mjs': "import './chain-b.mjs';",
      'conformance/ci/chain-b.mjs': "import { c } from './chain-c.mjs';",
      'conformance/ci/chain-c.mjs': 'export const c = 1;',
    };
    const g = giteaOf(tree, {});
    assert.deepEqual(scriptImporters('conformance/ci/chain-c.mjs', g).sort(), ['conformance/ci/chain-a.mjs', 'conformance/ci/chain-b.mjs', 'conformance/ci/chain-c.mjs']);
    const r = runGitea(['conformance/ci/chain-c.mjs'], g);
    assert.deepEqual([r.all, r.crates], [false, true]);
  });

  test('a mention of an action only in a comment of ci.yml is no use: everything', () => {
    const tree = { ...BASE_TREE, '.gitea/workflows/ci.yml': `# see ./.gitea/actions/only-commented\n${CI()}  # ./.gitea/actions/only-commented\n` };
    assert.ok(runGitea(['.gitea/actions/only-commented/action.yml'], giteaOf(tree, {})).all);
  });

  test('a reusable workflow (app-e2e.yml) that uses a changed action is followed to its callers, here an app workflow', () => {
    const tree = {
      ...BASE_TREE,
      '.gitea/workflows/app-e2e.yml': 'on:\n  workflow_call:\njobs:\n  e2e:\n    steps:\n      - uses: ./.gitea/actions/unused\n',
      '.gitea/workflows/dyno-release.yml': 'jobs:\n  e2e:\n    uses: ./.gitea/workflows/app-e2e.yml\n',
    };
    const r = runGitea(['.gitea/actions/unused/action.yml'], giteaOf(tree, {}));
    assert.deepEqual([r.all, r.apps, r.crates], [false, ['dyno'], false]);
    // The same workflow with no caller cannot be traced: everything.
    const orphan = { ...tree, '.gitea/workflows/dyno-release.yml': 'jobs: {}\n' };
    assert.ok(runGitea(['.gitea/actions/unused/action.yml'], giteaOf(orphan, {})).all);
  });

  test('in files mode a .gitea or script file that is not dirty (committed, nothing to diff) marks everything', () => {
    const dirty = new Set([CI_FILE]);
    const r = runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: CI('node a --only C06') }, (f) => dirty.has(f)));
    assert.equal(r.all, false);
    assert.ok(runGitea([CI_FILE], giteaOf(BASE_TREE, { [CI_FILE]: CI('node a --only C06') }, () => false)).all);
    assert.ok(runGitea(['.gitea/actions/node-caches/action.yml'], giteaOf(BASE_TREE, {}, () => false)).all);
    assert.ok(runGitea(['conformance/ci/build-folder.sh'], giteaOf(BASE_TREE, {}, () => false)).all);
  });

  test('the toolchain marks everything; the shared Rust inputs (.cargo, lint) only the Rust builds, lint also conformance', () => {
    const t = run('rust-toolchain.toml');
    assert.ok(t.all && t.ui && t.crates);
    assert.deepEqual(t.apps, APPS);
    for (const f of ['.cargo/config.toml', 'lint/clippy.toml']) {
      const r = run(f);
      assert.deepEqual([r.all, r.ui, r.crates, r.paddockDev, r.conformance], [false, false, true, true, f.startsWith('lint/')], f);
      assert.deepEqual(r.apps, CARGO_APPS, f);
      assert.ok(CARGO_APPS.every((a) => r.kinds[a].join() === 'rust'), f);
    }
  });

  test('ci.yml wires the plan job outputs: every needs.plan.outputs.X is declared, and every declared output is one --ci writes', () => {
    const ci = readFileSync(join(ROOT, CI_FILE), 'utf8');
    const planJob = workflowJobs(ci).jobs.get('plan');
    const declared = [...planJob.matchAll(/^ {6}([a-z_]+): \$\{\{ steps\.plan\.outputs\.([a-z_]+) \}\}/gm)];
    const written = Object.keys(ciOutputs({ paddockDev: false }, []));
    for (const [, name, step] of declared) {
      assert.equal(name, step, `output ${name} reads steps.plan.outputs.${step}`);
      assert.ok(written.includes(step), `--ci writes ${step}`);
    }
    const names = declared.map((d) => d[1]);
    for (const [, used] of ci.matchAll(/needs\.plan\.outputs\.([a-z_]+)/g)) assert.ok(names.includes(used), `ci.yml uses needs.plan.outputs.${used}, which the plan job does not declare`);
    for (const w of written) assert.ok(names.includes(w), `--ci writes ${w}, which the plan job does not declare`);
    assert.equal(ciOutputs({}, []).any_app, false);
    assert.equal(ciOutputs({}, [{ app: 'x' }]).any_app, true);
    assert.equal(ciOutputs({}, [{ app: 'x' }]).apps, '[{"app":"x"}]');
    assert.equal(ciOutputs({}, [], 'abc123').base, 'abc123');
    assert.equal(ciOutputs({}, []).base, '', 'no base: the commit lint checks the head commit only');
  });

  test('commitLintRange lints only what a change adds, and skips tags and app gates', () => {
    const Z = '0'.repeat(40);
    assert.equal(commitLintRange({ event: 'pull_request', baseRef: 'main' }), 'origin/main..HEAD');
    assert.equal(commitLintRange({ event: 'pull_request' }), null);
    assert.equal(commitLintRange({ event: 'push', ref: 'refs/heads/main', before: 'aaa', sha: 'bbb' }), 'aaa..bbb');
    assert.equal(commitLintRange({ event: 'push', ref: 'refs/heads/x', before: Z, sha: 'bbb' }), 'bbb^!', 'a new branch: the head commit');
    assert.equal(commitLintRange({ event: 'push', ref: 'refs/heads/x', before: '', sha: 'bbb' }), 'bbb^!');
    assert.equal(commitLintRange({ event: 'push', ref: 'refs/heads/main', before: 'gone', sha: 'bbb', known: () => false }), 'bbb^!', 'a force push: before is not in the clone');
    assert.equal(commitLintRange({ event: 'workflow_dispatch', base: 'ccc', sha: 'bbb' }), 'ccc..bbb');
    assert.equal(commitLintRange({ event: 'workflow_dispatch', sha: 'bbb' }), 'bbb^!');
    assert.equal(commitLintRange({ event: 'push', ref: 'refs/tags/v1.0.0', before: Z, sha: 'bbb' }), null, 'a tag');
    assert.equal(commitLintRange({ event: 'push', ref: 'refs/tags/dyno-v1.0.0', before: 'aaa', sha: 'bbb' }), null);
    assert.equal(commitLintRange({ event: 'push', ref: 'refs/heads/main', before: 'aaa', sha: 'bbb', app: 'dyno' }), null, 'an app tag call');
    assert.equal(commitLintRange({ event: 'schedule', sha: 'bbb' }), null);
  });

  test('ci.yml has a commits job that asks commitLintRange for its range', () => {
    const job = workflowJobs(readFileSync(join(ROOT, CI_FILE), 'utf8')).jobs.get('commits');
    assert.ok(job, 'a commits job');
    assert.match(job, /needs: \[plan\]/);
    assert.match(job, /node scripts\/affected\.mjs --commit-range/);
    assert.match(job, /fetch-depth: 0/);
    for (const v of ['EVENT', 'REF', 'BEFORE', 'SHA', 'BASE_REF', 'PLAN_BASE', 'INPUT_APP']) assert.match(job, new RegExp(`${v}: \\$\\{\\{`), v);
  });
});

describe('planFor reads the .gitea revisions from git', () => {
  const sh = (cwd, ...a) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...a], { cwd, encoding: 'utf8' });
  const scratch = () => {
    const dir = realpathSync(mkdtempSync(join(tmpdir(), 'affected-')));
    sh(dir, 'init', '-q', '-b', 'main');
    mkdirSync(join(dir, '.gitea/workflows'), { recursive: true });
    writeFileSync(join(dir, CI_FILE), CI());
    sh(dir, 'add', '-A');
    sh(dir, 'commit', '-q', '-m', 'base');
    return dir;
  };

  test('base mode: a committed change to one job marks that job (before is the base, after the head)', () => {
    const dir = scratch();
    try {
      const base = sh(dir, 'rev-parse', 'HEAD').trim();
      writeFileSync(join(dir, CI_FILE), CI('node a --only C06'));
      sh(dir, 'commit', '-q', '-am', 'one job');
      const r = planFor({ base, head: 'HEAD', root: dir, explain: true }).plan;
      assert.deepEqual([r.all, r.conformance, r.crates, r.ui], [false, true, false, false]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test('base mode: a committed change to an imported helper marks the job of the action that runs the importing script', () => {
    const dir = scratch();
    try {
      const put = (f, text) => {
        mkdirSync(join(dir, f.split('/').slice(0, -1).join('/')), { recursive: true });
        writeFileSync(join(dir, f), text);
      };
      put('.gitea/actions/install-test-browser/action.yml', 'run: node "${{ github.action_path }}/../../../conformance/ci/install-test-browser.mjs"');
      put('conformance/ci/install-test-browser.mjs', "import { entryFor } from './runtime-catalogue.mjs';");
      put('conformance/ci/runtime-catalogue.mjs', 'export const entryFor = 1;');
      sh(dir, 'add', '-A');
      sh(dir, 'commit', '-q', '-m', 'script');
      const base = sh(dir, 'rev-parse', 'HEAD').trim();
      put('conformance/ci/runtime-catalogue.mjs', 'export const entryFor = 2;');
      sh(dir, 'commit', '-q', '-am', 'helper');
      const r = planFor({ base, head: 'HEAD', root: dir }).plan;
      assert.deepEqual([r.all, r.crates, r.ui], [false, true, false]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test('files mode: an uncommitted change is narrowed, a committed one (HEAD equals the working tree) marks everything', () => {
    const dir = scratch();
    try {
      writeFileSync(join(dir, CI_FILE), CI('node a --only C06'));
      const dirty = planFor({ files: [CI_FILE], root: dir }).plan;
      assert.deepEqual([dirty.all, dirty.conformance, dirty.crates], [false, true, false]);
      sh(dir, 'commit', '-q', '-am', 'one job');
      assert.ok(planFor({ files: [CI_FILE], root: dir }).plan.all);
    } finally {
      rmSync(dir, { recursive: true, force: true });
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
    assert.equal(leg.nextest_config, 'crates/.config/nextest-demos.toml');
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
    assert.ok(rust.every((l) => l.cargo === 'apps/pitcrew' && /fake_runner/.test(l.process_patterns) && !('processes' in l)), 'the Cargo folder is normalised and the test processes are named by patterns only');
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

  test('WebKit is wanted on main, tags, the nightly and a manual run that asks, never on a plain pull request or a branch push', () => {
    assert.equal(wantsWebkit({ GITHUB_EVENT_NAME: 'pull_request', GITHUB_REF: 'refs/pull/1/merge' }), false);
    assert.equal(wantsWebkit({ GITHUB_EVENT_NAME: 'workflow_dispatch', GITHUB_REF: 'refs/heads/topic' }), false);
    assert.equal(wantsWebkit({ GITHUB_EVENT_NAME: 'workflow_dispatch', GITHUB_REF: 'refs/heads/topic', PLAN_WEBKIT: '1' }), true);
    assert.equal(wantsWebkit({ GITHUB_EVENT_NAME: 'push', GITHUB_REF: 'refs/heads/main' }), true);
    assert.equal(wantsWebkit({ GITHUB_EVENT_NAME: 'push', GITHUB_REF: 'refs/tags/spotter-v1.0.0' }), true);
    assert.equal(wantsWebkit({ GITHUB_EVENT_NAME: 'schedule', GITHUB_REF: 'refs/heads/main' }), true);
  });

  test("`e2eWebkit` is a leg of its own when WebKit is wanted and the change reaches the UI; every e2e leg has its own name", () => {
    const apps = new Map([['x', { dir: 'apps/x', cargo: '.', ui: ['npm test'], e2e: ['pw'], e2eShards: 2, e2eWebkit: ['pw --project=webkit'] }]]);
    const plan = { apps: ['x'], kinds: { x: ['ui'] }, packages: {} };
    const withWebkit = matrix(plan, { cargo: new Map(), apps }, { webkit: true });
    assert.deepEqual(withWebkit.map((l) => [l.e2e_leg, l.e2e_shard, l.e2e_checks, l.ui_checks]), [['', '', '', 'npm test'], ['e2e-1', '1/2', 'pw', ''], ['e2e-2', '2/2', 'pw', ''], ['e2e-webkit', '', 'pw --project=webkit', '']]);
    assert.equal(new Set(withWebkit.map((l) => l.e2e_leg)).size, withWebkit.length, 'the Angular cache name per leg is unique');
    assert.equal(matrix(plan, { cargo: new Map(), apps }).length, 3, 'no WebKit leg unless asked');
    assert.equal(matrix({ ...plan, kinds: { x: ['rust'] } }, { cargo: new Map(), apps }, { webkit: true }).length, 1, 'nor for a change that does not reach the UI');
    const spotter = graph.apps.get('spotter');
    assert.equal(spotter.e2eShards, 2);
    assert.match(spotter.e2e.join('\n'), /--shard=\$E2E_SHARD/);
    assert.match(spotter.e2eWebkit.join('\n'), /--project=webkit/);
    assert.ok(!spotter.ui.some((l) => /webkit/.test(l)), 'WebKit does not lengthen the main leg');
  });

  test("an app's host-cleanup patterns match its own paths and never the real runner or another job (every app that has them)", () => {
    const real = ['gitea-runner daemon', '/usr/local/bin/gitea-runner daemon', '/Users/x/runners/main/bin/gitea-runner daemon', '/opt/act_runner/act_runner daemon', 'C:\\runner\\act_runner.exe daemon', '/Users/x/actions-runner/Runner.Listener run'];
    const t = JOB.tmp;
    const other = '/Users/x/gitea-runner/work/fedcba9876543210/tmp';
    const ours = {
      dyno: ['/Users/x/.paddock-ci/target-dyno-app-s1/debug/dyno-test-plugin echo', 'C:\\Users\\x\\.paddock-ci\\target-dyno-app-s1\\debug\\dyno-test-plugin.exe', `${t}/dyno-it-plugins-Xk3a9Q/p1/plugin`],
      pitwall: ['/Users/x/.paddock-ci/target-pitwall-app-s1/debug/pitwall-server --die-after-ms 500 --port 4455 --fake-engine'],
      pitcrew: [`${t}/pitcrew-it-stop-4242/runner/bin/gitea-runner daemon`, 'C:\\Users\\x\\gitea-runner\\work\\0123456789abcdef\\tmp\\pitcrew-it-stop-7\\runner\\bin\\gitea-runner.exe daemon', '/Users/x/.paddock-ci/target-pitcrew-app-s1/debug/examples/fake_runner --exit-after 5'],
      spotter: ['/Users/x/.paddock-ci/test-browser-spotter-s1/chrome-mac/Google Chrome for Testing --headless', 'C:\\Users\\x\\.paddock-ci\\test-browser-spotter-s1\\chrome-win64\\chrome.exe --headless'],
    };
    // Another job on the same host: the next slot's kept folders, another runner folder's temp.
    const otherJob = {
      dyno: ['/Users/x/.paddock-ci/target-dyno-app-s2/debug/dyno-test-plugin echo', `${other}/dyno-it-plugins-Xk3a9Q/p1/plugin`],
      pitwall: ['/Users/x/.paddock-ci/target-pitwall-app-s2/debug/pitwall-server --fake-engine'],
      pitcrew: [`${other}/pitcrew-it-stop-4242/runner/bin/gitea-runner daemon`, '/Users/x/.paddock-ci/target-pitcrew-app-s2/debug/examples/fake_runner'],
      spotter: ['/Users/x/.paddock-ci/test-browser-spotter-s2/chrome-mac/Google Chrome for Testing', '/Users/x/.paddock-ci/test-browser-spotter-s12/chrome-mac/Google Chrome for Testing', 'C:\\Users\\x\\.paddock-ci\\test-browser-spotter-s2\\chrome.exe'],
    };
    let checked = 0;
    for (const [name, app] of graph.apps) {
      if (!app.processPatterns?.length) continue;
      assert.ok(ours[name] && otherJob[name], `${name} has process patterns: list its own and another job's command lines here`);
      const res = scopedAll(name, app.processPatterns);
      assert.ok(res.every(Boolean), `${name}: every pattern is anchored on a folder of the job ({tmp}, {build} or {slot})`);
      for (const line of ours[name]) assert.ok(res.some((re) => re.test(line)), `${name}: no pattern matches ${line}`);
      for (const re of res) {
        for (const line of [...real, ...otherJob[name]]) assert.doesNotMatch(line, re, `${name}: ${re} would kill ${line}`);
        checked += 1;
      }
    }
    assert.ok(checked >= 2);
  });

  test('job-isolation refuses a process pattern that names no folder of the job, or one the job does not have', () => {
    assert.equal(scoped('[/\\\\]pitcrew-it-[^/]+[/\\\\]gitea-runner', 'pitcrew'), null, 'a temp prefix alone matches every job');
    assert.equal(scoped('fake_runner', 'pitcrew'), null, 'an exact name matches every job');
    const r = spawnSync('bash', [join(ROOT, 'conformance', 'ci', 'job-isolation.sh'), 'pattern', '{tmp}/x'], { env: { PATH: '/usr/bin:/bin', RUNNER_OS: 'macOS' }, encoding: 'utf8' });
    assert.notEqual(r.status, 0, 'no job temp folder: nothing to anchor on');
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

  // Regression (CI 7277): the demos ran the crates' nextest config, whose override names paddock-chromium, a package the demo
  // workspaces do not have; nextest refuses a filter that matches no package, so both demo legs failed before running a test.
  test("a leg's nextest config names only packages of that leg's Cargo workspace, in CI's leg and in the app's release workflow", () => {
    for (const d of readdirSync(join(ROOT, 'apps'), { withFileTypes: true }).filter((e) => e.isDirectory())) {
      const app = graph.apps.get(d.name);
      if (!app?.nextestConfig || !app.cargo || !tracked.some((f) => f.startsWith(`apps/${d.name}/`))) continue;
      const lock = readFileSync(join(ROOT, app.dir, app.cargo, 'Cargo.lock'), 'utf8');
      const members = new Set([...lock.matchAll(/^name = "([^"]+)"$/gm)].map((m) => m[1]));
      const config = readFileSync(join(ROOT, app.nextestConfig), 'utf8');
      for (const [, name] of config.matchAll(/package\(([^)]+)\)/g)) {
        assert.ok(members.has(name), `${d.name}: ${app.nextestConfig} names package ${name}, which its Cargo workspace does not have`);
      }
      const release = join(ROOT, '.gitea', 'workflows', `${d.name}-release.yml`);
      if (existsSync(release)) {
        const used = [...readFileSync(release, 'utf8').matchAll(/--config-file (\S+nextest[^\s]*\.toml)/g)].map((m) => m[1].replace(/^(\.\.\/)+/, ''));
        for (const file of used) assert.equal(file, app.nextestConfig, `${d.name}-release.yml runs a different nextest config than its CI leg`);
      }
    }
  });

  test('rust checks reach only the Linux leg, and run only when the change reaches the Rust half', () => {
    const [leg] = matrix(run('apps/pitwall/crates/pitwall-core/src/lib.rs'), graph).filter((l) => l.os === LINUX);
    assert.match(leg.rust_checks, /cargo llvm-cov nextest -p pitwall-core/);
    assert.equal(leg.rust, true);
    for (const l of matrix(run('apps/pitwall/crates/pitwall-core/src/lib.rs'), graph).filter((l) => l.os !== LINUX)) assert.equal(l.rust_checks, '', 'host legs run the Rust gates only');
    assert.match(readFileSync(join(ROOT, '.gitea/actions/app-gates/action.yml'), 'utf8'), /if: \$\{\{ !cancelled\(\) && steps\.prepare\.outcome != 'failure' && inputs\.rust-checks != '' && inputs\.rust == 'true' \}\}/, 'the action skips them with the Rust half');
  });

  test('windowsClippy is opt-in: Pitwall lints its Windows-only code, an app without the field keeps the default', () => {
    const win = (app) => matrix(run(`apps/${app}/crates/${app}-core/src/lib.rs`), graph).filter((l) => l.os === 'windows-latest').map((l) => l.windows_clippy);
    assert.deepEqual([...new Set(win('pitwall'))], ['true']);
    assert.deepEqual([...new Set(win('pitcrew'))], ['false']);
    assert.match(readFileSync(join(ROOT, '.gitea/actions/app-gates/action.yml'), 'utf8'), /"\$RUNNER_OS" != Windows \] \|\| \[ "\$WINDOWS_CLIPPY" = true \]; then step cargo clippy/);
  });

  test('notices: an app with a `notices` block checks its THIRD_PARTY_NOTICES.md on the Linux leg that runs Rust or UI, and only there', () => {
    for (const [name, app] of graph.apps) {
      if (!app.notices) continue;
      assert.ok(app.notices.name && Array.isArray(app.notices.manifests), `${name}: notices needs a name and its Cargo manifests`);
    }
    const pitwall = graph.apps.get('pitwall');
    const legsOf = () => matrix(run('apps/pitwall/crates/pitwall-core/src/lib.rs'), graph).filter((l) => l.app === 'pitwall');
    assert.ok(legsOf().every((l) => l.notices === 'false'), 'no block, no check');
    pitwall.notices = { name: 'Pitwall', manifests: [] };
    try {
      const legs = legsOf();
      assert.ok(legs.length > 1, 'Pitwall has host legs too');
      for (const l of legs) assert.equal(l.notices, l.os === LINUX ? 'true' : 'false', l.os);
    } finally {
      delete pitwall.notices;
    }
    assert.match(readFileSync(join(ROOT, '.gitea/actions/app-gates/action.yml'), 'utf8'), /scripts\/notices\.sh" "\$APP" --check/, 'app-gates runs the shared check');
  });

  test('notices: the check runs on exactly one leg of an app, not on its e2e shards or the WebKit leg; the tool and its config are inputs', () => {
    const dyno = graph.apps.get('dyno');
    const had = dyno.notices;
    dyno.notices = { name: 'Dyno', manifests: [] };
    try {
      const legs = matrix(run('apps/dyno/app/src/app/shell/about.providers.ts'), graph, { webkit: true }).filter((l) => l.app === 'dyno');
      assert.ok(legs.length > 2, 'the UI change gives the main leg, the e2e shards and more');
      assert.equal(legs.filter((l) => l.notices === 'true').length, 1);
      assert.ok(legs.filter((l) => l.e2e_leg).every((l) => l.notices === 'false'));
      for (const f of ['scripts/notices.sh', 'templates/repo/about.toml', 'conformance/src/notices/generate.mjs']) {
        const hit = matrix(run(f), graph).filter((l) => l.app === 'dyno' && l.notices === 'true');
        assert.equal(hit.length, 1, `${f} re-checks the notices of an app with the block`);
      }
    } finally {
      if (had) dyno.notices = had;
      else delete dyno.notices;
    }
  });

  test("an app's last packages reach every leg, so a host leg and the Linux leg agree on what is tested apart", () => {
    const legs = matrix(run('apps/pitwall/crates/pitwall-core/src/lib.rs'), graph);
    assert.deepEqual([...new Set(legs.map((l) => l.last_packages))], ['pitwall-app'], 'Pitwall tests its Tauri shell after the rest');
    assert.ok(matrix(run('apps/pitcrew/crates/pitcrew-core/src/lib.rs'), graph).every((l) => l.last_packages === ''), 'an app without the field tests its workspace in one run');
  });

  test("an app's process patterns (host-cleanup `pkill -f`) name its own temp folder and never match the real runner", () => {
    const legs = matrix(run('apps/pitcrew/crates/pitcrew-core/src/lib.rs'), graph);
    const patterns = graph.apps.get('pitcrew').processPatterns;
    assert.ok(patterns?.length, 'Pitcrew names the fake runner copy the tests start');
    assert.ok(legs.every((l) => l.process_patterns === patterns.join('\n')), 'every leg hands the patterns to host-cleanup');
    const t = JOB.tmp;
    const ours = [`${t}/pitcrew-it-stop-4242/runner/bin/gitea-runner daemon`, 'C:\\Users\\x\\gitea-runner\\work\\0123456789abcdef\\tmp\\pitcrew-it-stop-7\\runner\\bin\\gitea-runner.exe daemon'];
    const real = ['gitea-runner daemon', '/usr/local/bin/gitea-runner daemon', '/Users/x/runners/main/bin/gitea-runner daemon', '/opt/act_runner/act_runner daemon', '/Users/x/pitcrew/runner/bin/gitea-runner daemon', '/tmp/pitcrew-it/gitea-runner', '/tmp/other-1/runner/bin/gitea-runner', '/var/folders/ab/T/pitcrew-it-stop-4242/runner/bin/gitea-runner daemon'];
    const res = scopedAll('pitcrew', patterns);
    for (const line of ours) assert.ok(res.some((re) => re.test(line)), `no pattern matches the test copy ${line}`);
    for (const re of res) for (const line of real) assert.doesNotMatch(line, re, `${re} would kill ${line}`);
  });

  test("every process pattern is anchored on a folder of this job, and Pitwall's never reaches an installed app, another job or the user's own engine", () => {
    for (const [name, app] of graph.apps) {
      assert.equal(app.processes, undefined, `${name}: no exact process names (paddock.ci.json \`processes\`): they match every job's processes`);
      for (const pat of app.processPatterns ?? []) assert.match(pat, /\{(tmp|build|slot)\}/, `${name}: ${pat} names no folder of the job ({tmp}, {build}, {slot})`);
    }
    const pitwall = graph.apps.get('pitwall');
    const ours = ['/Users/x/.paddock-ci/target-pitwall-app-s1/debug/pitwall-server --die-after-ms 500 --port 4455 --fake-engine'];
    const near = [
      '/Applications/Pitwall.app/Contents/MacOS/pitwall-server --port 8080',
      '/Applications/Pitwall.app/Contents/MacOS/pitwall-server --fake-engine',
      '/Users/x/.paddock-ci/target-spotter-app-s1/debug/pitwall-server --fake-engine',
      '/Users/x/.paddock-ci/target-pitwall-bundle-s1/debug/pitwall-server --fake-engine',
      '/Users/x/.paddock-ci/target-pitwall-app-s2/debug/pitwall-server --fake-engine',
      '/Users/x/.paddock/engines/llama-server --port 8081 -m model.gguf',
      '/Users/x/dev/pitwall/target/debug/pitwall-server --fake-engine',
      '/opt/homebrew/bin/llama-server --port 8080',
    ];
    for (const re of scopedAll('pitwall', pitwall.processPatterns)) {
      for (const line of ours) assert.match(line, re, `${re} misses ${line}`);
      for (const line of near) assert.doesNotMatch(line, re, `${re} would kill ${line}`);
    }
  });

  test("Dyno's process pattern ends only test programs of its own build folder: never the installed app, its agent, another job's folder or the user's tools", () => {
    const dyno = graph.apps.get('dyno');
    const t = JOB.tmp;
    const ours = ['/Users/x/.paddock-ci/target-dyno-app-s1/debug/dyno-test-plugin echo', '/Users/x/.paddock-ci/target-dyno-app-s1/debug/dyno-bench cpu --millis 20', 'C:\\Users\\x\\.paddock-ci\\target-dyno-app-s1\\debug\\dyno-test-plugin.exe', `${t}/dyno-it-plugins-Xk3a9Q/p1/plugin`, 'C:\\Users\\x\\gitea-runner\\work\\0123456789abcdef\\tmp\\dyno-it-plugins-Xk3a9Q\\p1\\plugin.exe'];
    const near = [
      '/Users/x/Library/Application Support/Dyno/plugins/p1/plugin',
      `${t}/other-it-plugins-Xk3a9Q/p1/plugin`,
      `${t}/dyno-it-plugins-Xk3a9Q/p1/plugin.toml`,
      '/var/folders/ab/T/dyno-it-plugins-Xk3a9Q/p1/plugin',
      '/Users/x/gitea-runner/work/fedcba9876543210/tmp/dyno-it-plugins-Xk3a9Q/p1/plugin',
      '/Applications/Dyno.app/Contents/MacOS/dyno-agent run',
      '/Applications/Dyno.app/Contents/MacOS/dyno-helper',
      '/Applications/Dyno.app/Contents/MacOS/dyno-bench',
      '/Users/x/.paddock-ci/target-dyno-bundle-s1/debug/dyno-test-plugin echo',
      '/Users/x/.paddock-ci/target-spotter-app-s1/debug/dyno-test-plugin echo',
      '/Users/x/.paddock-ci/target-dyno-app-s1/debug/dyno-agent run',
      '/Users/x/.paddock-ci/target-dyno-app-s2/debug/dyno-bench cpu',
      '/Users/x/.paddock-ci/target-dyno-app2/debug/dyno-bench cpu',
      '/Users/x/dev/dyno/target/debug/dyno-test-plugin echo',
      'C:\\Program Files\\Dyno\\dyno-agent.exe',
    ];
    const res = scopedAll('dyno', dyno.processPatterns);
    for (const line of ours) assert.ok(res.some((re) => re.test(line)), `no pattern matches ${line}`);
    for (const re of res) for (const line of near) assert.doesNotMatch(line, re, `${re} would kill ${line}`);
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


describe('command line', () => {
  test('`<base>` alone and `<base> <head>` keep the base; the --files value and flags are not positional', () => {
    assert.deepEqual(parseArgs(['abc']), { files: null, base: 'abc', head: 'HEAD', all: false, ci: false, app: null, explain: false });
    assert.equal(parseArgs(['abc', 'def', '--explain']).base, 'abc');
    assert.equal(parseArgs(['abc', 'def', '--explain']).head, 'def');
    assert.equal(parseArgs(['--explain', 'abc']).base, 'abc');
    const f = parseArgs(['--files', 'a,b', '--explain']);
    assert.deepEqual([f.files, f.base], [['a', 'b'], null]);
    assert.equal(parseArgs(['--all']).base, null);
  });
});

describe('command line: incomplete or unknown arguments are refused, never planned as nothing', () => {
  const refused = {
    '--files without a value': ['--files'],
    '--files followed by a flag': ['--files', '--explain'],
    'three positionals': ['a', 'b', 'c'],
    'an unknown flag': ['a', '--frobnicate'],
    '--app without --ci': ['a', '--app', 'pitwall'],
    '--ci --app without a value': ['--ci', '--app'],
    '--files with a base': ['--files', 'a,b', 'origin/main'],
    '--all with --files': ['--all', '--files', 'a'],
    '--all with a base': ['--all', 'origin/main'],
    '--ci with a positional': ['--ci', 'origin/main'],
    '--ci with --files': ['--ci', '--files', 'a'],
    '--ci with --all': ['--ci', '--all'],
    '--app with an empty value': ['--ci', '--app', ''],
    '--files with an empty value': ['--files', ''],
    '--files with an empty list': ['--files', ','],
    'a repeated --app': ['--ci', '--app', 'x', '--app', 'y'],
    'a repeated --explain': ['a', '--explain', '--explain'],
  };
  for (const [name, args] of Object.entries(refused)) {
    test(`${name} throws`, () => assert.throws(() => parseArgs(args), /./));
  }
  test('every documented form parses', () => {
    assert.equal(parseArgs(['--ci']).ci, true);
    const a = parseArgs(['--ci', '--app', 'pitwall']);
    assert.deepEqual([a.ci, a.app, a.base], [true, 'pitwall', null]);
    assert.equal(parseArgs(['origin/main', '--explain']).explain, true);
    assert.deepEqual(parseArgs(['--files', 'a,b', '--explain']).files, ['a', 'b']);
    assert.equal(parseArgs(['--all']).all, true);
  });
});

describe('paths from cargo metadata on Windows', () => {
  test('repoRelative gives a / path where path.relative would give a \\ one', () => {
    const win = (root, p) => repoRelative(root, p, win32.relative, win32.sep);
    assert.equal(win('C:\\work\\paddock', 'C:\\work\\paddock\\crates\\paddock-cli'), 'crates/paddock-cli');
    assert.equal(win('C:\\work\\paddock', 'c:\\WORK\\paddock\\apps\\x\\app\\src-tauri'), 'apps/x/app/src-tauri', 'drive letter and case do not matter on Windows');
    assert.equal(win('C:\\work\\paddock', 'C:\\work\\paddock'), '');
    assert.equal(repoRelative('/r', '/r/crates/a'), 'crates/a');
  });
});
