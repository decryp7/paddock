import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isCleanupSource, unscopedCleanups } from '../src/checks/C115-scoped-cleanup.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lines = (text, file = 'x.sh') => unscopedCleanups(text, file).map((k) => k.line);

test('C115 bad: git worktree prune in shell, YAML, PowerShell, JS and Rust argument arrays', () => {
  assert.deepEqual(lines('set -e\ngit worktree prune\n'), [2]);
  assert.deepEqual(lines('git -C "$repo" worktree  prune --expire now\n'), [1]);
  assert.deepEqual(lines('steps:\n  - run: git worktree prune\n', 'ci.yml'), [2]);
  assert.deepEqual(lines('git worktree prune\n', 'a.ps1'), [1]);
  assert.deepEqual(lines('spawnSync("git", ["worktree", "prune"]);\n', 'a.mjs'), [1]);
  assert.deepEqual(lines('fn a() {\n    Command::new("git").args(["worktree", "prune"]).status();\n}\n', 'a.rs'), [2]);
  assert.deepEqual(lines('fn a() {\n    Command::new("git").args([\n        "worktree",\n        "prune",\n    ]);\n}\n', 'a.rs'), [3]);
});

test('C115 bad: container-engine prunes and mass removals', () => {
  for (const c of [
    'docker system prune -af',
    'docker container prune -f',
    'docker volume prune -f',
    'docker image prune -a',
    'docker network prune',
    'podman system prune',
    'docker rm -f $(docker ps -aq)',
    'docker stop `docker ps -q`',
    'docker kill $(docker container ls -q)',
    'docker ps -aq | xargs docker rm -f',
    'docker ps -q | xargs -r docker stop',
  ]) assert.deepEqual(lines(`${c}\n`), [1], c);
  assert.deepEqual(lines('Command::new("docker").args(["system", "prune", "-f"]);\n', 'a.rs'), [1]);
  assert.deepEqual(lines('Command::new("docker").args(["volume", "prune"]);\n', 'a.rs'), [1]);
});

test('C115 bad: git clean -x or -d with no pathspec', () => {
  for (const c of ['git clean -fdx', 'git clean -fd', 'git clean -xf', 'git clean -f -d', 'git clean -ffdx', 'git -C "$d" clean -fX', 'git clean -fd && echo ok']) {
    assert.deepEqual(lines(`${c}\n`), [1], c);
  }
  assert.deepEqual(lines('Command::new("git").args(["clean", "-fdx"]);\n', 'a.rs'), [1]);
});

test('C115 bad: rm -r and Remove-Item -Recurse on a bare variable or glob', () => {
  for (const c of ['rm -rf "$DIR"', 'rm -rf $DIR', 'rm -rf "${DIR}"', 'rm -rf "$DIR"/*', 'rm -rf $DIR/*', 'rm -rf *', 'rm -rf ./*', 'rm -rf /*', 'rm -fr "$x"', 'rm -r -f "$x"', 'cd /x && rm -rf "$x"; echo', 'rm -rf ~', 'rm -rf .', 'rm --recursive --force "$x"']) {
    assert.deepEqual(lines(`${c}\n`), [1], c);
  }
  for (const c of ['Remove-Item -Recurse -Force $dir', 'Remove-Item $dir -Recurse', 'Remove-Item -Path "$dir\\*" -Recurse -Force', 'Remove-Item -Recurse -Force -LiteralPath $env:SCRATCH']) {
    assert.deepEqual(lines(`${c}\n`, 'a.ps1'), [1], c);
  }
});

test('C115 good: scoped cleanups, dry runs and unrelated commands', () => {
  for (const c of [
    'git worktree list',
    'git worktree remove "$wt"',
    'git worktree add ../x',
    './dev worktree clean',
    'docker ps -q --filter label=paddock-own | xargs docker stop',
    'docker rm -f paddock-test-1',
    'docker stop $(docker ps -q --filter name=paddock-own)',
    'docker run --rm img',
    'docker image ls',
    'git clean -n -fdx',
    'git clean --dry-run -fd',
    'git clean -fd -- target/own',
    'git clean -fdx apps/debrief/out',
    'git clean -f',
    'rm -rf "$DIR/build"',
    'rm -rf "$OWN"/cache',
    'rm -rf target/own',
    'rm -f "$file"',
    'rm "$x"',
    'Remove-Item -Recurse -Force "$dir\\build"',
    'Remove-Item $file',
  ]) assert.deepEqual(lines(`${c}\n`, c.startsWith('Remove') ? 'a.ps1' : 'x.sh'), [], c);
  assert.deepEqual(lines('Command::new("git").args(["clean", "-fd", "--", "out"]);\n', 'a.rs'), []);
  assert.deepEqual(lines('Command::new("git").args(["worktree", "list"]);\n', 'a.rs'), []);
  assert.deepEqual(lines('Command::new("docker").args(["ps", "-q"]);\n', 'a.rs'), []);
  assert.deepEqual(lines('Command::new("notdocker").args(["system", "prune"]);\n', 'a.rs'), [], 'an array is read only near a container engine');
});

