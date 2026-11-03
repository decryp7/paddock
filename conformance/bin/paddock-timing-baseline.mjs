#!/usr/bin/env node
// Lists (or with --write rewrites) the C71 baseline: every timing site in the tests under --root (default: the current folder).
// `--write` may only shrink an existing baseline unless `--init` is also given (the first run, or a deliberate reset with the
// user's approval); the date is kept from the existing file, or `--expires YYYY-MM-DD` on --init.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { BASELINE_PATH, againstBaseline, timingSites } from '../src/checks/C71-test-timing.mjs';
import { openTree } from '../src/util/fs.mjs';

const args = process.argv.slice(2);
const val = (k) => (args.includes(k) ? args[args.indexOf(k) + 1] : undefined);
const root = resolve(val('--root') ?? '.');
const tree = openTree(root);
const sites = timingSites(tree.files(), tree.read);
const file = join(root, BASELINE_PATH);
const old = existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : null;
const summary = {};
for (const s of sites) summary[s.file] = (summary[s.file] ?? 0) + 1;

if (!args.includes('--write')) {
  for (const s of sites) console.log(`${s.file}:${s.line}  ${s.what}`);
  console.log(`${sites.length} sites in ${Object.keys(summary).length} files`);
  process.exit(0);
}
if (old && !args.includes('--init') && againstBaseline(sites, old.sites).fresh.length) {
  console.error('refusing to grow the baseline: fix or waive the new sites (use --init only with the user\'s approval)');
  process.exit(1);
}
const expires = val('--expires') ?? old?.expires;
if (!expires) {
  console.error('--expires YYYY-MM-DD is needed for a new baseline');
  process.exit(2);
}
const doc = {
  _comment: 'Known timing sites of ADR 0041 / C71, by file and hash of the normalised line (line is for reading only). May only shrink; migrate a site by waiting on the condition and delete its entry.',
  expires,
  sites: sites.map(({ file: f, line, hash }) => ({ file: f, line, hash })),
};
mkdirSync(dirname(file), { recursive: true });
writeFileSync(file, `${JSON.stringify(doc, null, 1)}\n`);
console.log(`wrote ${sites.length} sites to ${BASELINE_PATH}`);
