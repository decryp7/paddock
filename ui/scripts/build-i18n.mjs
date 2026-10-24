// The translation files of the package and the shell demo (plan S15; `npm run i18n`, CI: `npm run i18n:check`):
//   1. the shell's `$localize` messages, extracted from the package build (dist/paddock-ui, so every entry point counts,
//      not only what one app imports), plus the shell's `msg('pd.…')` contributions scanned from the sources
//      -> projects/paddock-ui/i18n/messages.pd.xlf (the catalog translators edit; shipped in the package);
//   2. its pseudo-locale -> projects/paddock-ui/i18n/messages.pd.en-XA.json (shipped; apps merge it);
//   3. the demo's own `msg('sd.…')` catalog and its loader file for the pseudo-locale (the shell's + the demo's),
//      written by `paddock-i18n merge` exactly as an app does it.
// Run `npm run build` first (the extraction reads the built package). `--check` writes nothing and fails on drift.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { main as i18n } from '../projects/paddock-ui/bin/paddock-i18n.mjs';

const ui = join(dirname(fileURLToPath(import.meta.url)), '..');
const check = process.argv.includes('--check') ? ['--check'] : [];
const dist = join(ui, 'dist', 'paddock-ui');
if (!existsSync(join(dist, 'fesm2022'))) {
  console.error('i18n: dist/paddock-ui is missing: run `npm run build` first.');
  process.exit(1);
}
const work = join(ui, '.angular', 'i18n');
mkdirSync(work, { recursive: true });
const extracted = join(work, 'pd.extracted.json');
execFileSync(
  process.execPath,
  [join(ui, 'node_modules', '@angular', 'localize', 'tools', 'bundles', 'src', 'extract', 'cli.js'), '-r', dist, '-s', 'fesm2022/*.mjs', '-f', 'json', '-o', extracted, '--useSourceMaps=false', '-l', 'en-US', '-d', 'error', '--loglevel', 'warn'],
  { stdio: 'inherit' },
);

const pkg = join(ui, 'projects', 'paddock-ui');
const catalog = join(pkg, 'i18n', 'messages.pd.xlf');
const pseudo = join(pkg, 'i18n', 'messages.pd.en-XA.json');
const demo = join(ui, 'projects', 'shell-demo', 'src');
const demoCatalog = join(demo, 'locale', 'messages.xlf');
const demoPseudo = join(demo, 'locale', 'messages.en-XA.json');
mkdirSync(dirname(catalog), { recursive: true });
mkdirSync(dirname(demoCatalog), { recursive: true });
try {
  for (const args of [
    ['catalog', '--out', catalog, '--json', extracted, '--src', pkg, '--prefix', 'pd.'],
    ['merge', '--locale', 'en-XA', '--out', pseudo, catalog],
    ['catalog', '--out', demoCatalog, '--src', demo, '--prefix', 'sd.'],
    ['merge', '--locale', 'en-XA', '--out', demoPseudo, pseudo, demoCatalog],
  ])
    console.log(`i18n: ${i18n([...args, ...check])}`);
} catch (e) {
  console.error(`i18n: ${e instanceof Error ? e.message : String(e)}`);
  process.exit(1);
}
