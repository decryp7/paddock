#!/usr/bin/env node
// C129's baseline: the shell and Python files of the tree that no exception covers, each with its line count, one entry per line,
// sorted by path (conformance/shell-baseline.json). Every work package of docs/plans/shell-to-rust.md removes its own lines.
//   (no flag)  lists the files and counts, writes nothing.
//   --shrink   rewrites the baseline with only the entries whose file is still shell, each lowered to its current line count (stale
//              entries removed; nothing added, nothing raised).
//   --init     writes it from every file: the first run, or a reset with the user's approval (the baseline may only shrink).
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { BASELINE, EXCEPTIONS, parseBaseline, parseExceptions, shellFiles } from '../src/checks/C129-no-new-shell.mjs';
import { openTree } from '../src/util/fs.mjs';

const args = process.argv.slice(2);
const root = resolve(args.includes('--root') ? args[args.indexOf('--root') + 1] : '.');
const tree = openTree(root);
const { shell, unread } = shellFiles(tree.files(), tree.read, parseExceptions(readFileSync(join(root, EXCEPTIONS), 'utf8')));
if (unread.length) {
  console.error(`cannot read ${unread.length} file(s), for example ${unread[0]}: fix that first`);
  process.exit(2);
}
const file = join(root, BASELINE);
const COMMENT = 'Shell and Python files of the tree that no exception covers (C129), with their line counts. May only shrink: port the script to Rust, delete its entry. Regenerate with: node conformance/bin/paddock-shell-baseline.mjs --shrink';
const write = (entries) => {
  const rows = entries.map((e) => `  ${JSON.stringify({ path: e.path, lines: e.lines })}`);
  writeFileSync(file, `{\n "_comment": ${JSON.stringify(COMMENT)},\n "files": [\n${rows.join(',\n')}\n ]\n}\n`);
  console.log(`wrote ${entries.length} entries to ${BASELINE}`);
};
const current = new Map(shell.map((s) => [s.file, s.lines]));
if (args.includes('--init')) {
  write(shell.map((s) => ({ path: s.file, lines: s.lines })));
} else if (args.includes('--shrink')) {
  if (!existsSync(file)) {
    console.error(`${BASELINE} does not exist: --init writes it the first time`);
    process.exit(2);
  }
  const old = parseBaseline(readFileSync(file, 'utf8'));
  const kept = old.filter((e) => current.has(e.path)).map((e) => ({ path: e.path, lines: Math.min(e.lines, current.get(e.path)) }));
  const fresh = shell.filter((s) => !old.some((e) => e.path === s.file)).length;
  write(kept.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0)));
  console.log(`${old.length - kept.length} removed; ${fresh} shell files are not in the baseline and were not added`);
} else {
  for (const s of shell) console.log(`${s.lines}\t${s.file}`);
  console.log(`${shell.length} shell files, ${shell.reduce((n, s) => n + s.lines, 0)} lines`);
}
