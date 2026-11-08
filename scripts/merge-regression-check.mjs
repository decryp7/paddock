#!/usr/bin/env node
// Merge check for stale reverts (ER-27 style output): lists files where a merge's result goes backwards against its first parent.
//   node scripts/merge-regression-check.mjs [--repo <dir>] <merge-commit>
//   node scripts/merge-regression-check.mjs [--repo <dir>] --branch <branch> --target <target>
// A file whose result differs from the first parent is an ERROR when
//   (a) the branch never touched it (its blob equals the merge base's), so the merge itself changed it, or
//   (c) a non-merge branch commit sets it to an exact older blob of that path that is not its parent's blob (read from
//       `git log --raw` blob ids, no contents): the stale-tree signature. `Revert` commits are exempt.
// and a WARNING (::warning::, exit unchanged) when
//   (b) the result removes lines (and does not re-add them elsewhere) that make up at least half of what one earlier commit added
//       (at least 2 lines): a revert of it, or a deliberate rewrite, which looks the same.
// With --branch/--target the result is `git merge-tree --write-tree`'s tree, also on exit 1 (conflicts); the conflicted paths it lists
// are skipped and named, the rest is analysed.
// Why: on 2026-11-05 a branch commit built on a stale tree reverted a memory fix, a redaction and test fixes with no conflict (b48df1818).
// Exit 0 no error, 1 error found (one ::error:: line), 2 usage or git error. Octopus merges: only the second parent is compared.
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

const isRevert = (subject) => /^Revert\b/.test(subject);

/** Branch commits (merge base to `second`, no merges): sha, subject and the files each modified (old blob, new blob, path). */
function branchCommits(git, base, second) {
  const out = git('-c', 'core.quotePath=false', 'log', '--no-merges', '--raw', '--no-abbrev', '--format=%x01%H%x02%s', `${base}..${second}`);
  return out.split('\x01').filter(Boolean).map((chunk) => {
    const [head, ...rest] = chunk.split('\n');
    const [sha, subject] = head.split('\x02');
    const mods = [];
    for (const l of rest) {
      const m = /^:\d+ \d+ ([0-9a-f]{40}) ([0-9a-f]{40}) ([A-Z])\d*\t(.+)$/.exec(l);
      if (m) mods.push({ oldBlob: m[1], newBlob: m[2], status: m[3], file: m[4] });
    }
    return { sha, subject, mods };
  });
}

/**
 * Files where `result` goes backwards against `first` (the first parent); `second` is the merged branch tip. `skip` holds paths that
 * are conflicted in a merge-tree result (their content has conflict markers, so there is nothing to compare).
 * `errors` are certain enough to stop a merge: (a) the branch never touched the file, (c) a non-merge branch commit sets the file to an
 * exact older blob (not its parent's). `warnings` are only to read: (b) the result removes at least half of what one earlier commit
 * added (a deliberate rewrite looks the same). `Revert` commits of the branch are exempt from (b) and (c).
 */
export function analyze(git, first, second, result, skip = new Set()) {
  const base = git('merge-base', first, second).trim();
  const touchedByBranch = new Set(git('diff', '--name-only', '-z', base, second).split('\0').filter(Boolean));
  const files = git('diff', '--name-only', '-z', first, result).split('\0').filter((f) => f && !skip.has(f));
  const commits = branchCommits(git, base, second);
  const reverted = new Set(commits.filter((c) => isRevert(c.subject)).flatMap((c) => c.mods.map((m) => m.file)));
  const errors = [];
  const warnings = [];
  const inResult = new Set(files);
  // (c) a branch commit sets a file to an exact older blob of that path, read from blob ids alone.
  const staleSeen = new Set();
  for (const c of commits) {
    if (isRevert(c.subject)) continue;
    for (const m of c.mods) {
      if (m.status !== 'M' || !inResult.has(m.file) || staleSeen.has(m.file)) continue;
      const earlier = git('-c', 'core.quotePath=false', 'log', '--raw', '--no-abbrev', '--format=%x01%H', '-n', String(DEPTH), `${c.sha}^`, '--', m.file);
      let at;
      for (const chunk of earlier.split('\x01').filter(Boolean)) {
        const sha = chunk.slice(0, 9);
        if (new RegExp(`^:\\d+ \\d+ [0-9a-f]{40} ${m.newBlob} `, 'm').test(chunk)) { at = sha; break; }
      }
      if (at && m.newBlob !== m.oldBlob) {
        staleSeen.add(m.file);
        errors.push({ file: m.file, reason: `commit ${c.sha.slice(0, 9)} sets the file back to an older version (the exact blob it had at ${at}), not to its parent's` });
      }
    }
  }
  for (const file of files) {
    if (staleSeen.has(file)) continue;
    if (!touchedByBranch.has(file)) {
      errors.push({ file, reason: 'the branch never touched it, but the merge changed it against the first parent' });
      continue;
    }
    if (reverted.has(file)) continue;
    const { added, removed } = changedLines(git('diff', '-U0', '--no-color', first, result, '--', file));
    const readded = counts(added);
    const gone = counts(removed.filter((l) => !trivial(l)));
    for (const [l, n] of readded) if (gone.has(l)) gone.set(l, Math.max(0, gone.get(l) - n));
    if ([...gone.values()].every((n) => n === 0)) continue;
    const log = git('log', '--no-merges', '-n', String(DEPTH), '--format=%x01%h %s', '-U0', '--no-color', '-p', first, '--', file);
    for (const chunk of log.split('\x01').filter(Boolean)) {
      const head = chunk.slice(0, chunk.indexOf('\n')).trim();
      const sha = head.split(' ')[0];
      if (isRevert(head.slice(sha.length + 1))) continue;
      const mine = changedLines(chunk).added.filter((l) => !trivial(l));
      const back = mine.filter((l) => (gone.get(l) ?? 0) > 0);
      if (back.length >= MIN_LINES && back.length >= MIN_SHARE * mine.length) {
        warnings.push({ file, reason: `removes ${back.length} of the ${mine.length} lines commit ${sha} added (a revert of it, or a rewrite?)` });
        break;
      }
    }
  }
  return { base, errors, warnings };
}

