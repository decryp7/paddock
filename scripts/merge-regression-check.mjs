#!/usr/bin/env node
// Merge check for stale reverts (ER-27 style output): lists files where a merge's result goes backwards against its first parent.
//   node scripts/merge-regression-check.mjs [--repo <dir>] <merge-commit>
//   node scripts/merge-regression-check.mjs [--repo <dir>] --branch <branch> --target <target>
// A file is flagged when the result differs from the first parent and either
//   (a) the branch did not touch it (its blob equals the merge base's), so the merge itself changed it, or
//   (b) the result removes lines (and does not re-add them elsewhere in the file) that make up at least half of what one earlier
//       commit added to the file (at least 2 lines): a revert of that commit, as when a branch commit is built on a stale tree.
// Why: on 2026-11-05 a branch commit built on a stale tree reverted a memory fix, a redaction and test fixes with no conflict (b48df1818).
// Exit 0 clean, 1 flagged (one ::error:: line), 2 usage or git error. Octopus merges: only the second parent is compared.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const DEPTH = 50; // commits touching a file that are searched for the commit being reverted
const MIN_LINES = 2;
const MIN_SHARE = 0.5;
const trivial = (l) => l.trim().length < 4; // blank lines, braces: never evidence

function makeGit(repo) {
  return (...args) => {
    const r = spawnSync('git', ['-C', repo, ...args], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
    if (r.error) throw new Error(`git ${args[0]}: ${r.error.message}`);
    if (r.status !== 0) throw new Error(`git ${args.join(' ')}: ${r.stderr.trim()}`);
    return r.stdout;
  };
}

function changedLines(patch) {
  const added = [];
  const removed = [];
  for (const l of patch.split('\n')) {
    if (l.startsWith('+') && !l.startsWith('+++')) added.push(l.slice(1));
    else if (l.startsWith('-') && !l.startsWith('---')) removed.push(l.slice(1));
  }
  return { added, removed };
}

const counts = (arr) => arr.reduce((m, l) => m.set(l, (m.get(l) ?? 0) + 1), new Map());

/** Files where `result` goes backwards against `first` (the first parent); `second` is the merged branch tip. */
export function analyze(git, first, second, result) {
  const base = git('merge-base', first, second).trim();
  const touchedByBranch = new Set(git('diff', '--name-only', '-z', base, second).split('\0').filter(Boolean));
  const files = git('diff', '--name-only', '-z', first, result).split('\0').filter(Boolean);
  const flagged = [];
  for (const file of files) {
    if (!touchedByBranch.has(file)) {
      flagged.push({ file, reason: 'the branch never touched it, but the merge changed it against the first parent' });
      continue;
    }
    const { added, removed } = changedLines(git('diff', '-U0', '--no-color', first, result, '--', file));
    const readded = counts(added);
    const gone = counts(removed.filter((l) => !trivial(l)));
    for (const [l, n] of readded) if (gone.has(l)) gone.set(l, Math.max(0, gone.get(l) - n));
    if ([...gone.values()].every((n) => n === 0)) continue;
    const log = git('log', '--no-merges', '-n', String(DEPTH), '--format=%x01%h', '-U0', '--no-color', '-p', first, '--', file);
    for (const chunk of log.split('\x01').filter(Boolean)) {
      const sha = chunk.slice(0, chunk.indexOf('\n')).trim();
      const mine = changedLines(chunk).added.filter((l) => !trivial(l));
      const back = mine.filter((l) => (gone.get(l) ?? 0) > 0);
      if (back.length >= MIN_LINES && back.length >= MIN_SHARE * mine.length) {
        flagged.push({ file, reason: `removes ${back.length} of the ${mine.length} lines commit ${sha} added (a revert of it?)` });
        break;
      }
    }
  }
  return { base, flagged };
}

function fail(msg, code = 2) {
  console.log(`::error::merge-regression-check: ${msg}`);
  process.exit(code);
}

function main(argv) {
  let repo = '.';
  let branch;
  let target;
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--repo') repo = argv[++i];
    else if (argv[i] === '--branch') branch = argv[++i];
    else if (argv[i] === '--target') target = argv[++i];
    else rest.push(argv[i]);
  }
  const git = makeGit(repo);
  let first;
  let second;
  let result;
  let label;
  try {
    if (branch && target) {
      first = git('rev-parse', target).trim();
      second = git('rev-parse', branch).trim();
      const mt = spawnSync('git', ['-C', repo, 'merge-tree', '--write-tree', first, second], { encoding: 'utf8' });
      if (mt.status !== 0) fail(`${branch} does not merge cleanly into ${target}, so there is no result to check. Next step: resolve the conflicts in a real merge, then run this on the merge commit.`);
      result = mt.stdout.split('\n')[0].trim();
      label = `${branch} into ${target}`;
    } else if (rest.length === 1) {
      const parents = git('rev-list', '--parents', '-n', '1', rest[0]).trim().split(' ');
      if (parents.length < 3) fail(`${rest[0]} is not a merge commit; pass a merge commit, or --branch and --target. Next step: give the merge commit hash.`);
      [, first, second] = parents;
      result = git('rev-parse', `${rest[0]}^{tree}`).trim();
      label = `merge ${rest[0]}`;
    } else {
      fail('usage: merge-regression-check.mjs [--repo <dir>] <merge-commit> | --branch <b> --target <t>. Next step: pass one of the two forms.');
    }
    const { base, flagged } = analyze(git, first, second, result);
    for (const f of flagged) console.log(`  ${f.file}: ${f.reason}`);
    if (flagged.length === 0) {
      console.log(`merge-regression-check: ${label}: no file goes backwards (base ${base.slice(0, 9)})`);
      return 0;
    }
    const names = flagged.map((f) => f.file).join(', ');
    console.log(`::error::${label}: ${flagged.length} file(s) go backwards against the first parent (${names}); the branch was probably built on a stale tree. Next step: diff each against the first parent (git diff ${first.slice(0, 9)} <result> -- <file>), restore what was not meant to change, and merge the current target into the branch before merging again.`);
    return 1;
  } catch (e) {
    fail(`${e.message}. Next step: check the revisions exist (and that --branch merges cleanly into --target).`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) process.exit(main(process.argv.slice(2)));
