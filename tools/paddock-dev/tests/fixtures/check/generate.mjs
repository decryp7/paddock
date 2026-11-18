// Records maskRust (the JS helper that the checks still on the JS host share) of every fixture case:
//   node tools/paddock-dev/tests/fixtures/check/generate.mjs
// writes masked.json. expected.json (the JS runner's result for C39 and C53, with the waivers) was written by this script before the
// JS modules of both checks were retired (pair P3, ADR 0022 amendment 2026-11-19); it is a recorded answer now and does not change.
// tests/it/check.rs materializes the same tree (`a__b__c.rs.txt` -> `a/b/c.rs`, `paddock.waivers.toml.txt` -> `paddock.waivers.toml`).
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../../../..');
const { maskRust } = await import(pathToFileURL(join(repo, 'conformance/src/util/mask-rust.mjs')).href);

const root = mkdtempSync(join(tmpdir(), 'rt5-fixture-'));
const masked = {};
for (const name of readdirSync(join(here, 'cases')).sort()) {
  const rel = name.replace(/\.txt$/, '').split('__').join('/');
  const text = readFileSync(join(here, 'cases', name), 'utf8');
  mkdirSync(dirname(join(root, rel)), { recursive: true });
  writeFileSync(join(root, rel), text);
  if (rel.endsWith('.rs')) masked[rel] = maskRust(text);
}
rmSync(root, { recursive: true, force: true });
writeFileSync(join(here, 'masked.json'), `${JSON.stringify(masked, null, 2)}\n`);
console.log(`${Object.keys(masked).length} masked files`);
