// conformance/ci/safe-rm.sh: the shell scripts' one guarded recursive delete, the shell copy of paddock-dev's `remove_under`
// (tools/paddock-dev/src/safe_remove/guard.rs; docs/known-copies.toml, "safe-remove-under"). Each case runs the helper in bash
// and sh against a scratch tree and checks both the exit code and what is left on disk.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const LIB = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci', 'safe-rm.sh');
/** The parity table the Rust guard's tests read too (tools/paddock-dev/src/safe_remove/guard.rs). */
const PARITY = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'tools', 'paddock-dev', 'tests', 'fixtures', 'safe-remove-parity.tsv');
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

test('SAFE_RM_ANNOTATION=warning makes a refusal a warning for a caller that goes on', () => {
  const { base, root, home } = tree();
  try {
    const r = spawnSync('bash', ['-c', `. '${LIB}' && SAFE_RM_ANNOTATION=warning safe_rm '${root}' /`], { encoding: 'utf8', env: { ...process.env, HOME: home } });
    assert.equal(r.status, 1);
    assert.match(r.stderr, /^::warning::safe_rm: /m);
    assert.doesNotMatch(r.stderr, /::error::/);
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});

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

const FOLD = process.platform === 'darwin' || process.platform === 'win32';
const gone = (p) => {
  try {
    lstatSync(p);
    return false;
  } catch {
    return true;
  }
};

for (const shell of SHELLS) {
  test(`${shell}: the shared parity table with the Rust guard holds`, () => {
    const rows = readFileSync(PARITY, 'utf8').split('\n').filter((l) => l && !l.startsWith('#') && !l.startsWith('name\t'));
    assert.ok(rows.length >= 15, `the table was read (${rows.length} rows)`);
    for (const row of rows) {
      const [name, root, target, result, goneRel] = row.split('\t');
      const b = realpathSync(mkdtempSync(join(tmpdir(), 'safe-rm-parity-')));
      try {
        mkdirSync(join(b, 'root', 'a', 'b'), { recursive: true });
        writeFileSync(join(b, 'root', 'a', 'b', 'f'), 'x');
        mkdirSync(join(b, 'outside', 'keep'), { recursive: true });
        writeFileSync(join(b, 'outside', 'keep', 'f'), 'x');
        symlinkSync(join(b, 'outside'), join(b, 'root', 'link'));
        symlinkSync(join(b, 'root'), join(b, 'rootlink'));
        const fill = (s) => s.replaceAll('<B>', b);
        const r = safeRm(shell, [fill(root), fill(target)], join(b, 'home'));
        const ok = result === 'ok' || (result === 'ok-fold|refused-nofold' && FOLD);
        assert.equal(r.status, ok ? 0 : 1, `${name}: ${r.err}`);
        if (!ok) assert.ok(existsSync(join(b, 'root', 'a', 'b', 'f')), `${name}: a refusal removes nothing`);
        // `gone` is what an ok row removes; a row refused here (ok-fold|refused-nofold on a case-sensitive OS) removes nothing.
        if (ok && goneRel !== '-') assert.ok(gone(join(b, goneRel)), `${name}: ${goneRel} is still there`);
        assert.ok(existsSync(join(b, 'outside', 'keep', 'f')), `${name}: the folder outside the root was touched`);
      } finally {
        rmSync(b, { recursive: true, force: true });
      }
    }
  });
}

// kept-clone.sh calls safe_rm inside a phase bounded by a limit (PADDOCK_KEPT_CLONE_TIMEOUT_<ID>): every program it starts per call
// costs a fork and an exec, which under load added up to more than the checkout phase's 1 s limit in its test before the phase reached
// its own git (4 of 40 runs at load 40 to 60; 0 of 40 with the plain `rm -rf` it replaced). The OS is asked once when the helper is
// sourced, and plain ASCII case is folded by the shell, so a call starts no program but `rm`.
for (const shell of SHELLS) {
  test(`${shell}: a call starts no program but rm, and folds case as before`, () => {
    const { base, root, home } = tree();
    try {
      const bin = join(base, 'bin');
      const log = join(base, 'calls');
      mkdirSync(bin);
      // A case-folding OS (Darwin) on every host, and a `tr` that logs each call before it does the work.
      for (const [name, body] of [
        ['uname', 'echo Darwin'],
        ['tr', `exec /usr/bin/env PATH=/usr/bin:/bin tr "$@"`],
        ['cygpath', 'echo cygpath-must-not-run >&2; exit 1'],
      ]) {
        writeFileSync(join(bin, name), `#!/bin/sh\necho ${name} >> '${log}'\n${body}\n`, { mode: 0o755 });
      }
      writeFileSync(log, '');
      const run = (body) =>
        spawnSync(shell, ['-c', `. '${LIB}' && : > '${log}' && ${body}`], {
          encoding: 'utf8',
          env: { ...process.env, HOME: home, PATH: `${bin}:${process.env.PATH}` },
        });
      const calls = () => readFileSync(log, 'utf8').split('\n').filter(Boolean);
      const three = `safe_rm '${root}' '${join(root, 'a', 'b', 'f')}' && safe_rm '${root}' '${join(root, 'a')}' && safe_rm '${root}' '${join(root, 'gone')}'`;
      let r = run(three);
      assert.equal(r.status, 0, r.stderr);
      assert.equal(existsSync(join(root, 'a')), false);
      assert.deepEqual(calls(), [], 'three calls whose names match the root started no uname, tr or cygpath');
      // Only the case differs: on a case-folding OS it is under the root (the parity table's ok-fold rows); plain ASCII is folded by
      // the shell, so still no program runs.
      r = run(`safe_rm '${root}' '${join(root.toUpperCase(), 'x')}'`);
      assert.equal(r.status, 0, `a case-only difference is under the root where case is ignored: ${r.stderr}`);
      assert.deepEqual(calls(), [], 'an ASCII case-only difference is folded without a program');
      r = run(`safe_rm '${root}' '${join(root, 'A')}' && safe_rm '${root}' '${join(home.toUpperCase(), 'x')}'`);
      assert.equal(r.status, 1, 'the home folder is not under the root, in any case');
      assert.match(r.stderr, /not strictly under/);
      // The home check folds case too: the home folder spelled in upper case is still the home folder, refused as a root, and plain
      // ASCII needs no program for that either.
      r = run(`safe_rm '${home.toUpperCase()}' '${join(home, 'x')}'`);
      assert.equal(r.status, 1, 'the home folder in another case is still the home folder');
      assert.match(r.stderr, /home folder or above it/);
      assert.deepEqual(calls(), [], 'the case-folded home check of plain ASCII starts no program');
      // A name beyond plain ASCII is left to `tr`, as before (whether it folds `Ä` depends on the host's tr and locale: GNU tr works
      // on bytes).
      run(`safe_rm '${join(root, 'Ä')}' '${join(root, 'ä', 'x')}'`);
      assert.ok(calls().length > 0 && calls().every((c) => c === 'tr'), `only tr folds the case: ${calls()}`);
    } finally {
      rmSync(base, { recursive: true, force: true });
    }
  });
}
