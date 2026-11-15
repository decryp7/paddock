// conformance/ci/safe-rm.sh: the shell scripts' one guarded recursive delete, the shell copy of paddock-dev's `remove_under`
// (tools/paddock-dev/src/safe_remove/guard.rs; docs/known-copies.toml, "safe-remove-under"). Each case runs the helper in bash
// and sh against a scratch tree and checks both the exit code and what is left on disk.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const LIB = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci', 'safe-rm.sh');
const SHELLS = ['bash', 'sh'];

/** Runs `safe_rm <args>` after sourcing the helper; HOME is `home`. */
function safeRm(shell, args, home) {
  const quoted = args.map((a) => `'${a.replace(/'/g, `'\\''`)}'`).join(' ');
  const r = spawnSync(shell, ['-c', `. '${LIB}' && safe_rm ${quoted}`], { encoding: 'utf8', env: { ...process.env, HOME: home } });
  return { status: r.status, err: r.stderr };
}

function tree() {
  const base = realpathSync(mkdtempSync(join(tmpdir(), 'safe-rm-')));
  const root = join(base, 'root');
  const home = join(base, 'home');
  mkdirSync(join(root, 'a', 'b'), { recursive: true });
  writeFileSync(join(root, 'a', 'b', 'f'), 'x');
  mkdirSync(join(base, 'outside'));
  writeFileSync(join(base, 'outside', 'keep'), 'x');
  mkdirSync(home);
  return { base, root, home };
}

for (const shell of SHELLS) {
  test(`${shell}: a folder strictly under the root is removed; a missing one is fine`, () => {
    const { base, root, home } = tree();
    try {
      assert.deepEqual(safeRm(shell, [root, join(root, 'a')], home).status, 0);
      assert.equal(existsSync(join(root, 'a')), false);
      assert.equal(existsSync(root), true, 'the root stays');
      assert.equal(safeRm(shell, [root, join(root, 'gone', 'x')], home).status, 0, 'a path that is not there is the goal');
    } finally {
      rmSync(base, { recursive: true, force: true });
    }
  });

  test(`${shell}: empty, /, the home folder, .., relative, the root itself and paths outside it are refused, and nothing is touched`, () => {
    const { base, root, home } = tree();
    try {
      const refused = [
        [[root, ''], /empty/],
        [['', join(root, 'a')], /empty/],
        [[root, '/'], /not strictly under/],
        [['/', join(root, 'a')], /drive root or a top-level folder/],
        [[base.split('/').slice(0, 2).join('/'), join(root, 'a')], /drive root or a top-level folder/],
        [[home, join(home, 'x')], /home folder or above it/],
        [[dirname(home), join(root, 'a')], /home folder or above it/],
        [[root, home], /not strictly under/],
        [[root, `${root}/a/../../outside`], /\.\./],
        [[`${root}/a/..`, join(root, 'a')], /\.\./],
        [[root, 'a/b'], /not an absolute path/],
        [['root', join(root, 'a')], /not an absolute path/],
        [[root, root], /not strictly under/],
        [[root, join(base, 'outside')], /not strictly under/],
        [[root, join(root, 'a'), join(base, 'outside')], /not strictly under/],
      ];
      for (const [args, why] of refused) {
        const r = safeRm(shell, args, home);
        assert.equal(r.status, 1, `${JSON.stringify(args)} must be refused`);
        assert.match(r.err, /::error::safe_rm: /, JSON.stringify(args));
        assert.match(r.err, why, JSON.stringify(args));
      }
      assert.equal(existsSync(join(root, 'a', 'b', 'f')), true, 'a refused call removes nothing, also its good targets');
      assert.equal(existsSync(join(base, 'outside', 'keep')), true);
    } finally {
      rmSync(base, { recursive: true, force: true });
    }
  });

  test(`${shell}: a link on the way is refused; a link as the target is removed as a link`, () => {
    const { base, root, home } = tree();
    try {
      symlinkSync(join(base, 'outside'), join(root, 'via'));
      const r = safeRm(shell, [root, join(root, 'via', 'keep')], home);
      assert.equal(r.status, 1);
      assert.match(r.err, /passes through the link/);
      assert.equal(existsSync(join(base, 'outside', 'keep')), true);
      assert.equal(safeRm(shell, [root, join(root, 'via')], home).status, 0);
      assert.throws(() => lstatSync(join(root, 'via')), 'the link is gone');
      assert.equal(existsSync(join(base, 'outside', 'keep')), true, 'its target stays');
    } finally {
      rmSync(base, { recursive: true, force: true });
    }
  });
}

test('safe_rm needs a root and a path', () => {
  const { base, home } = tree();
  try {
    const r = safeRm('bash', [], home);
    assert.equal(r.status, 2);
    assert.match(r.err, /usage: safe_rm <root> <path>/);
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
