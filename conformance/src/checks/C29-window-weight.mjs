// C29: window weight budgets (rule 35, plan X46). Each window kind has a budget in paddock.conformance.json
//   "windowWeight": { "stats": "app/dist/app/browser-stats.json", "windows": { "main": { "maxKb": 1355 }, "about": { "maxKb": 480 } } }
// The JS a window loads before first paint (static closure of its entry chunks, from `ng build --stats-json`) may not exceed
// its budget, and a budget more than 10% above the measured size is stale: "tightened to measured size plus 10%".
import { error, warn } from '../finding.mjs';
import { budgetKb, measure } from '../window-weight/measure.mjs';

export default {
  id: 'C29',
  scope: 'both',
  // Every outcome on the repository's own run is a warning: windowWeight is configured nowhere and the conformance job builds no stats
  // (docs/benchmarks.md). C100 must not count it as a gate; it becomes 'gate' when an app's budgets and stats are wired (ADR 0060 P1-3).
  mode: 'report-only',
  title: 'Each window stays within its initial JS budget',
  rule: 35,
  run(ctx) {
    const cfg = ctx.config.windowWeight;
    const file = 'paddock.conformance.json';
    if (!cfg?.windows || !Object.keys(cfg.windows).length) {
      return [warn('C29', 35, file, 1, 'no window budgets: "windowWeight.windows" is not configured (rule 35)', 'Run `paddock-conformance window-weight --record` after `ng build --stats-json` to record the measured sizes plus 10%.')];
    }
    const statsPath = cfg.stats ?? `${ctx.config.appDir}/dist/app/browser-stats.json`;
    const text = ctx.read(statsPath);
    if (text === null) {
      return [warn('C29', 35, statsPath, 1, `no build stats at ${statsPath}: the window budgets were not checked`, 'Run `ng build --stats-json` before the conformance step.')];
    }
    let outputs;
    try {
      outputs = JSON.parse(text).outputs;
      if (!outputs || typeof outputs !== 'object') throw new Error('no "outputs"');
    } catch (e) {
      return [error('C29', 35, statsPath, 1, `${statsPath} is not an esbuild metafile (${e.message})`, 'Build with `ng build --stats-json` (the application builder).')];
    }
    const out = [];
    for (const [kind, spec] of Object.entries(cfg.windows)) {
      const m = measure(outputs, kind, spec);
      if (!m) {
        out.push(error('C29', 35, statsPath, 1, `the "${kind}" window's entry chunks are not in the build stats`, 'Rebuild with --stats-json, or list the window\'s "entries" in windowWeight.windows.' + kind + '.'));
        continue;
      }
      const kb = m.total / 1024;
      if (typeof spec.maxKb !== 'number') out.push(warn('C29', 35, file, 1, `the "${kind}" window has no maxKb (it loads ${kb.toFixed(1)} kB)`, `Set "maxKb": ${budgetKb(m.total)} (measured plus 10%).`));
      else if (kb > spec.maxKb) out.push(error('C29', 35, statsPath, 1, `the "${kind}" window loads ${kb.toFixed(1)} kB before first paint, over its ${spec.maxKb} kB budget`, 'Move heavy code behind @defer or loadComponent; raise the budget only with a reason in the change.'));
      else if (spec.maxKb > budgetKb(m.total)) out.push(warn('C29', 35, file, 1, `the "${kind}" window's budget ${spec.maxKb} kB is above measured plus 10% (${kb.toFixed(1)} kB)`, `Tighten it to ${budgetKb(m.total)} kB (\`paddock-conformance window-weight --record\`).`));
    }
    return out;
  },
};
