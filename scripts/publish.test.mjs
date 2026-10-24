// Tests of scripts/publish.mjs (pure parts) and of the repository facts it relies on: `node --test scripts/`.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  cargoToken,
  crateProblems,
  crateStepsMissing,
  entryPoints,
  formatTable,
  indexPath,
  indexVersions,
  lockstepProblems,
  shellVersionProblems,
  mask,
  missingExports,
  npmPackages,
  parseArgs,
  planRows,
  publishOrder,
  tagProblems,
  workspaceCrates,
  WORKSPACE_HACK,
} from './publish.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const scratch = mkdtempSync(join(tmpdir(), 'paddock-publish-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

const crate = (name, deps = [], extra = {}) => ({ name, version: '0.1.0', publish: ['decryp7'], scaffold: false, deps, ...extra });

describe('parseArgs', () => {
  test('dry run is the default', () => assert.deepEqual(parseArgs([]), { publish: false, skipBuild: false, offline: false }));
  test('flags', () => assert.deepEqual(parseArgs(['--skip-build', '--offline']), { publish: false, skipBuild: true, offline: true }));
  test('unknown flag fails', () => assert.throws(() => parseArgs(['--dry-run']), /unknown option --dry-run/));
  test('publish cannot skip the build or the registries', () => {
    assert.throws(() => parseArgs(['--publish', '--skip-build']), /dry run/);
    assert.throws(() => parseArgs(['--publish', '--offline']), /dry run/);
  });
});

describe('indexPath', () => {
  test('cargo index layout', () => {
    assert.equal(indexPath('a'), '1/a');
    assert.equal(indexPath('ab'), '2/ab');
    assert.equal(indexPath('abc'), '3/a/abc');
    assert.equal(indexPath('Paddock-Jobs'), 'pa/dd/paddock-jobs');
  });
  test('empty name fails', () => assert.throws(() => indexPath(''), /empty/));
});

describe('workspaceCrates', () => {
  const pkg = (id, name, deps, metadata = null) => ({
    id,
    name,
    version: '0.1.0',
    publish: ['decryp7'],
    metadata,
    manifest_path: `/w/${name}/Cargo.toml`,
    dependencies: deps,
  });
  test('workspace members only, sibling non-dev deps, scaffold marker', () => {
    const list = workspaceCrates({
      workspace_members: ['b', 'a'],
      packages: [
        pkg('b', 'paddock-b', [
          { name: 'paddock-a', kind: null },
          { name: 'paddock-a', kind: 'build' },
          { name: 'serde', kind: null },
        ]),
        pkg('a', 'paddock-a', [{ name: 'paddock-b', kind: 'dev' }], { paddock: { scaffold: true } }),
        pkg('x', 'outsider', []),
      ],
    });
    assert.deepEqual(
      list.map((c) => [c.name, c.deps, c.scaffold, c.dir]),
      [
        ['paddock-a', [], true, '/w/paddock-a'],
        ['paddock-b', ['paddock-a'], false, '/w/paddock-b'],
      ],
    );
  });
  test('the workspace-hack crate is neither a crate to publish nor a dependency', () => {
    const list = workspaceCrates({
      workspace_members: ['a', 'h'],
      packages: [pkg('a', 'paddock-a', [{ name: WORKSPACE_HACK, kind: null }]), pkg('h', WORKSPACE_HACK, [])],
    });
    assert.deepEqual(
      list.map((c) => [c.name, c.deps]),
      [['paddock-a', []]],
    );
  });
});

describe('crateProblems', () => {
  test('only the decryp7 registry or publish = false', () => {
    assert.deepEqual(crateProblems([crate('ok'), crate('off', [], { publish: [] })]), []);
    const problems = crateProblems([crate('io', [], { publish: null }), crate('two', [], { publish: ['decryp7', 'crates-io'] })]);
    assert.equal(problems.length, 2);
    assert.match(problems[0], /^io: set publish = \["decryp7"\]/);
  });
});

describe('publishOrder', () => {
  test('dependencies first, ties by name', () => {
    const order = publishOrder([crate('updater', ['download']), crate('download', ['jobs']), crate('jobs'), crate('health')]);
    assert.deepEqual(order.map((c) => c.name), ['health', 'jobs', 'download', 'updater']);
  });
  test('a cycle fails', () => assert.throws(() => publishOrder([crate('a', ['b']), crate('b', ['a'])]), /dependency cycle between a, b/));
});

