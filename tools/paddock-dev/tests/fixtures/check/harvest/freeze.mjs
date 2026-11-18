import { statSync, readFileSync, existsSync, mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';
const [repo, old, list, out] = process.argv.slice(2);
const files = JSON.parse(readFileSync(list, 'utf8')).filter((f) => existsSync(join(repo, f)) && statSync(join(repo, f)).isFile());
const kept = [];
for (const f of files) { const b = readFileSync(join(repo, f)); if (b.includes(0)) continue; kept.push([f, b.toString('utf8')]); }
const dir = mkdtempSync(join(tmpdir(), 'frozen-'));
for (const [p, t] of kept) { mkdirSync(dirname(join(dir, p)), { recursive: true }); writeFileSync(join(dir, p), t); }
const { runChecks } = await import(pathToFileURL(join(old, 'conformance/src/runner.mjs')).href);
const ids = ['C39','C53','C119','C120','C121','C122','C123','C124','C125','C129','C130','C131'];
const r = await runChecks({ root: dir, app: 'paddock', only: ids, options: { now: new Date('2026-11-19T12:00:00Z') } });
rmSync(dir, { recursive: true, force: true });
writeFileSync(out, JSON.stringify({ files: kept, js: { app: r.app, checks: r.checks, findings: r.findings, waived: r.waived, errors: r.errors, warnings: r.warnings } }));
console.log(kept.length, 'files;', r.findings.length, 'findings,', r.waived.length, 'waived,', r.errors, 'errors');
