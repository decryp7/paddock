#!/usr/bin/env node
// Lists the C71 baseline's candidates: every timing site in the tests under --root (default: the current folder).
//   --shrink  rewrites the baseline with only the entries that still match a site (stale ones removed, nothing added): always allowed.
//   --write   rewrites it from every site; refused when that would add an entry. `--init` allows it: the first run, or a deliberate
//             reset with the user's approval (the baseline may only shrink, and C71 compares it with the merge base's).
//   --races   the same for the state-race baseline (C71-races.json).
//   --waits   works on the fixed-budget baseline (C71-waits.json: Durations and timeouts given to waits) instead of the sleeps.
// There is no expiry: ADR 0041 WP-6 is open-ended.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { BASELINE_PATH, WAITS_BASELINE_PATH, RACES_BASELINE_PATH, againstBaseline, raceSites, shrinkBaseline, timingSites, waitSites } from '../src/checks/C71-test-timing.mjs';
import { openTree } from '../src/util/fs.mjs';

const args = process.argv.slice(2);
const val = (k) => (args.includes(k) ? args[args.indexOf(k) + 1] : undefined);
const root = resolve(val('--root') ?? '.');
const tree = openTree(root);
const waits = args.includes('--waits');
const races = args.includes('--races');
const PATH = races ? RACES_BASELINE_PATH : waits ? WAITS_BASELINE_PATH : BASELINE_PATH;
const sites = (races ? raceSites : waits ? waitSites : timingSites)(tree.files(), tree.read);
const file = join(root, PATH);
const old = existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : null;
const summary = {};
for (const s of sites) summary[s.file] = (summary[s.file] ?? 0) + 1;

const write = (entries, what) => {
  const doc = {
    _comment: races
      ? 'Known state races of ER-21 / C71 (an assertion on a field the wait before it did not read; a copy of the current status after a timer wait), by file and hash of the normalised line. May only shrink; wait for the field itself and delete the entry.'
      : waits
      ? 'Known fixed budgets of ADR 0071 / C71 (a Duration or timeout under 40 s given to a wait or a start setting), by file and hash of the normalised line (line is for reading only). May only shrink; migrate a site to the condition wait (ready or exit) and delete its entry.'
      : 'Known timing sites of ADR 0041 / C71, by file and hash of the normalised line (line is for reading only). May only shrink; migrate a site by waiting on the condition and delete its entry.',
    sites: entries,
  };
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, `${JSON.stringify(doc, null, 1)}\n`);
  console.log(`wrote ${entries.length} ${what} to ${PATH}`);
};
const reportMoved = () => {
  for (const { item, from } of againstBaseline(sites, old.sites).moved) console.log(`moved: ${from.file} -> ${item.file}  (same text; the entry's file is rewritten, not counted as growth)`);
};
if (args.includes('--shrink')) {
  if (!old) {
    console.error('no baseline to shrink');
    process.exit(2);
  }
  reportMoved();
  const fresh = againstBaseline(sites, old.sites).fresh;
  write(shrinkBaseline(sites, old.sites), `known sites (${old.sites.length - shrinkBaseline(sites, old.sites).length} removed; ${fresh.length} sites are not in the baseline and were not added)`);
  process.exit(0);
}
if (!args.includes('--write')) {
  for (const s of sites) console.log(`${s.file}:${s.line}  ${s.what}`);
  console.log(`${sites.length} sites in ${Object.keys(summary).length} files`);
  process.exit(0);
}
if (old && !args.includes('--init') && againstBaseline(sites, old.sites).fresh.length) {
  console.error('refusing to grow the baseline: fix or waive the new sites (use --init only with the user\'s approval), or --shrink');
  process.exit(1);
}
write(sites.map(({ file: f, line, hash }) => ({ file: f, line, hash })), 'sites');