test('C115 good: comments are not read', () => {
  assert.deepEqual(lines('# git worktree prune\n# rm -rf "$X"\necho ok\n'), []);
  assert.deepEqual(lines('// git worktree prune\n/// docker system prune\nfn a() {}\n', 'a.rs'), []);
});

test('C115 waivers: with a reason the site passes, without one it is reported', () => {
  assert.equal(unscopedCleanups('# conformance: scoped-cleanup — a mktemp folder of this run\nrm -rf "$tmp"\n')[0].waived, 'reason');
  assert.equal(unscopedCleanups('rm -rf "$tmp" # conformance: scoped-cleanup — our own mktemp folder\n')[0].waived, 'reason');
  assert.equal(unscopedCleanups('// conformance: scoped-cleanup — the record of this run\nCommand::new("git").args(["worktree", "prune"]);\n', 'a.rs')[0].waived, 'reason');
  assert.equal(unscopedCleanups('rm -rf "$tmp" # conformance: scoped-cleanup\n')[0].waived, 'no-reason');
  assert.equal(unscopedCleanups('# conformance: scoped-cleanup — fine\n\nrm -rf "$tmp"\n')[0].waived, undefined, 'only the line or the line above');
});

test('C115 reads shell scripts without an extension and skips docs, fixtures and the guard hook', () => {
  assert.equal(isCleanupSource('dev', '#!/usr/bin/env bash\n'), true);
  assert.equal(isCleanupSource('dev', 'plain text'), false);
  assert.equal(isCleanupSource('docs/x.md'), false);
  assert.equal(isCleanupSource('a/fixtures/x.sh'), true);
  assert.equal(isCleanupSource('conformance/fixtures/x.sh'), false);
  assert.equal(isCleanupSource('tools/paddock-dev/src/hook/rules/no_unscoped_cleanup.rs'), false);
  assert.equal(isCleanupSource('crates/a/src/x.rs'), true);
});

test('C115 through the runner: an error with the next step per site, a waiver and the baseline pass', async () => {
  const f = await findingsOf(
    'C115',
    makeApp({
      'scripts/bad.sh': '#!/bin/sh\ngit worktree prune\n',
      'scripts/ok.sh': '#!/bin/sh\n# conformance: scoped-cleanup — our own mktemp folder\nrm -rf "$tmp"\n',
      'scripts/bare.sh': '#!/bin/sh\n# conformance: scoped-cleanup\nrm -rf "$tmp"\n',
      'docs/note.md': 'git worktree prune\n',
      'conformance/test/x.sh': 'git worktree prune\n',
      'crates/a/src/c.rs': 'fn a() { Command::new("docker").args(["system", "prune"]); }\n',
    }),
  );
  assert.deepEqual(f.filter((x) => x.severity === 'error').map((x) => x.file).sort(), ['crates/a/src/c.rs', 'scripts/bad.sh', 'scripts/bare.sh']);
  assert.match(f.find((x) => x.severity === 'warn').message, /1 site waived.*scripts\/ok\.sh:3/);
  assert.match(f.find((x) => x.file === 'scripts/bad.sh').fix, /\.\/dev worktree clean/);
  assert.match(f.find((x) => x.file.endsWith('c.rs')).fix, /by name or label/);
  assert.match(f.find((x) => x.file.endsWith('bare.sh')).message, /no reason/);
});

test('C115 baseline: a listed site passes, a new one fails, a gone one warns', async () => {
  const { lineHash } = await import('../src/checks/C71-test-timing.mjs');
  const old = 'rm -rf "$tmp"';
  const base = JSON.stringify({ sites: [{ file: 'scripts/old.sh', line: 2, hash: lineHash(old) }, { file: 'scripts/gone.sh', line: 2, hash: 'deadbeefdeadbeef' }] });
  const root = makeApp({
    'conformance/baselines/C115.json': base,
    'scripts/old.sh': `#!/bin/sh\n${old}\n`,
    'scripts/new.sh': `#!/bin/sh\n${old}\n`,
  });
  const f = await findingsOf('C115', root, { options: { git: () => { throw new Error('no git'); } } });
  assert.deepEqual(f.filter((x) => x.severity === 'error').map((x) => x.file), ['scripts/new.sh']);
  assert.ok(f.some((x) => x.severity === 'warn' && /is gone/.test(x.message) && x.file === 'scripts/gone.sh'));
});

