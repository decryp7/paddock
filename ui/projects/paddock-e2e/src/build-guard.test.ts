import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { appBuildGuard, appBuildGuardOf, assertFreshBuilds, CSS_ENTRIES, reuseServer, stampBuild, staleBuilds, uiConfigGuard } from './build-guard.ts';

const put = (file: string, text = 'x') => {
  mkdirSync(join(file, '..'), { recursive: true });
  writeFileSync(file, text);
};
/** A ui/ folder with sources and a build of them (css and dist, both stamped). */
function fixture(): string {
  const ui = mkdtempSync(join(tmpdir(), 'build-guard-'));
  const pkg = join(ui, 'projects', 'paddock-ui');
  put(join(pkg, 'scss', '_tokens.scss'), 'a');
  put(join(pkg, 'src', 'a.ts'), 'a');
  put(join(pkg, 'shell', 'src', 'b.ts'), 'a');
  put(join(pkg, 'ng-package.json'), '{}');
  for (const n of CSS_ENTRIES) put(join(pkg, 'css', `${n}.css`));
  put(join(ui, 'dist', 'paddock-ui', 'index.mjs'));
  stampBuild(ui, 'css');
  stampBuild(ui, 'dist');
  return ui;
}

test('reuse is opt-in only, and never on CI', () => {
  assert.equal(reuseServer({}), false);
  assert.equal(reuseServer({ PW_REUSE_SERVER: '' }), false);
  assert.equal(reuseServer({ PW_REUSE_SERVER: '0' }), false);
  assert.equal(reuseServer({ PW_REUSE_SERVER: '1' }), true);
  assert.equal(reuseServer({ PW_REUSE_SERVER: '1', CI: 'true' }), false);
});

test('a fresh build passes; a changed source fails with the rebuild hint, an unchanged rebuild or a touch does not', () => {
  const ui = fixture();
  try {
    assert.deepEqual(staleBuilds(ui, 'ui-dist'), []);
    // Same content, newer mtime (a checkout, a touch, a rebuild of unchanged sources): still fresh.
    utimesSync(join(ui, 'projects', 'paddock-ui', 'scss', '_tokens.scss'), 9_999_999_999, 9_999_999_999);
    stampBuild(ui, 'css');
    assert.deepEqual(staleBuilds(ui, 'ui-dist'), []);
    put(join(ui, 'projects', 'paddock-ui', 'scss', '_tokens.scss'), 'b');
    assert.match(staleBuilds(ui, 'ui')[0], /css\/\*\.css was built from older sources/);
    assert.throws(() => assertFreshBuilds(ui, 'ui', {}), /rebuild ui: npm run build -w ui/);
    assert.doesNotThrow(() => assertFreshBuilds(ui, 'ui', { PW_SKIP_BUILD_GUARD: '1' }));
    stampBuild(ui, 'css');
    assert.deepEqual(staleBuilds(ui, 'ui'), []);
    assert.match(staleBuilds(ui, 'ui-dist')[0], /ui\/dist was built from older sources/); // css restamped, dist not
  } finally {
    rmSync(ui, { recursive: true, force: true });
  }
});

test('tests, bin and the generated css are not sources of the dist; a missing entry or stamp is stale', () => {
  const ui = fixture();
  const pkg = join(ui, 'projects', 'paddock-ui');
  try {
    put(join(pkg, 'src', 'a.spec.ts'), 'new');
    put(join(pkg, 'bin', 'tool.mjs'), 'new');
    put(join(pkg, 'css', 'material.css'), 'rewritten');
    assert.deepEqual(staleBuilds(ui, 'ui-dist'), []);
    rmSync(join(pkg, 'css', 'material.css'));
    assert.deepEqual(staleBuilds(ui, 'ui'), ['css/material.css is missing']);
    rmSync(join(ui, 'dist', '.source-hash'));
    assert.match(staleBuilds(ui, 'ui-dist').join(), /ui\/dist has no \.source-hash/);
    assert.deepEqual(staleBuilds(ui, 'none'), []);
  } finally {
    rmSync(ui, { recursive: true, force: true });
  }
});

test('stampBuild leaves an unchanged stamp alone', () => {
  const ui = fixture();
  try {
    const file = join(ui, 'dist', '.source-hash');
    utimesSync(file, 1000, 1000);
    stampBuild(ui, 'dist');
    assert.equal(Math.round(readFileSync(file).length), 64);
    assert.ok(existsSync(file));
  } finally {
    rmSync(ui, { recursive: true, force: true });
  }
});

// Every app config calls appBuildGuard; on a fresh checkout (no dist, no css) it must pass unless the app consumes ui/dist.
const REPO = join(import.meta.dirname, '..', '..', '..', '..');
const APPS = join(REPO, 'apps');
const appDirs = readdirSync(APPS, { withFileTypes: true })
  .filter((e) => e.isDirectory() && existsSync(join(APPS, e.name, 'app', 'playwright.config.ts')))
  .map((e) => join(APPS, e.name, 'app'));

test('there are app configs to check', () => {
  assert.ok(appDirs.length >= 5);
});

for (const dir of appDirs) {
  test(`${dir.slice(APPS.length + 1)}: its config calls the guard, and a fresh checkout passes unless it consumes ui/dist`, () => {
    for (const config of [join(dir, 'playwright.config.ts'), ...['scripts/screenshots', 'scripts/visual-qa', 'e2e/screenshots'].map((d) => join(dir, d, 'playwright.config.ts')).filter(existsSync)]) {
      assert.match(readFileSync(config, 'utf8'), /appBuildGuard\(/, config);
    }
    const emptyUi = mkdtempSync(join(tmpdir(), 'build-guard-empty-'));
    try {
      const consumes = appBuildGuardOf(dir) === 'ui-dist';
      assert.equal(consumes, /ui\/dist\/paddock-ui/.test(readFileSync(join(dir, 'tsconfig.json'), 'utf8')));
      if (consumes) assert.throws(() => appBuildGuard(dir, emptyUi, {}), /rebuild ui/);
      else assert.doesNotThrow(() => appBuildGuard(dir, emptyUi, {}));
    } finally {
      rmSync(emptyUi, { recursive: true, force: true });
    }
  });
}

test('a change in a secondary entry point or a package file makes the dist stale', () => {
  for (const file of [['shell', 'src', 'b.ts'], ['ng-package.json'], ['workbench', 'src', 'new.ts']]) {
    const ui = fixture();
    try {
      put(join(ui, 'projects', 'paddock-ui', ...file), 'changed');
      assert.match(staleBuilds(ui, 'ui-dist').join(), /ui\/dist was built from older sources/, file.join('/'));
      assert.deepEqual(staleBuilds(ui, 'ui'), []);
    } finally {
      rmSync(ui, { recursive: true, force: true });
    }
  }
});

test('the ui config checks the css only when a running server is reused (its webServers build it otherwise)', () => {
  const ui = fixture();
  try {
    put(join(ui, 'projects', 'paddock-ui', 'scss', '_tokens.scss'), 'changed');
    assert.doesNotThrow(() => uiConfigGuard(ui, {}));
    assert.throws(() => uiConfigGuard(ui, { PW_REUSE_SERVER: '1' }), /rebuild ui/);
    assert.doesNotThrow(() => uiConfigGuard(ui, { PW_REUSE_SERVER: '1', PW_SKIP_BUILD_GUARD: '1' }));
  } finally {
    rmSync(ui, { recursive: true, force: true });
  }
});
