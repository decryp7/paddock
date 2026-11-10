// Records what the JS conformance runner answers for the fixture cases (RT-5's oracle, until C39 and C53 leave the JS host):
//   node tools/paddock-dev/tests/fixtures/check/generate.mjs
// writes expected.json (the runner's result for C39 and C53, with the waivers) and masked.json (maskRust of every case).
// tests/it/check.rs materializes the same tree (`a__b__c.rs.txt` -> `a/b/c.rs`, `paddock.waivers.toml.txt` -> `paddock.waivers.toml`).
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../../../..');
const { runChecks } = await import(pathToFileURL(join(repo, 'conformance/src/runner.mjs')).href);
const { maskRust } = await import(pathToFileURL(join(repo, 'conformance/src/checks/C39-freed-port.mjs')).href);

const root = mkdtempSync(join(tmpdir(), 'rt5-fixture-'));
const masked = {};
for (const name of readdirSync(join(here, 'cases')).sort()) {
  const rel = name.replace(/\.txt$/, '').split('__').join('/');
  const text = readFileSync(join(here, 'cases', name), 'utf8');
  mkdirSync(dirname(join(root, rel)), { recursive: true });
  writeFileSync(join(root, rel), text);
  if (rel.endsWith('.rs')) masked[rel] = maskRust(text);
}
const result = await runChecks({ root, app: 'fixture', only: ['C39', 'C53'], options: { now: new Date('2026-11-10T12:00:00Z') } });
rmSync(root, { recursive: true, force: true });
writeFileSync(join(here, 'expected.json'), `${JSON.stringify(result, null, 2)}\n`);
writeFileSync(join(here, 'masked.json'), `${JSON.stringify(masked, null, 2)}\n`);
console.log(`${result.findings.length} findings, ${result.waived.length} waived, ${Object.keys(masked).length} masked files`);
