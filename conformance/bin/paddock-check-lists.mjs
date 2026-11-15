#!/usr/bin/env node
// Rewrites the lists that name every conformance check from the check modules (src/derived-lists.mjs says which and how).
//
//   node conformance/bin/paddock-check-lists.mjs [--root <repo>]           rewrite index.mjs and paddock.ci.json's `only`
//   node conformance/bin/paddock-check-lists.mjs --check [--root <repo>]   exit 1 and print the difference when either is stale
//
// Each module is imported once to make sure the id it exports is the id in its file name.
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { REGENERATE, generate, lineDiff, modules } from '../src/derived-lists.mjs';

const args = process.argv.slice(2);
const check = args.includes('--check');
const at = args.indexOf('--root');
const root = at >= 0 ? resolve(args[at + 1] ?? '.') : resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const unknown = args.filter((a, i) => a !== '--check' && a !== '--root' && args[i - 1] !== '--root');
if (unknown.length) {
  console.error(`::error::paddock-check-lists: unknown argument ${unknown[0]} (usage: ${REGENERATE} [--check] [--root <repo>])`);
  process.exit(2);
}

try {
  const dir = join(root, 'conformance/src/checks');
  const files = readdirSync(dir);
  for (const m of modules(files)) {
    const mod = await import(pathToFileURL(join(dir, m.file)).href);
    if (mod.default?.id !== m.id) {
      throw new Error(`conformance/src/checks/${m.file} exports id ${JSON.stringify(mod.default?.id)}, not ${m.id}: the file name and the id must agree`);
    }
  }
  const out = generate(files, readFileSync(join(root, 'conformance/paddock.ci.json'), 'utf8'));
  let stale = 0;
  for (const { path, text } of out) {
    const file = join(root, path);
    let have = '';
    try {
      have = readFileSync(file, 'utf8');
    } catch {
      // a missing file is stale
    }
    if (have === text) continue;
    if (check) {
      stale++;
      console.error(`::error file=${path}::${path} is stale: run \`${REGENERATE}\` and commit the result`);
      for (const l of lineDiff(have, text)) console.error(`  ${l}`);
    } else {
      writeFileSync(file, text);
      console.log(`wrote ${path}`);
    }
  }
  if (check && stale) process.exit(1);
  if (check) console.log(`check lists: current (${modules(files).length} checks)`);
} catch (e) {
  console.error(`::error::paddock-check-lists: ${e.message}. Next step: fix what it names, then run \`${REGENERATE}\`.`);
  process.exit(1);
}