describe('entry points', () => {
  test('every folder with ng-package.json below the library root, sorted', () => {
    const lib = join(scratch, 'lib');
    for (const d of ['shell/startup', 'shell', 'workbench/dock', 'src/lib', 'node_modules/x', '.cache/y']) {
      mkdirSync(join(lib, d), { recursive: true });
    }
    for (const d of ['', 'shell', 'shell/startup', 'workbench/dock', 'node_modules/x', '.cache/y']) {
      writeFileSync(join(lib, d, 'ng-package.json'), '{}');
    }
    assert.deepEqual(entryPoints(lib), ['shell', 'shell/startup', 'workbench/dock']);
  });
  test('missing exports are named', () => {
    assert.deepEqual(missingExports(['shell', 'shell/jobs'], { exports: { '.': {}, './shell': {} } }), ['shell/jobs']);
    assert.deepEqual(missingExports(['shell'], {}), ['shell']);
  });
  test('the real library: every entry point the plan names (ADR 0012 §4, ADR 0013)', () => {
    const entries = entryPoints(join(root, 'ui/projects/paddock-ui'));
    for (const e of ['shell', 'shell/startup', 'shell/notify', 'shell/dialogs', 'shell/jobs', 'shell/settings', 'shell/settings/window', 'shell/about', 'shell/menu',
      'shell/pages', 'shell/i18n', 'shell/store', 'shell/testing', 'workbench', 'workbench/dock', 'workbench/testing']) {
      assert.ok(entries.includes(e), `missing entry point ${e}`);
    }
  });
});

describe('npm packages', () => {
  test('public @decryp7 packages with their dist folder and scaffold marker', () => {
    const projects = join(scratch, 'projects');
    const write = (folder, pkg) => {
      mkdirSync(join(projects, folder), { recursive: true });
      writeFileSync(join(projects, folder, 'package.json'), JSON.stringify(pkg));
    };
    write('paddock-ui', { name: '@decryp7/paddock-ui', version: '0.2.0' });
    write('paddock-e2e', { name: '@decryp7/paddock-e2e', version: '0.2.0', paddock: { scaffold: true } });
    write('demo', { name: '@decryp7/demo', version: '0.0.0', private: true });
    write('other', { name: 'left-pad', version: '1.0.0' });
    mkdirSync(join(projects, 'gallery'), { recursive: true });
    assert.deepEqual(npmPackages(projects, '/d'), [
      { name: '@decryp7/paddock-e2e', version: '0.2.0', scaffold: true, dist: '/d/paddock-e2e' },
      { name: '@decryp7/paddock-ui', version: '0.2.0', scaffold: false, dist: '/d/paddock-ui' },
    ]);
  });
  test('lockstep with paddock-ui', () => {
    assert.deepEqual(lockstepProblems([{ name: '@decryp7/paddock-ui', version: '1.0.0' }, { name: '@decryp7/paddock-e2e', version: '1.0.0' }]), []);
    assert.match(lockstepProblems([{ name: '@decryp7/paddock-ui', version: '1.0.0' }, { name: '@decryp7/paddock-e2e', version: '0.9.0' }])[0], /carry/);
    assert.match(lockstepProblems([])[0], /missing/);
  });
  test('the real packages are in lockstep', () => {
    assert.deepEqual(lockstepProblems(npmPackages(join(root, 'ui/projects'), '/d')), []);
    // The shell plugin crate carries the npm package's version (ADR 0012 §3), checked here and by paddock-shell's tests.
    const ui = [{ name: '@decryp7/paddock-ui', version: '0.2.0' }];
    assert.deepEqual(shellVersionProblems([{ name: 'paddock-shell', version: '0.2.0' }], ui), []);
    assert.match(shellVersionProblems([{ name: 'paddock-shell', version: '0.1.0' }], ui)[0], /bump both together/);
    assert.deepEqual(shellVersionProblems([{ name: 'paddock-jobs', version: '0.1.0' }], ui), []);
  });
});

describe('tagProblems (rule 9)', () => {
  test('CI on the tag of this version passes', () => {
    assert.deepEqual(tagProblems({ CI: 'true', GITHUB_REF: 'refs/tags/v0.2.0' }, '0.2.0'), []);
    assert.deepEqual(tagProblems({ GITEA_ACTIONS: 'true', GITHUB_REF: 'refs/tags/v0.2.0' }, '0.2.0'), []);
  });
  test('a workstation, a branch or another version is refused', () => {
    assert.equal(tagProblems({ GITHUB_REF: 'refs/tags/v0.2.0' }, '0.2.0').length, 1);
    assert.equal(tagProblems({ CI: 'true', GITHUB_REF: 'refs/heads/main' }, '0.2.0').length, 1);
    assert.equal(tagProblems({ CI: 'true', GITHUB_REF: 'refs/tags/v0.1.0' }, '0.2.0').length, 1);
    assert.equal(tagProblems({}, '0.2.0').length, 2);
  });
});

