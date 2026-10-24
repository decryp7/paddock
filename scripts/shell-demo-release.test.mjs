// Tests of scripts/shell-demo-release.mjs (pure parts) and of the repository facts it relies on: `node --test scripts/`.
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { appVersion, cargoVersion, changelogSection, FIRST_LAUNCH, iconProblems, releaseNotes, TAG_PREFIX, tagProblems } from './shell-demo-release.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

describe('the version', () => {
  test('is read from [package] of Cargo.toml, not from another table', () => {
    const toml = '[workspace]\n\n[package]\nname = "x"\nversion = "1.2.3"\n\n[dependencies]\nfoo = { version = "9.9.9" }\n';
    assert.equal(cargoVersion(toml), '1.2.3');
    assert.throws(() => cargoVersion('[package]\nname = "x"\n'), /no release-number version/);
    assert.throws(() => cargoVersion('[package]\nversion = "1.2"\n'), /no release-number version/);
  });

  test('the app has a release-number version and a CHANGELOG section for it', () => {
    const version = appVersion();
    assert.match(version, /^\d+\.\d+\.\d+$/);
    assert.ok(changelogSection(readFileSync(join(root, 'apps/shell-demo/CHANGELOG.md'), 'utf8'), version).length > 0);
  });
});

describe('the tag', () => {
  test('must be shell-demo-v<the app version>', () => {
    assert.deepEqual(tagProblems(`${TAG_PREFIX}1.2.3`, '1.2.3'), []);
    assert.match(tagProblems('v1.2.3', '1.2.3')[0], /not a Shell Demo tag/);
    assert.match(tagProblems(`${TAG_PREFIX}1.2`, '1.2.3')[0], /release number/);
    assert.match(tagProblems(`${TAG_PREFIX}1.2.4`, '1.2.3')[0], /names 1\.2\.4, but .* has 1\.2\.3/);
  });

  test('never matches the packages\' v* tags (publish.yml, ci.yml)', () => {
    assert.ok(!TAG_PREFIX.startsWith('v'));
    const workflow = readFileSync(join(root, '.gitea/workflows/shell-demo-release.yml'), 'utf8');
    assert.match(workflow, /tags: \["shell-demo-v\*"\]/);
  });
});

describe('the notes', () => {
  const changelog = '# Changelog\n\n## [Unreleased]\n\n- later\n\n## [0.2.0] - 2026-11-01\n\n### Added\n\n- B\n\n## [0.1.0] - 2026-10-24\n\n### Added\n\n- A\n';

  test('are the version\'s section only, then the first-launch notes', () => {
    assert.equal(changelogSection(changelog, '0.2.0'), '### Added\n\n- B');
    assert.equal(changelogSection(changelog, '0.1.0'), '### Added\n\n- A');
    assert.equal(releaseNotes(changelog, '0.1.0'), `### Added\n\n- A\n\n${FIRST_LAUNCH}\n`);
    assert.match(FIRST_LAUNCH, /Open Anyway/);
  });

  test('a version without a section, or an empty one, is refused', () => {
    assert.throws(() => changelogSection(changelog, '0.3.0'), /no section for 0\.3\.0/);
    assert.throws(() => changelogSection('## [1.0.0]\n\n## [0.9.0]\n- x\n', '1.0.0'), /empty/);
    // 0.1.0 is not 0.1.01 or 10.1.0.
    assert.throws(() => changelogSection('## [10.1.0]\n- x\n', '0.1.0'), /no section/);
  });
});

describe('the icons', () => {
  test('a missing or different file is named', () => {
    const a = Buffer.from('a');
    const b = Buffer.from('b');
    assert.deepEqual(iconProblems(new Map([['x.png', a]]), new Map([['x.png', a]])), []);
    assert.match(iconProblems(new Map([['x.png', a]]), new Map())[0], /x\.png is missing/);
    assert.match(iconProblems(new Map([['x.png', a]]), new Map([['x.png', b]]))[0], /x\.png differs/);
  });

  test('the app carries every icon the pipeline made for it, byte for byte', () => {
    const dir = (p) => new Map(readdirSync(join(root, p)).filter((f) => f.includes('.')).map((f) => [f, readFileSync(join(root, p, f))]));
    assert.deepEqual(iconProblems(dir('icons/shell-demo/tauri'), dir('apps/shell-demo/src-tauri/icons')), []);
  });
});
