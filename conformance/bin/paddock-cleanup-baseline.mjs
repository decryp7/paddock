#!/usr/bin/env node
// Lists C115's sites: every unscoped cleanup under --root (default: the current folder), then the waived ones.
//   --shrink  rewrites conformance/baselines/C115.json with only the entries that still match a site (stale ones removed, nothing added).
//   --write   rewrites it from every site; refused when that would add an entry. `--init` allows it: the first run, or a deliberate
//             reset with the user's approval (the baseline may only shrink, and C115 compares it with the merge base's).
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { BASELINE_PATH, cleanupSites } from '../src/checks/C115-scoped-cleanup.mjs';
import { againstBaseline, shrinkBaseline } from '../src/checks/C71-test-timing.mjs';
import { openTree } from '../src/util/fs.mjs';

const args = process.argv.slice(2);
const val = (k) => (args.includes(k) ? args[args.indexOf(k) + 1] : undefined);
const root = resolve(val('--root') ?? '.');
const tree = openTree(root);
const { sites, waived, noReason } = cleanupSites(tree.files(), tree.read);
const file = join(root, BASELINE_PATH);
const old = existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : null;
const COMMENT = 'Known sites of C115 (a cleanup touches only its own items): `rm -r` on a bare variable in scripts that clean up their own folders, by file and hash of the normalised line (line is for reading only). May only shrink; scope the cleanup (an owned prefix, "${VAR:?}", mktemp) and delete its entry.';
const write = (entries, what) => {
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, `${JSON.stringify({ _comment: COMMENT, sites: entries }, null, 1)}\n`);
  console.log(`wrote ${entries.length} ${what} to ${BASELINE_PATH}`);
};
const reportMoved = () => {
  for (const { item, from } of againstBaseline(sites, old.sites, () => true).moved) console.log(`moved: ${from.file} -> ${item.file}  (same text; the entry's file is rewritten, not counted as growth)`);
};
if (args.includes('--shrink')) {
  if (!old) {
    console.error('no baseline to shrink');
    process.exit(2);
  }
  reportMoved();
  const kept = shrinkBaseline(sites, old.sites);
  write(kept, `known sites (${old.sites.length - kept.length} removed; ${againstBaseline(sites, old.sites, () => true).fresh.length} sites are not in the baseline and were not added)`);
  process.exit(0);
}
if (!args.includes('--write')) {
  for (const s of sites) console.log(`${s.file}:${s.line}  ${s.what}`);
  for (const s of waived) console.log(`${s.file}:${s.line}  (waived)`);
  for (const s of noReason) console.log(`${s.file}:${s.line}  (waiver without a reason)`);
  console.log(`${sites.length} sites, ${waived.length} waived, ${noReason.length} waivers without a reason`);
  process.exit(0);
}
if (old && !args.includes('--init') && againstBaseline(sites, old.sites, () => true).fresh.length) {
  console.error('refusing to grow the baseline: fix or waive the new sites (use --init only with the user\'s approval), or --shrink');
  process.exit(1);
}
write(sites.map(({ file: f, line, hash }) => ({ file: f, line, hash })), 'sites');
