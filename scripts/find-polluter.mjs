#!/usr/bin/env node
// Finds the spec file that leaks state into a failing one (rule 58, ER-29). Dependency-free.
//
//   node scripts/find-polluter.mjs --victim <spec> --before <file> [--cwd DIR] [--timeout MS] [--repeat N]
//                                  [--no-flags] [--file-flag FLAG] -- <test cmd...>
//
// A full Vitest run (isolate: false, rule 49) failed on <victim>. <file> lists, one path per line, the spec files that ran
// before it in the same worker, in that order (take them from the failing run's log, which also prints the seed). The script
// runs <test cmd> with the candidates plus the victim, in that order, and bisects the candidates (delta debugging, one
// polluter assumed): victim + first half; if that fails, recurse there, otherwise the second half. About log2(n) + 2 runs.
//
// Vitest does NOT run files in the order given: BaseSequencer runs failed files first and sorts the rest by duration,
// shuffle reorders each subset, workers split the files, and file arguments are substring filters. So, unless --no-flags,
// each run appends `--no-file-parallelism --maxWorkers=1 --sequence.shuffle=false`, then the files as ABSOLUTE paths
// (resolved against --cwd), and sets PADDOCK_TEST_ORDER so the order-keeping sequencer runs exactly that order. The
// sequencer can only be set in the config (Vitest reads the CLI option as a string), so the project's Vitest config needs
// the one line from scripts/ordered-sequencer.mjs: `sequence: { ...orderedSequence() }` (empty unless the variable is set).
// Do not put those flags, a seed or a shuffle in <test cmd>.
//
// Under `ng test` (Angular unit-test builder) the flags above are not accepted: point the builder's Vitest config
// (`runnerConfig`) at a config that has the `orderedSequence()` line and sets `fileParallelism: false` and
// `maxWorkers: 1`, then run `--no-flags --file-flag --include -- ng test`, which passes each file as `--include <path>`.
// (Not verified against a real Angular workspace yet; check `ng test --help` of the installed version.)
//
// Assumptions: exactly one polluter (else status "combination"); the failure is deterministic for a given order (else use
// --repeat N: a probe counts as failing if any of N runs fails); the victim and polluter run in one worker and one process
// (isolate: false); setup files and config are unchanged between runs. --timeout MS kills a run that hangs (a leaked fake
// timer often hangs rather than fails) and counts it as a failure; the default is 600000.
//
// Exit 0: polluter found (printed). Exit 1: no single polluter (does not reproduce, fails alone, or needs two files).
// Exit 2: usage error. Each run is a child process with no shell; nothing is slept on.

import { spawnSync } from 'node:child_process';
import { readFileSync, realpathSync } from 'node:fs';
import { isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';


/**
 * Bisects `candidates` for the one file that makes `fails(files)` true together with the victim.
 * @param {string[]} candidates files that ran before the victim, in order
 * @param {(subset: string[]) => boolean} fails runs `subset` then the victim, true when the victim failed
 * @returns {{status: 'found', polluter: string, runs: number} | {status: 'no-repro' | 'fails-alone' | 'combination', remaining: string[], runs: number}}
 */
export function findPolluter(candidates, fails) {
  let runs = 0;
  const run = (subset) => {
    runs += 1;
    return fails(subset);
  };
  if (!run(candidates)) return { status: 'no-repro', remaining: candidates, runs };
  if (run([])) return { status: 'fails-alone', remaining: [], runs };
  let current = candidates;
  while (current.length > 1) {
    const mid = Math.ceil(current.length / 2);
    const first = current.slice(0, mid);
    const second = current.slice(mid);
    if (run(first)) current = first;
    else if (run(second)) current = second;
    else return { status: 'combination', remaining: current, runs };
  }
  return { status: 'found', polluter: current[0], runs };
}

/** Parses the argument list; returns `{error}` instead of throwing so the caller prints one clear line. */
export function parseArgs(argv) {
  const dash = argv.indexOf('--');
  const head = dash < 0 ? argv : argv.slice(0, dash);
  const cmd = dash < 0 ? [] : argv.slice(dash + 1);
  const opt = (name) => {
    const i = head.indexOf(`--${name}`);
    return i < 0 ? undefined : head[i + 1];
  };
  const victim = opt('victim');
  const before = opt('before');
  const usage = 'usage: find-polluter.mjs --victim <spec> --before <list-file> [--cwd DIR] [--timeout MS] [--repeat N] [--no-flags] [--file-flag FLAG] -- <test cmd...>';
  if (!victim || !before || cmd.length === 0) return { error: usage };
  const timeout = Number(opt('timeout') ?? 600000);
  const repeat = Number(opt('repeat') ?? 1);
  if (!Number.isInteger(timeout) || timeout <= 0 || !Number.isInteger(repeat) || repeat <= 0) {
    return { error: `--timeout and --repeat must be positive integers; ${usage}` };
  }
  return { victim, before, cwd: opt('cwd') ?? process.cwd(), timeout, repeat, noFlags: head.includes('--no-flags'), fileFlag: opt('file-flag'), cmd };
}

function main() {
  const parsed = parseArgs(process.argv.slice(2));
  if (parsed.error) {
    console.error(`::error::${parsed.error}`);
    return 2;
  }
  let candidates;
  try {
    candidates = readFileSync(parsed.before, 'utf8')
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l !== '' && l !== parsed.victim);
  } catch (e) {
    console.error(`::error::cannot read ${parsed.before}: ${e.message}; next step: write the files that ran before the victim, one per line`);
    return 2;
  }
  const [exe, ...base] = parsed.cmd;
  // Vitest matches file arguments against real paths, so a symlinked folder (macOS /var -> /private/var) must be resolved.
  const abs = (p) => {
    const full = isAbsolute(p) ? p : resolve(parsed.cwd, p);
    try {
      return realpathSync(full);
    } catch {
      return full;
    }
  };
  const victim = abs(parsed.victim);
  candidates = candidates.map(abs).filter((c) => c !== victim);
  const fails = (subset) => {
    const order = [...subset, victim];
    const files = parsed.fileFlag ? order.flatMap((f) => [parsed.fileFlag, f]) : order;
    const flags = parsed.noFlags
      ? []
      : ['--no-file-parallelism', '--maxWorkers=1', '--sequence.shuffle=false'];
    const env = { ...process.env, PADDOCK_TEST_ORDER: order.join('\n') };
    for (let i = 0; i < parsed.repeat; i += 1) {
      const r = spawnSync(exe, [...base, ...flags, ...files], { cwd: parsed.cwd, env, stdio: 'inherit', timeout: parsed.timeout });
      // A command that cannot start is a usage problem, not a failing victim: stop instead of bisecting noise.
      if (r.error && r.error.code !== 'ETIMEDOUT') throw r.error;
      if (r.error || r.status !== 0) return true;
    }
    return false;
  };
  const result = findPolluter(candidates, fails);
  switch (result.status) {
    case 'found':
      console.log(`polluter: ${result.polluter} (victim ${victim}, ${result.runs} runs)`);
      return 0;
    case 'no-repro':
      console.error('::error::the victim passes with all candidates before it; next step: use the seed and flags of the failing run');
      return 1;
    case 'fails-alone':
      console.error('::error::the victim fails alone, so it is not order dependent; next step: debug it on its own');
      return 1;
    default:
      console.error(`::error::no single polluter; the failure needs several of: ${result.remaining.join(', ')}`);
      return 1;
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) process.exit(main());