/**
 * The merge result of `first` and `second` without a merge commit. `git merge-tree --write-tree` exits 1 on conflicts but still prints
 * the tree it wrote (conflicted files hold conflict markers) and lists the conflicted paths; exit 2 or more is a real failure.
 */
export function mergeTree(repo, first, second) {
  const mt = spawnSync('git', ['-C', repo, 'merge-tree', '--write-tree', '--name-only', '-z', first, second], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
  if (mt.error || (mt.status !== 0 && mt.status !== 1)) throw new Error(`git merge-tree: ${mt.error?.message ?? mt.stderr.trim()}`);
  const [tree, ...rest] = mt.stdout.split('\0');
  const conflicted = [];
  for (const name of rest) {
    if (name === '') break;
    conflicted.push(name);
  }
  if (!/^[0-9a-f]{40,64}$/.test(tree)) throw new Error(`git merge-tree printed no tree id (exit ${mt.status}): ${mt.stderr.trim()}`);
  return { tree, conflicted: new Set(conflicted) };
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
  // Paths in git's output are relative to the folder it runs in: run it at the top level whatever --repo names.
  const top = spawnSync('git', ['-C', repo, 'rev-parse', '--show-toplevel'], { encoding: 'utf8' });
  if (top.status === 0) repo = top.stdout.trim();
  const git = makeGit(repo);
  let first;
  let second;
  let result;
  let label;
  let conflicted = new Set();
  try {
    if (branch && target) {
      first = git('rev-parse', target).trim();
      second = git('rev-parse', branch).trim();
      ({ tree: result, conflicted } = mergeTree(repo, first, second));
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
    const { base, errors, warnings } = analyze(git, first, second, result, conflicted);
    if (conflicted.size > 0) {
      console.log(`  conflicted in the merge, not checked (resolve by hand, then run this on the merge commit): ${[...conflicted].join(', ')}`);
    }
    for (const f of errors) console.log(`  error ${f.file}: ${f.reason}`);
    for (const f of warnings) console.log(`  warning ${f.file}: ${f.reason}`);
    if (warnings.length > 0) {
      console.log(`::warning::${label}: ${warnings.length} file(s) remove most of what an earlier commit added (${warnings.map((f) => f.file).join(', ')}); a deliberate rewrite looks the same. Next step: read each (git diff ${first.slice(0, 9)} <result> -- <file>) and confirm it is meant.`);
    }
    if (errors.length === 0) {
      console.log(`merge-regression-check: ${label}: no file certainly goes backwards (base ${base.slice(0, 9)})`);
      return 0;
    }
    const names = errors.map((f) => f.file).join(', ');
    console.log(`::error::${label}: ${errors.length} file(s) go backwards against the first parent (${names}); the branch was probably built on a stale tree. Next step: diff each against the first parent (git diff ${first.slice(0, 9)} <result> -- <file>), restore what was not meant to change, and merge the current target into the branch before merging again.`);
    return 1;
  } catch (e) {
    fail(`${e.message}. Next step: check the revisions exist.`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) process.exit(main(process.argv.slice(2)));
