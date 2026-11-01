// Tests of scripts/publish.mjs (pure parts) and of the repository facts it relies on: `node --test scripts/`.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
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
  linkAuth,
  linkPackages,
  linkUrl,
  lockstepProblems,
  shellVersionProblems,
  mask,
  missingExports,
  npmPackages,
  parseArgs,
  parseToolTag,
  planRows,
  publishOrder,
  tagProblems,
  toolFolders,
  toolProblems,
  toolTagProblems,
  workspaceCrates,
  WORKSPACE_HACK,
} from './publish.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const scratch = mkdtempSync(join(tmpdir(), 'paddock-publish-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

const crate = (name, deps = [], extra = {}) => ({ name, version: '0.1.0', publish: ['decryp7'], scaffold: false, deps, ...extra });

describe('parseArgs', () => {
  test('dry run is the default', () => assert.deepEqual(parseArgs([]), { publish: false, skipBuild: false, offline: false, tool: null }));
  test('flags', () => assert.deepEqual(parseArgs(['--skip-build', '--offline']), { publish: false, skipBuild: true, offline: true, tool: null }));
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
  test('a dev-dependency with a version is kept by cargo publish, so it orders like a normal one; a path-only one is stripped', () => {
    const full = workspaceCrates({
      workspace_members: ['a', 't', 'c'],
      packages: [
        pkg('a', 'paddock-a', [{ name: 'paddock-t', kind: 'dev', req: '^0.1.0' }, { name: 'paddock-c', kind: 'dev', req: '*' }]),
        pkg('t', 'paddock-t', []),
        pkg('c', 'paddock-c', []),
      ],
    });
    assert.deepEqual(full.find((c) => c.name === 'paddock-a')?.deps, ['paddock-t']);
    assert.deepEqual(publishOrder(full).map((c) => c.name), ['paddock-c', 'paddock-t', 'paddock-a']);
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
  test('a --workspace step covers every crate it does not exclude', () => {
    const ws = '        run: cargo clippy --workspace --all-targets --all-features -- -D warnings\n        run: cargo nextest run --workspace --all-features --profile ci --exclude paddock-b\n';
    assert.deepEqual(crateStepsMissing(ws, ['paddock-a', 'paddock-b']), ['paddock-b']);
    const cov = '        run: cargo clippy --workspace --all-targets --all-features -- -D warnings\n          cargo llvm-cov nextest --profile ci --workspace --all-features --lcov\n';
    assert.deepEqual(crateStepsMissing(cov, ['paddock-a']), []);
    const noFeatures = '        run: cargo clippy --workspace --all-targets -- -D warnings\n        run: cargo nextest run --workspace --profile ci\n';
    assert.deepEqual(crateStepsMissing(noFeatures, ['paddock-a']), ['paddock-a'], 'without --all-features the adapters are not built');
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

describe('linking published packages to the paddock repository', () => {
  /** A fetch that answers each URL from `answers` (status, body) and records the calls. */
  const fakeFetch = (answers) => {
    const calls = [];
    const fn = async (url, init) => {
      calls.push({ url, method: init.method, auth: init.headers.Authorization });
      const a = answers(url);
      if (a instanceof Error) throw a;
      return { status: a.status, text: async () => a.body ?? '' };
    };
    return { fn, calls };
  };
  const quiet = () => {
    const lines = { log: [], warn: [] };
    return { lines, log: (l) => lines.log.push(l), warn: (l) => lines.warn.push(l) };
  };
  const rows = [
    { kind: 'crate', name: 'paddock-jobs' },
    { kind: 'npm', name: '@decryp7/paddock-ui' },
  ];

  test('the link URL names the cargo or npm type and percent-encodes a scoped name', () => {
    assert.equal(linkUrl('crate', 'paddock-jobs'), 'https://dev.decryptology.net/api/v1/packages/decryp7/cargo/paddock-jobs/-/link/paddock');
    assert.equal(linkUrl('npm', '@decryp7/paddock-ui'), 'https://dev.decryptology.net/api/v1/packages/decryp7/npm/%40decryp7%2Fpaddock-ui/-/link/paddock');
    assert.throws(() => linkUrl('pypi', 'x'), /unknown package kind/);
  });

  test('the publish credential is used: basic auth first, then a package token, then the job token', () => {
    assert.equal(linkAuth({ PACKAGE_AUTH: 'dXNlcjpwdw==', GITEA_TOKEN: 'g' }), 'Basic dXNlcjpwdw==');
    assert.equal(linkAuth({ PACKAGE_TOKEN: 'p', GITEA_TOKEN: 'g' }), 'token p');
    assert.equal(linkAuth({ GITEA_TOKEN: 'g' }), 'token g');
    assert.equal(linkAuth({}), null);
  });

  test('201, 204 and "already linked" are all fine; every package gets one POST', async () => {
    const { fn, calls } = fakeFetch((url) => (url.includes('/cargo/') ? { status: 201 } : { status: 400, body: '{"message":"package is already linked to a repository"}' }));
    const q = quiet();
    const res = await linkPackages(rows, { PACKAGE_AUTH: 'c2VjcmV0' }, { fetch: fn, ...q });
    assert.deepEqual(res.map((r) => [r.ok, r.message]), [[true, 'linked'], [true, 'already linked']]);
    assert.deepEqual(calls.map((c) => [c.method, c.auth]), [['POST', 'Basic c2VjcmV0'], ['POST', 'Basic c2VjcmV0']]);
    assert.equal(q.lines.warn.length, 0);
    const { fn: fn204 } = fakeFetch(() => ({ status: 204 }));
    assert.ok((await linkPackages(rows.slice(0, 1), { GITEA_TOKEN: 't' }, { fetch: fn204, ...quiet() }))[0].ok);
  });

  test('a failed link warns and never throws, and the warning never shows the credential', async () => {
    const { fn } = fakeFetch((url) => (url.includes('/cargo/') ? { status: 403, body: 'denied for c2VjcmV0' } : new Error('connect ECONNREFUSED')));
    const q = quiet();
    const res = await linkPackages(rows, { PACKAGE_AUTH: 'c2VjcmV0' }, { fetch: fn, ...q });
    assert.deepEqual(res.map((r) => r.ok), [false, false]);
    assert.equal(q.lines.warn.length, 2);
    assert.match(q.lines.warn[0], /paddock-jobs was published but not linked to paddock \(HTTP 403 denied for \*\*\*\)/);
    assert.match(q.lines.warn[1], /ECONNREFUSED/);
    assert.ok(!q.lines.warn.join('\n').includes('c2VjcmV0'));
  });

  test('without any credential each link is a warning, not a failure', async () => {
    const { fn, calls } = fakeFetch(() => ({ status: 201 }));
    const q = quiet();
    const res = await linkPackages(rows, {}, { fetch: fn, ...q });
    assert.deepEqual(res.map((r) => r.ok), [false, false]);
    assert.equal(calls.length, 0);
    assert.match(q.lines.warn[0], /no PACKAGE_AUTH/);
  });

  test('the dry run says what it would link, and the publish links before the release is created', () => {
    const src = readFileSync(join(root, 'scripts/publish.mjs'), 'utf8');
    assert.match(src, /would link \$\{/);
    assert.ok(src.indexOf('await linkPackages(todo, env)') < src.indexOf('await createRelease(lead.version, todo, env)'));
  });
});

describe('publishing a tool from its tag (ADR 0022 amendment)', () => {
  const meta = (extra = {}, dep = {}) => ({
    workspace_members: ['t 0.1.0'],
    packages: [
      {
        id: 't 0.1.0',
        name: 'paddock-t',
        version: '0.1.0',
        publish: ['decryp7'],
        metadata: null,
        manifest_path: '/w/tools/t/Cargo.toml',
        dependencies: [{ name: 'paddock-proc', kind: null, req: '^0.1', path: '/w/crates/paddock-proc', registry: 'https://x', ...dep }],
        ...extra,
      },
    ],
  });

  test('parseArgs takes --tool <name> and refuses a bad name or the npm-only flag', () => {
    assert.deepEqual(parseArgs(['--tool', 'heavy', '--offline']), { publish: false, skipBuild: false, offline: true, tool: 'heavy' });
    assert.equal(parseArgs([]).tool, null);
    assert.throws(() => parseArgs(['--tool']), /--tool needs/);
    assert.throws(() => parseArgs(['--tool', '../x']), /--tool needs/);
    assert.throws(() => parseArgs(['--tool', 'heavy', '--skip-build']), /npm build/);
  });

  test('parseToolTag reads <tool>-vX.Y.Z and nothing else', () => {
    assert.deepEqual(parseToolTag('refs/tags/heavy-v0.1.0'), { tool: 'heavy', version: '0.1.0' });
    assert.deepEqual(parseToolTag('refs/tags/wiki-v1.2.3-rc.1'), { tool: 'wiki', version: '1.2.3-rc.1' });
    assert.equal(parseToolTag('refs/tags/v0.2.0'), null);
    assert.equal(parseToolTag('refs/heads/heavy-v0.1.0'), null);
    assert.equal(parseToolTag(undefined), null);
  });

  test('toolTagProblems: only CI on the tool\'s own tag and version', () => {
    assert.deepEqual(toolTagProblems({ CI: 'true', GITHUB_REF: 'refs/tags/heavy-v0.1.0' }, 'heavy', '0.1.0'), []);
    assert.equal(toolTagProblems({ GITHUB_REF: 'refs/tags/heavy-v0.1.0' }, 'heavy', '0.1.0').length, 1);
    assert.equal(toolTagProblems({ CI: 'true', GITHUB_REF: 'refs/tags/wiki-v0.1.0' }, 'heavy', '0.1.0').length, 1);
    assert.equal(toolTagProblems({ CI: 'true', GITHUB_REF: 'refs/tags/heavy-v0.2.0' }, 'heavy', '0.1.0').length, 1);
    assert.equal(toolTagProblems({ CI: 'true', GITHUB_REF: 'refs/tags/v0.1.0' }, 'heavy', '0.1.0').length, 1);
  });

  test('toolProblems: publish setting, path dependencies need a version and the registry', () => {
    assert.deepEqual(toolProblems('t', meta()), []);
    assert.match(toolProblems('t', meta({ publish: [] }))[0], /publish = false/);
    assert.match(toolProblems('t', meta({ publish: null }))[0], /publish = \["decryp7"\]/);
    assert.match(toolProblems('t', meta({}, { req: '*' }))[0], /path without a version/);
    assert.match(toolProblems('t', meta({}, { registry: null }))[0], /registry = "decryp7"/);
    assert.deepEqual(toolProblems('t', meta({}, { req: '*', registry: null, kind: 'dev' })), []);
  });

  test('dry-run plan on the real tools: heavy and wiki publish alone, paddock-dev is refused', () => {
    const plan = (tool) => {
      try {
        return { out: execFileSync('node', [join(root, 'scripts/publish.mjs'), '--tool', tool, '--offline'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }) };
      } catch (e) {
        return { err: String(e.stderr) };
      }
    };
    for (const [tool, name] of [['heavy', 'paddock-heavy'], ['wiki', 'paddock-wiki']]) {
      const { out, err } = plan(tool);
      assert.ok(out, err);
      assert.match(out, new RegExp(`crate\\s+${name}\\s+0\\.1\\.0\\s+publish`));
      assert.match(out, new RegExp(`the tag ${tool}-v\\d+\\.\\d+\\.\\d+ would publish 1 of 1; nothing was uploaded`));
      assert.doesNotMatch(out, /paddock-ui|npm /);
    }
    assert.match(plan('paddock-dev').err, /publish = false/);
    assert.match(plan('nosuchtool').err, /no tools\/nosuchtool/);
  });

  test('every publishable tool passes toolProblems and has its tag trigger in publish-tools.yml', () => {
    const workflow = readFileSync(join(root, '.gitea/workflows/publish-tools.yml'), 'utf8');
    const publishable = [];
    for (const tool of toolFolders(join(root, 'tools'))) {
      const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], { cwd: join(root, 'tools', tool), encoding: 'utf8' }));
      const [pkg] = workspaceCrates(metadata);
      if (!pkg || pkg.publish.length === 0) continue;
      publishable.push(tool);
      assert.deepEqual(toolProblems(tool, metadata), [], `tools/${tool}`);
      assert.ok(workflow.includes(`"${tool}-v*"`), `publish-tools.yml has no "${tool}-v*" trigger`);
    }
    assert.deepEqual(publishable, ['heavy', 'wiki']);
    assert.ok(existsSync(join(root, '.gitea/workflows/publish-tools.yml')));
  });
});
