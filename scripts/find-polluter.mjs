#!/usr/bin/env node
// Finds the spec file that leaks state into a failing one (rule 58, ER-29). Dependency-free.
//
//   node scripts/find-polluter.mjs --victim <spec> --before <file> [--cwd DIR] -- <test cmd...>
//
// A full Vitest run (isolate: false, rule 49) failed on <victim>. <file> lists, one path per line, the spec files that ran
// before it in the same worker, in that order (take them from the failing run's log; print the seed there with
// `--sequence.seed`, and pass the same flags in <test cmd>). The script runs the command with the candidates plus the victim
// appended as arguments, always in the given order, and bisects the candidates (delta debugging, one polluter assumed):
// victim + first half; if that fails, recurse there, otherwise the second half. About log2(n) + 2 runs.
//
// Exit 0: polluter found (printed). Exit 1: no single polluter (does not reproduce, fails alone, or needs two files).
// Exit 2: usage error. Each run is a child process with no shell; nothing is slept on.

import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
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
  if (!victim || !before || cmd.length === 0) {
    return { error: 'usage: find-polluter.mjs --victim <spec> --before <list-file> [--cwd DIR] -- <test cmd...>' };
  }
  return { victim, before, cwd: opt('cwd') ?? process.cwd(), cmd };
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
  const fails = (subset) => {
    const r = spawnSync(exe, [...base, ...subset, parsed.victim], { cwd: parsed.cwd, stdio: 'inherit' });
    if (r.error) throw r.error;
    return r.status !== 0;
  };
  const result = findPolluter(candidates, fails);
  switch (result.status) {
    case 'found':
      console.log(`polluter: ${result.polluter} (victim ${parsed.victim}, ${result.runs} runs)`);
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
