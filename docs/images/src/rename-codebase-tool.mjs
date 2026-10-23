// Renames the codebase tool everywhere Paddock mentions it: node docs/images/src/rename-codebase-tool.mjs <NewName>
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { CODEBASE_TOOL } from './family.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const next = process.argv[2];
if (!next || !/^[A-Za-z][A-Za-z0-9]*$/.test(next)) { console.error('usage: rename-codebase-tool.mjs <NewName>'); process.exit(2); }
const files = [join(here, 'family.mjs'), join(here, '..', '..', '..', 'README.md'), join(here, '..', '..', 'ecosystem.md')];
for (const f of files) writeFileSync(f, readFileSync(f, 'utf8').replaceAll(CODEBASE_TOOL, next));
for (const s of ['ecosystem.mjs', 'integrations.mjs']) execFileSync('node', [join(here, s), '--png'], { stdio: 'inherit' });
