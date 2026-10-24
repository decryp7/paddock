// The JS a window loads before first paint, from the production build's esbuild metafile (`ng build --stats-json` writes
// dist/<app>/browser-stats.json). Rule 35 (plan X46, ported from Debrief's app/scripts/window-weight.mjs). Pure functions.
const base = (p) => p.replace(/^.*\//, '');

/** Entries every window shares unless the app lists its own. */
export const COMMON_ENTRIES = ['src/main.ts', 'src/bootstrap.ts'];

/**
 * The entry sources whose chunks a window loads. `spec.entries` lists them all; otherwise `main` is the common entries plus
 * app.ts and app.config.ts, `popout` swaps app.ts for src/app/popout/popout-shell.ts, and any other name is a dialog window
 * (`src/app/<name>/<name>-dialog.ts` behind the dialog shell and bootstrap).
 */
export function windowEntries(kind, spec = {}) {
  if (spec.entries?.length) return spec.entries;
  const common = spec.common ?? COMMON_ENTRIES;
  if (kind === 'main') return [...common, 'src/app/app.ts', 'src/app/app.config.ts'];
  if (kind === 'popout') return [...common, 'src/app/popout/popout-shell.ts', 'src/app/app.config.ts'];
  return [...common, 'src/app/dialog-windows/dialog-shell.ts', 'src/app/dialog-windows/dialog-bootstrap.ts', `src/app/${kind}/${kind}-dialog.ts`];
}

/** Indexes the metafile's JS outputs by file name. */
export function indexOutputs(outputs) {
  return new Map(Object.entries(outputs).filter(([f]) => f.endsWith('.js')).map(([f, o]) => [base(f), o]));
}

/** The output file whose entry point is the source file (a lazy chunk named after its entry point). */
export function chunkOf(byFile, source) {
  return [...byFile].find(([, o]) => o.entryPoint === source)?.[0];
}

/** Static closure of output files reachable from the roots (dynamic imports excluded; cycles and unknown files are fine). */
export function staticClosure(byFile, roots) {
  const seen = new Set();
  const stack = [...roots];
  while (stack.length) {
    const f = stack.pop();
    if (!f || seen.has(f) || !byFile.has(f)) continue;
    seen.add(f);
    for (const imp of byFile.get(f).imports ?? []) if (imp.kind === 'import-statement') stack.push(base(imp.path));
  }
  return seen;
}

/** The package a source file belongs to: the npm package, or the directory under src/app. */
export function packageOf(input) {
  const nm = input.match(/node_modules\/((?:@[^/]+\/)?[^/]+)/);
  if (nm) return nm[1];
  const app = input.match(/^src\/app\/([^/]+)/);
  if (app) return `src/app/${app[1]}`;
  return input.startsWith('src/') ? 'src' : input.split('/').slice(0, 2).join('/');
}

/**
 * Measures one window: { kind, total (bytes), count, chunks, packages, files }, or null when its entry chunks are not in
 * the build (an entry source of the window has no chunk: the stats are not current, or the window does not exist; `spec.optional` allows that many missing).
 */
export function measure(outputs, kind, spec = {}) {
  const byFile = indexOutputs(outputs);
  const entries = windowEntries(kind, spec);
  const roots = entries.map((s) => chunkOf(byFile, s)).filter(Boolean);
  if (roots.length < entries.length - (spec.optional ?? 0) || !roots.length) return null;
  const loaded = staticClosure(byFile, roots);
  const pkgs = new Map();
  let total = 0;
  const chunks = [];
  for (const f of loaded) {
    const o = byFile.get(f);
    total += o.bytes;
    const inputs = Object.entries(o.inputs ?? {}).sort((a, b) => b[1].bytesInOutput - a[1].bytesInOutput);
    chunks.push({ file: f, bytes: o.bytes, src: inputs[0]?.[0] });
    for (const [src, i] of inputs) pkgs.set(packageOf(src), (pkgs.get(packageOf(src)) ?? 0) + i.bytesInOutput);
  }
  chunks.sort((a, b) => b.bytes - a.bytes);
  const packages = [...pkgs].map(([name, bytes]) => ({ name, bytes })).sort((a, b) => b.bytes - a.bytes);
  return { kind, total, count: loaded.size, chunks, packages, files: loaded };
}

/** The budget rule 35 asks for after a release: measured size plus 10%, in whole kB, never below 1. */
export const budgetKb = (bytes) => Math.max(1, Math.ceil((bytes * 1.1) / 1024));

/** Text report of one measurement (heaviest chunks and packages), for the CLI and the job log. */
export function report(m, top = 15) {
  const kb = (b) => (b / 1024).toFixed(1).padStart(8);
  const lines = [`JS the "${m.kind}" window loads before first paint (static closure of its entry chunks): ${(m.total / 1024).toFixed(1)} kB in ${m.count} chunks`];
  for (const r of m.chunks.slice(0, top)) lines.push(`  ${kb(r.bytes)} kB  ${r.file}  (${r.src ?? ''})`);
  lines.push('  heaviest packages:');
  for (const p of m.packages.slice(0, top)) lines.push(`  ${kb(p.bytes)} kB  ${p.name}`);
  return lines.join('\n');
}
