// C32: the translation catalogs are current (plan X48, decision D10). The app declares its build in `paddock-i18n.json`
// (see @decryp7/paddock-ui bin/i18n-build.mjs); this check runs `paddock-i18n build --check --no-extract` against the committed
// files: every catalog still equals what its sources produce, every runtime JSON equals its catalog, nothing was
// hand-merged. The catalog of the built `$localize` messages is read as committed (no build in the conformance step); the
// app's CI also runs `paddock-i18n build --check` after its build, which re-extracts.
//   "i18n": { "config": "app/paddock-i18n.json" }            in paddock.conformance.json (default <appDir>/paddock-i18n.json)
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { error, warn } from '../finding.mjs';

/** Runs `paddock-i18n build --check --no-extract` of the app's installed @decryp7/paddock-ui; `{ status, output }` or `{ missing }`. */
export function runPaddockI18n({ root, configFile, appDir }) {
  const bin = [join(root, appDir, 'node_modules'), join(root, 'node_modules'), join(dirname(configFile), 'node_modules')]
    .map((nm) => join(nm, '@decryp7', 'paddock-ui', 'bin', 'paddock-i18n.mjs'))
    .find((f) => existsSync(f));
  if (!bin) return { missing: true };
  const r = spawnSync(process.execPath, [bin, 'build', '--check', '--no-extract', '--config', configFile], { cwd: dirname(configFile), encoding: 'utf8', timeout: 60_000 });
  return { status: r.status ?? 1, output: `${r.stdout ?? ''}${r.stderr ?? ''}`.trim() };
}

export default {
  id: 'C32',
  title: 'The translation catalogs are current',
  rule: 40,
  async run(ctx) {
    const rel = ctx.config.i18n?.config ?? `${ctx.config.appDir}/paddock-i18n.json`;
    if (!ctx.exists(rel)) {
      return [warn('C32', 40, rel, 1, 'no paddock-i18n.json: the app does not declare its translation build, so nothing keeps its catalogs current', 'Add paddock-i18n.json (see @decryp7/paddock-ui, paddock-i18n build) and run `paddock-i18n build`.')];
    }
    const run = ctx.options.i18n ?? runPaddockI18n;
    const r = await run({ root: ctx.root, configFile: join(ctx.root, rel), appDir: ctx.config.appDir });
    if (r.missing) {
      return [warn('C32', 40, rel, 1, '@decryp7/paddock-ui is not installed, so the catalogs were not checked', 'Run `npm ci` before the conformance step.')];
    }
    if (r.status === 0) return [];
    const why = (r.output.split('\n').find((l) => l.trim()) ?? 'the build check failed').replace(/^paddock-i18n:\s*/, '');
    return [error('C32', 40, rel, 1, `the translation files are not current: ${why}`, 'Run `paddock-i18n build` (npm run i18n) and commit the result; never edit the catalogs by hand.')];
  },
};
