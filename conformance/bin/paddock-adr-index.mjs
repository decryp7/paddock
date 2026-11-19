#!/usr/bin/env node
// Rewrites the ADR index of docs/adr/README.md from the ADR files (src/adr-index.mjs says how).
//
//   node conformance/bin/paddock-adr-index.mjs [--root <repo>]          rewrite the index between its markers
//   node conformance/bin/paddock-adr-index.mjs --check [--root <repo>]  exit 1 and print the difference when it is stale
//
// A rewrite is refused when CI=true (CI checks, it never regenerates).
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { README_PATH, REGENERATE, adrFiles, entryOf, readmeText } from '../src/adr-index.mjs';
import { lineDiff } from '../src/derived-lists.mjs';

const USAGE = `usage: ${REGENERATE} [--check] [--root <repo>]`;
const fail = (code, msg) => {
  console.error(`::error::paddock-adr-index: ${msg}`);
  process.exit(code);
};

const args = process.argv.slice(2);
let check = false;
let root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--check') check = true;
  else if (args[i] === '--root') {
    const v = args[++i];
    if (!v || v.startsWith('--')) fail(2, `--root needs a folder (${USAGE})`);
    root = resolve(v);
  } else fail(2, `unknown argument ${args[i]} (${USAGE})`);
}
if (!check && process.env.CI === 'true') fail(2, `CI=true: CI checks the index (--check) and never rewrites it; run \`${REGENERATE}\` on your machine and commit the result`);

try {
  const dir = join(root, 'docs/adr');
  const entries = adrFiles(readdirSync(dir)).map((f) => entryOf(f, readFileSync(join(dir, f), 'utf8')));
  if (!entries.length) throw new Error('docs/adr has no ADR files: nothing was measured');
  const have = readFileSync(join(root, README_PATH), 'utf8');
  const want = readmeText(have, entries);
  if (have === want) console.log(`adr index: current (${entries.length} ADRs)`);
  else if (check) {
    console.error(`::error file=${README_PATH}::${README_PATH} is stale: run \`${REGENERATE}\` and commit the result`);
    for (const l of lineDiff(have, want)) console.error(`  ${l}`);
    process.exit(1);
  } else {
    writeFileSync(join(root, README_PATH), want);
    console.log(`wrote ${README_PATH} (${entries.length} ADRs)`);
  }
} catch (e) {
  fail(1, `${e.message}. Next step: fix what it names, then run \`${REGENERATE}\``);
}
