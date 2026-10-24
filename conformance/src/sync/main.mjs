// `paddock-conformance sync`: shows (or with --write repairs) the baseline files (C28) and doc blocks (C23) of an app repo.
import { resolve } from 'node:path';
import { loadConfig } from '../context.mjs';
import { ownPaddock, paddockRoot } from './paddock-root.mjs';
import { planBaseline, planBlocks, writePlan } from './plan.mjs';

/** Runs the sync; returns the exit status (0 in sync or repaired, 1 differences without --write, 2 usage). */
export function syncMain(o, { log = console.log, err = console.error } = {}) {
  if (!o.app && !o.root) throw new Error('sync needs --app <name>');
  const root = resolve(o.root ?? '.');
  const config = loadConfig(root, o.app);
  const paddock = paddockRoot({ paddock: o.paddock ? resolve(o.paddock) : ownPaddock() });
  if (!paddock) {
    err('paddock-conformance sync: no Paddock checkout with templates/repo; pass --paddock <dir>');
    return 2;
  }
  const baseline = planBaseline({ root, paddock, config });
  const { items: blocks } = planBlocks({ root, paddock, config });
  const differing = [...baseline.filter((i) => i.state !== 'ok').map((i) => `${i.path}: ${i.detail}`), ...blocks.filter((i) => i.state !== 'ok').map((i) => `${i.path}: block ${i.block} is ${i.state}`)];
  for (const d of differing) log(`${o.write ? 'fixing' : 'differs'}  ${d}`);
  if (!differing.length) {
    log('in sync with Paddock');
    return 0;
  }
  if (!o.write) {
    err(`${differing.length} difference(s); run with --write to repair them`);
    return 1;
  }
  const written = writePlan({ root, baseline, blocks });
  log(`wrote ${written.length} file(s): ${written.join(', ')}`);
  return 0;
}
