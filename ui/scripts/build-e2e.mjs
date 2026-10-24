// Builds @decryp7/paddock-e2e into dist/paddock-e2e: compiles projects/paddock-e2e/src with tsc (ES modules and
// declarations) and copies package.json, README and the licences next to it, ready for `npm pack`.
import { execFileSync } from 'node:child_process';
import { copyFileSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ui = join(dirname(fileURLToPath(import.meta.url)), '..');
const project = join(ui, 'projects/paddock-e2e');
const dist = join(ui, 'dist/paddock-e2e');
const tsc = createRequire(import.meta.url).resolve('typescript/bin/tsc');

rmSync(dist, { recursive: true, force: true });
execFileSync(process.execPath, [tsc, '-p', join(project, 'tsconfig.json')], { cwd: ui, stdio: 'inherit' });
copyFileSync(join(project, 'package.json'), join(dist, 'package.json'));
copyFileSync(join(project, 'README.md'), join(dist, 'README.md'));
for (const f of ['LICENSE-MIT', 'LICENSE-APACHE']) copyFileSync(join(ui, '..', f), join(dist, f));
console.log(`built @decryp7/paddock-e2e into ${dist}`);
