#!/usr/bin/env node
// Rewrites the lists that name every conformance check from the check modules (src/derived-lists.mjs says which and how).
//
//   node conformance/bin/paddock-check-lists.mjs [--root <repo>]           rewrite index.mjs, paddock.ci.json's `only` and
//                                                                          the README's check index; print what changed
//   node conformance/bin/paddock-check-lists.mjs --check [--root <repo>]   exit 1 and print the difference when one is stale
//
// Each module is imported once: its exported id must be the id in its file name, and its title, scope, mode and needs fill
// the README index. A rewrite is refused when CI=true (CI checks, it never regenerates).
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { CONTRACT_PATH, README_PATH, REGENERATE, generate, lineDiff, modules } from '../src/derived-lists.mjs';

const USAGE = `usage: ${REGENERATE} [--check] [--root <repo>]`;
const fail = (code, msg) => {
  console.error(`::error::paddock-check-lists: ${msg}`);
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
if (!check && process.env.CI === 'true') fail(2, `CI=true: CI checks the lists (--check) and never rewrites them; run \`${REGENERATE}\` on your machine and commit the result`);

try {
  const dir = join(root, 'conformance/src/checks');
  const files = readdirSync(dir, { withFileTypes: true })
    .filter((e) => !e.isDirectory())
    .map((e) => e.name);
  const metas = [];
  for (const m of modules(files)) {
    const mod = (await import(pathToFileURL(join(dir, m.file)).href)).default;
    if (mod?.id !== m.id) {
      throw new Error(`conformance/src/checks/${m.file} exports id ${JSON.stringify(mod?.id)}, not ${m.id}: the file name and the id must agree`);
    }
    metas.push({ id: mod.id, title: mod.title, scope: mod.scope, mode: mod.mode, needs: mod.needs });
  }
  const read = (p) => {
    try {
      return readFileSync(join(root, p), 'utf8');
    } catch {
      return '';
    }
  };
  const out = generate(files, read(CONTRACT_PATH), read(README_PATH), metas);
  let stale = 0;
  for (const { path, text } of out) {
    const have = read(path);
    if (have === text) continue;
    stale++;
    if (check) console.error(`::error file=${path}::${path} is stale: run \`${REGENERATE}\` and commit the result`);
    else {
      writeFileSync(join(root, path), text);
      console.log(`wrote ${path}`);
    }
    for (const l of lineDiff(have, text)) (check ? console.error : console.log)(`  ${l}`);
  }
  if (check && stale) process.exit(1);
  if (!stale) console.log(`check lists: current (${metas.length} checks)`);
} catch (e) {
  fail(1, `${e.message}. Next step: fix what it names, then run \`${REGENERATE}\``);
}