test('C115 agrees with the shared corpus (the guard hook rule reads the same file)', async () => {
  const { readFileSync } = await import('node:fs');
  const text = readFileSync(new URL('../../tools/paddock-dev/tests/fixtures/cleanup-corpus.tsv', import.meta.url), 'utf8');
  const wrong = [];
  for (const row of text.split('\n').filter((l) => l && !l.startsWith('#'))) {
    const [verdict, , cmd] = row.split('\t');
    const hit = unscopedCleanups(`${cmd}\n`).length > 0;
    if (hit !== (verdict === 'deny')) wrong.push(`${verdict}: ${cmd}`);
  }
  assert.deepEqual(wrong, []);
});

test('C115 reads commands, not message strings, in Rust and JS', () => {
  assert.deepEqual(lines('fn a() -> String {\n    format!("its folder is missing (git worktree prune)")\n}\n', 'a.rs'), []);
  assert.deepEqual(lines('const m = "run git worktree prune to fix";\n', 'a.mjs'), []);
  assert.deepEqual(lines('execSync("git worktree prune");\n', 'a.mjs'), [1]);
  assert.deepEqual(lines('Command::new("sh").arg("-c").arg("git worktree prune");\n', 'a.rs'), [1]);
  assert.deepEqual(lines('Command::new("git").args(["worktree", "prune"]);\n', 'a.rs'), [1]);
  assert.deepEqual(lines('for p in ps {\n    Command::new("git").args(["worktree", "remove", "--force", p]);\n}\n', 'a.rs'), [2]);
  assert.deepEqual(lines('Command::new("git").args(["worktree", "remove", "--force", p]);\n', 'a.rs'), []);
});

test('C115 skips only its own rule file and the oracle fixtures', () => {
  assert.equal(isCleanupSource('docs/tools/x.sh'), true);
  assert.equal(isCleanupSource('crates/a/fixtures/x.sh'), true);
  assert.equal(isCleanupSource('tools/paddock-dev/tests/fixtures/worktree-oracle/worktree-sh.txt'), false);
  assert.equal(isCleanupSource('tools/paddock-dev/src/hook/rules/no_unscoped_cleanup.rs'), false);
  assert.equal(isCleanupSource('tools/paddock-dev/src/hook/lex.rs'), true);
});

test('C115 summarises the sites it let through on a waiver', async () => {
  const f = await findingsOf('C115', makeApp({ 'scripts/ok.sh': '#!/bin/sh\n# conformance: scoped-cleanup — our own folder\nrm -rf "$tmp"\n' }));
  assert.deepEqual(f.map((x) => x.severity), ['warn']);
  assert.match(f[0].message, /1 site.*waived.*scripts\/ok\.sh:3/);
});

test('C115 baseline tool: lists sites and waived sites, shrinks without adding', async () => {
  const { execFileSync } = await import('node:child_process');
  const { readFileSync } = await import('node:fs');
  const { join } = await import('node:path');
  const { lineHash } = await import('../src/checks/C71-test-timing.mjs');
  const root = makeApp({
    'conformance/baselines/C115.json': JSON.stringify({ sites: [{ file: 'scripts/old.sh', line: 2, hash: lineHash('rm -rf "$a"') }, { file: 'scripts/gone.sh', line: 2, hash: 'deadbeefdeadbeef' }] }),
    'scripts/old.sh': '#!/bin/sh\nrm -rf "$a"\n',
    'scripts/new.sh': '#!/bin/sh\nrm -rf "$b"\n# conformance: scoped-cleanup — own folder\nrm -rf "$c"\n',
  });
  const tool = new URL('../bin/paddock-cleanup-baseline.mjs', import.meta.url).pathname;
  const list = execFileSync('node', [tool, '--root', root], { encoding: 'utf8' });
  assert.match(list, /scripts\/new\.sh:2/);
  assert.match(list, /scripts\/new\.sh:4 {2}\(waived\)/);
  assert.match(list, /2 sites, 1 waived/);
  execFileSync('node', [tool, '--root', root, '--shrink'], { encoding: 'utf8' });
  const after = JSON.parse(readFileSync(join(root, 'conformance/baselines/C115.json'), 'utf8')).sites;
  assert.deepEqual(after.map((s) => s.file), ['scripts/old.sh'], 'the gone entry is removed and the new site is not added');
});
