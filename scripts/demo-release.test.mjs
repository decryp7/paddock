// Tests of scripts/demo-release.mjs (pure parts) and of the repository facts it relies on, for every demo app in its table:
// `node --test scripts/`.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { APPS, appConfig, appVersion, cargoVersion, changelogSection, firstLaunch, iconProblems, releaseNotes, tagPrefix, tagProblems } from './demo-release.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

describe('the table', () => {
  test('names the shell and workbench demos, and an unknown app is refused', () => {
    assert.deepEqual(Object.keys(APPS), ['shell-demo', 'workbench-demo']);
    assert.equal(appConfig('shell-demo').title, 'Shell Demo');
    assert.throws(() => appConfig('nope'), /unknown app "nope" \(shell-demo, workbench-demo\)/);
    assert.throws(() => appConfig('constructor'), /unknown app/);
    assert.throws(() => appConfig(undefined), /unknown app ""/);
  });

  test('every sidecar is a binary the app declares', () => {
    for (const app of Object.values(APPS)) {
      const conf = JSON.parse(readFileSync(join(root, 'apps', app.id, 'src-tauri/tauri.conf.json'), 'utf8'));
      const declared = (conf.bundle?.externalBin ?? []).map((b) => b.split('/').pop());
      assert.deepEqual([...app.sidecars].sort(), declared.sort(), app.id);
    }
  });
});

describe('the version', () => {
  test('is read from [package] of Cargo.toml, not from another table', () => {
    const toml = '[workspace]\n\n[package]\nname = "x"\nversion = "1.2.3"\n\n[dependencies]\nfoo = { version = "9.9.9" }\n';
    assert.equal(cargoVersion(toml), '1.2.3');
    assert.throws(() => cargoVersion('[package]\nname = "x"\n'), /no release-number version/);
    assert.throws(() => cargoVersion('[package]\nversion = "1.2"\n'), /no release-number version/);
  });

  test('each app has a release-number version and a CHANGELOG section for it', () => {
    for (const app of Object.values(APPS)) {
      const version = appVersion(app);
      assert.match(version, /^\d+\.\d+\.\d+$/, app.id);
      assert.ok(changelogSection(readFileSync(join(root, 'apps', app.id, 'CHANGELOG.md'), 'utf8'), version).length > 0, app.id);
    }
  });
});

describe('the tag', () => {
  for (const app of Object.values(APPS)) {
    test(`of ${app.id} must be ${app.id}-v<the app version>`, () => {
      const prefix = tagPrefix(app);
      assert.equal(prefix, `${app.id}-v`);
      assert.deepEqual(tagProblems(app, `${prefix}1.2.3`, '1.2.3'), []);
      assert.match(tagProblems(app, 'v1.2.3', '1.2.3')[0], new RegExp(`not a ${app.title} tag`));
      assert.match(tagProblems(app, `${prefix}1.2`, '1.2.3')[0], /release number/);
      assert.match(tagProblems(app, `${prefix}1.2.4`, '1.2.3')[0], new RegExp(`names 1\\.2\\.4, but apps/${app.id}/src-tauri/Cargo.toml has 1\\.2\\.3`));
    });

    test(`of ${app.id} never matches the packages' v* tags or another demo's, and its workflow is on it only`, () => {
      assert.ok(!tagPrefix(app).startsWith('v'));
      for (const other of Object.values(APPS)) if (other !== app) assert.match(tagProblems(app, `${tagPrefix(other)}1.2.3`, '1.2.3')[0], /not a/);
      const workflow = readFileSync(join(root, `.gitea/workflows/${app.id}-release.yml`), 'utf8');
      assert.match(workflow, new RegExp(`tags: \\["${app.id}-v\\*"\\]`));
      assert.match(workflow, new RegExp(`scripts/demo/bundle\\.sh ${app.id} `));
      assert.match(workflow, new RegExp(`scripts/demo/verify-macos\\.sh ${app.id}`));
    });
  }
});