describe('crateStepsMissing', () => {
  const yaml = (name) => `      - name: ${name}\n        run: cargo clippy -p ${name} --all-targets --all-features -- -D warnings && cargo nextest run -p ${name} --all-features --profile ci\n`;
  test('names crates without both steps', () => {
    const ci = yaml('paddock-a') + '        run: cargo clippy -p paddock-b --all-targets --all-features -- -D warnings\n';
    assert.deepEqual(crateStepsMissing(ci, ['paddock-a', 'paddock-b', 'paddock-c']), ['paddock-b', 'paddock-c']);
  });
  test('a prefix does not count for a longer name', () => {
    assert.deepEqual(crateStepsMissing(yaml('paddock-a'), ['paddock-ab']), ['paddock-ab']);
  });
  test('the real ci.yml has a step for every workspace member', () => {
    const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], { cwd: join(root, 'crates'), encoding: 'utf8' }));
    const names = workspaceCrates(metadata).map((c) => c.name);
    assert.ok(names.includes('paddock-jobs'));
    assert.deepEqual(crateStepsMissing(readFileSync(join(root, '.gitea/workflows/ci.yml'), 'utf8'), names), []);
    assert.deepEqual(crateProblems(workspaceCrates(metadata)), []);
  });
});

describe('planRows and formatTable', () => {
  const crates = [crate('jobs'), crate('menu', [], { scaffold: true }), crate('off', [], { publish: [] }), crate('health', [], { version: '0.2.0' })];
  const npm = [{ name: '@decryp7/paddock-ui', version: '0.2.0', scaffold: false }];
  test('decisions with registry answers', () => {
    const published = new Map([
      ['jobs', new Set(['0.1.0'])],
      ['health', new Set(['0.1.0'])],
      ['@decryp7/paddock-ui', new Set()],
    ]);
    const rows = planRows(crates, npm, published);
    assert.deepEqual(
      rows.map((r) => [r.kind, r.name, r.action, r.reason]),
      [
        ['crate', 'jobs', 'skip', 'already published'],
        ['crate', 'menu', 'skip', 'scaffold (held back until its work package lands)'],
        ['crate', 'off', 'skip', 'publish = false'],
        ['crate', 'health', 'publish', 'registry has 0.1.0'],
        ['npm', '@decryp7/paddock-ui', 'publish', 'new'],
      ],
    );
  });
  test('offline and unreachable', () => {
    assert.equal(planRows([crate('jobs')], [], null)[0].reason, 'registry not asked (--offline)');
    assert.equal(planRows([crate('jobs')], [], new Map())[0].reason, 'registry not reachable');
  });
  test('table is aligned', () => {
    const text = formatTable([{ kind: 'crate', name: 'a', version: '1', action: 'publish', reason: 'new' }]);
    assert.equal(text, 'kind   name  version  action   reason\ncrate  a     1        publish  new');
  });
});

describe('registry and secrets', () => {
  test('sparse index versions', () => {
    assert.deepEqual(indexVersions('{"name":"a","vers":"0.1.0"}\n{"name":"a","vers":"0.2.0"}\n'), new Set(['0.1.0', '0.2.0']));
    assert.deepEqual(indexVersions(''), new Set());
  });
  test('cargo token precedence', () => {
    assert.equal(cargoToken({ CARGO_REGISTRIES_DECRYP7_TOKEN: 'Bearer x', PACKAGE_AUTH: 'y' }), 'Bearer x');
    assert.equal(cargoToken({ PACKAGE_AUTH: 'dXNlcjpwdw==', PACKAGE_TOKEN: 't' }), 'Basic dXNlcjpwdw==');
    assert.equal(cargoToken({ PACKAGE_TOKEN: 't' }), 'Bearer t');
    assert.equal(cargoToken({}), null);
  });
  test('mask hides every secret', () => {
    assert.equal(mask('a s1 b s2 s1', ['s1', 's2', undefined]), 'a *** b *** ***');
  });
});
