// What `sync` writes and what C23/C28 compare: one pure planner per concern, shared so the check and the repair cannot disagree.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { loadBlocks, inspectBlock } from './blocks.mjs';
import { inspectFile, loadManifest, targetPath } from './baseline.mjs';

const readOrNull = (root, p) => {
  try {
    return readFileSync(join(root, p), 'utf8');
  } catch (e) {
    if (e.code === 'ENOENT') return null;
    throw e;
  }
};

/** Baseline items: [{ path, state, detail, desired }] for every manifest entry that is not skipped by config.baselineSkip. */
export function planBaseline({ root, paddock, config }) {
  const skip = new Set(config.baselineSkip ?? []);
  return loadManifest(paddock)
    .filter((e) => !skip.has(e.path))
    .map((e) => {
      const path = targetPath(e.path, config);
      const r = inspectFile(e, readOrNull(root, path), config);
      return { path, ...r };
    });
}

/** The files and blocks wanted in this app: config.docBlocks ({ file: [names] }) over the registry's defaults. */
export function blockTargets(registry, config) {
  const targets = config.docBlocks ?? registry.defaultTargets;
  return Object.entries(targets).map(([file, names]) => ({ file, names }));
}

/** Doc-block items: [{ path, block, state, desired }]; optional files that do not exist are skipped, AGENTS.md is required. */
export function planBlocks({ root, paddock, config }) {
  const registry = loadBlocks(paddock);
  const byName = new Map(registry.blocks.map((b) => [b.name, b]));
  const out = [];
  for (const { file, names } of blockTargets(registry, config)) {
    let text = readOrNull(root, file);
    if (text === null && registry.optionalTargets.includes(file)) continue;
    let current = text ?? '';
    for (const name of names) {
      const block = byName.get(name);
      if (!block) throw new Error(`paddock.conformance.json docBlocks: unknown block "${name}" (known: ${[...byName.keys()].join(', ')})`);
      const r = inspectBlock(current, block, config);
      out.push({ path: file, block: name, state: r.state, desired: r.desired, missingFile: text === null });
      current = r.desired;
    }
  }
  return { items: out, registry };
}

/** Writes the final desired text of every file that has a non-ok item. Returns the paths written. */
export function writePlan({ root, baseline = [], blocks = [] }) {
  const finals = new Map();
  for (const i of baseline) if (i.state !== 'ok') finals.set(i.path, i.desired);
  for (const i of blocks) finals.set(i.path, i.desired); // blocks of one file are chained: the last desired holds all
  const written = [];
  for (const [path, text] of finals) {
    const current = readOrNull(root, path);
    if (current === text) continue;
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
    written.push(path);
  }
  return written;
}

export const exists = (root, p) => existsSync(join(root, p));