describe('the notes', () => {
  const changelog = '# Changelog\n\n## [Unreleased]\n\n- later\n\n## [0.2.0] - 2026-11-01\n\n### Added\n\n- B\n\n## [0.1.0] - 2026-10-24\n\n### Added\n\n- A\n';

  test('are the version\'s section only, then the app\'s first-launch notes', () => {
    const shell = appConfig('shell-demo');
    const workbench = appConfig('workbench-demo');
    assert.equal(changelogSection(changelog, '0.2.0'), '### Added\n\n- B');
    assert.equal(changelogSection(changelog, '0.1.0'), '### Added\n\n- A');
    assert.equal(releaseNotes(shell, changelog, '0.1.0'), `### Added\n\n- A\n\n${firstLaunch(shell)}\n`);
    assert.match(firstLaunch(shell), /Open Anyway[\s\S]*Open Anyway[\s\S]*administrator/);
    assert.match(firstLaunch(shell), /“Paddock Shell Demo” was blocked/);
    assert.match(firstLaunch(workbench), /Paddock Workbench Demo is not code-signed/);
  });

  test('a version without a section, or an empty one, is refused', () => {
    assert.throws(() => changelogSection(changelog, '0.3.0'), /no section for 0\.3\.0/);
    assert.throws(() => changelogSection('## [1.0.0]\n\n## [0.9.0]\n- x\n', '1.0.0'), /empty/);
    // 0.1.0 is not 0.1.01 or 10.1.0.
    assert.throws(() => changelogSection('## [10.1.0]\n- x\n', '0.1.0'), /no section/);
  });
});

describe('the icons', () => {
  const app = appConfig('workbench-demo');

  test('a missing or different file is named', () => {
    const a = Buffer.from('a');
    const b = Buffer.from('b');
    assert.deepEqual(iconProblems(app, new Map([['x.png', a]]), new Map([['x.png', a]])), []);
    assert.match(iconProblems(app, new Map([['x.png', a]]), new Map())[0], /x\.png is missing from apps\/workbench-demo\/src-tauri\/icons/);
    assert.match(iconProblems(app, new Map([['x.png', a]]), new Map([['x.png', b]]))[0], /x\.png differs from icons\/workbench-demo\/tauri/);
  });

  test('each app carries every icon the pipeline made for it, byte for byte', () => {
    const dir = (p) => new Map(readdirSync(join(root, p)).filter((f) => f.includes('.')).map((f) => [f, readFileSync(join(root, p, f))]));
    for (const a of Object.values(APPS)) assert.deepEqual(iconProblems(a, dir(`icons/${a.id}/tauri`), dir(`apps/${a.id}/src-tauri/icons`)), [], a.id);
  });
});

describe('the command line', () => {
  const run = (...args) => spawnSync(process.execPath, [join(root, 'scripts/demo-release.mjs'), ...args], { encoding: 'utf8' });

  test('prints an app\'s version and sidecars, and stops on an unknown app or command', () => {
    for (const app of Object.values(APPS)) {
      assert.equal(run(app.id, 'version').stdout.trim(), appVersion(app));
      assert.equal(run(app.id, 'sidecars').stdout.trim(), app.sidecars.join('\n'));
      assert.equal(run(app.id, 'icons').status, 0, app.id);
    }
    assert.notEqual(run('nope', 'version').status, 0);
    assert.match(run('shell-demo', 'frobnicate').stderr, /unknown command/);
  });

  test('publish refuses outside CI (rule 9)', () => {
    const env = { ...process.env, CI: '', GITEA_ACTIONS: '' };
    const r = spawnSync(process.execPath, [join(root, 'scripts/demo-release.mjs'), 'shell-demo', 'publish', 'shell-demo-v0.0.0', '.', 'n.md'], { encoding: 'utf8', env });
    assert.match(r.stderr, /only in CI from a shell-demo-v\* tag/);
  });
});
