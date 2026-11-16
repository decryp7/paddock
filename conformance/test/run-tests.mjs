// `node test/run-tests.mjs --test <patterns>` runs `node --test <patterns>` (the conformance tests' `npm test`), with an optional
// cap on the test processes: PADDOCK_TEST_CONCURRENCY=<n> puts `--test-concurrency=<n>` right after `--test`. The push gate sets
// it so these tests do not start one process per core beside its Clippy rows (ER-24). Node refuses the flag in NODE_OPTIONS and
// ignores it after the file patterns (measured 2026-11-15), hence this script. The patterns stay in package.json, where
// test-files-run.test.mjs reads them. Unset (CI's conformance job), node chooses as before.
import { spawnSync } from 'node:child_process';
import { isMain } from '../src/util/is-main.mjs';

/** The arguments after `node` for `argv` (starting with `--test`) and `env`; throws on a bad PADDOCK_TEST_CONCURRENCY. */
export function testArgs(argv, env) {
  if (argv[0] !== '--test') throw new Error('usage: node test/run-tests.mjs --test <patterns>');
  const n = env.PADDOCK_TEST_CONCURRENCY;
  if (n === undefined || n === '') return [...argv];
  if (!/^[1-9][0-9]*$/.test(n)) throw new Error(`PADDOCK_TEST_CONCURRENCY must be a positive whole number (got "${n}"); set it to the test processes to allow, or unset it`);
  return ['--test', `--test-concurrency=${n}`, ...argv.slice(1)];
}

/** `env` without the variables that say "this is CI": a test passes `options.ci` when it needs either answer, so the tests give the same result on a laptop and in a CI job (the growth checks turn a target they cannot see into an error in CI). */
export function testEnv(env) {
  const e = { ...env };
  for (const k of ['CI', 'GITEA_ACTIONS', 'GITHUB_ACTIONS']) delete e[k];
  return e;
}

if (isMain(import.meta.url)) {
  let args;
  try {
    args = testArgs(process.argv.slice(2), process.env);
  } catch (e) {
    console.error(`::error::${e.message}`);
    process.exit(2);
  }
  const r = spawnSync(process.execPath, args, { stdio: 'inherit', env: testEnv(process.env) });
  if (r.error) {
    console.error(`::error::node --test did not start: ${r.error.message}`);
    process.exit(1);
  }
  process.exit(r.status ?? 1);
}
