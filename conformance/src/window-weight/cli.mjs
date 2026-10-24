// `paddock-conformance window-weight [kind...] [--root dir] [--stats file] [--max kb] [--record]`
// Prints the JS each window loads before first paint. Without kinds, the windows of paddock.conformance.json are used.
// --max fails above a size (the old `window-weight.mjs main --max 1355`); --record writes measured size plus 10% as the
// window's budget into paddock.conformance.json (rule 35: tightened every release).
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { loadConfig } from '../context.mjs';
import { budgetKb, measure, report } from './measure.mjs';

/** Runs the command; returns the exit status (0 ok, 1 over budget, 2 usage or unreadable stats). */
export function windowWeightMain(o, kinds, { log = console.log, err = console.error } = {}) {
  const root = resolve(o.root ?? '.');
  const config = loadConfig(root, o.app ?? 'app');
  const cfg = config.windowWeight ?? { windows: {} };
  const statsPath = resolve(root, o.stats ?? cfg.stats ?? `${config.appDir}/dist/app/browser-stats.json`);
  let outputs;
  try {
    outputs = JSON.parse(readFileSync(statsPath, 'utf8')).outputs;
    if (!outputs) throw new Error('no "outputs"');
  } catch (e) {
    err(`cannot read the build stats ${statsPath} (${e.message}); build with \`ng build --stats-json\``);
    return 2;
  }
  const names = kinds.length ? kinds : Object.keys(cfg.windows ?? {});
  if (!names.length) {
    err('no window given and none configured: window-weight <kind...> or windowWeight.windows in paddock.conformance.json');
    return 2;
  }
  let status = 0;
  const recorded = {};
  for (const kind of names) {
    const m = measure(outputs, kind, cfg.windows?.[kind] ?? {});
    if (!m) {
      err(`could not find the "${kind}" window's entry chunks in ${statsPath}; is the build current?`);
      status = 2;
      continue;
    }
    log(report(m));
    const limit = o.max !== undefined ? Number(o.max) : cfg.windows?.[kind]?.maxKb;
    if (typeof limit === 'number' && m.total / 1024 > limit) {
      err(`FAIL: ${(m.total / 1024).toFixed(1)} kB exceeds the ${limit} kB budget for the "${kind}" window`);
      status = Math.max(status, 1);
    }
    recorded[kind] = budgetKb(m.total);
  }
  if (o.record && status !== 2) {
    const file = join(root, 'paddock.conformance.json');
    let json = {};
    try {
      json = JSON.parse(readFileSync(file, 'utf8'));
    } catch (e) {
      if (e.code !== 'ENOENT') throw new Error(`paddock.conformance.json: ${e.message}`);
    }
    json.windowWeight = { ...json.windowWeight, stats: json.windowWeight?.stats ?? relativeStats(root, statsPath), windows: { ...json.windowWeight?.windows } };
    for (const [kind, kb] of Object.entries(recorded)) json.windowWeight.windows[kind] = { ...json.windowWeight.windows[kind], maxKb: kb };
    writeFileSync(file, `${JSON.stringify(json, null, 2)}\n`);
    log(`recorded budgets (measured + 10%): ${Object.entries(recorded).map(([k, v]) => `${k} ${v} kB`).join(', ')}`);
    return status === 1 ? 0 : status;
  }
  return status;
}

const relativeStats = (root, p) => p.slice(root.length + 1);
