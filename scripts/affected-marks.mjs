#!/usr/bin/env node
// What the planner marks for each of many files, one answer per file, from one graph read (ADR 0060 section 2.4): C100 asks
// "is this check's own file an input of the planner output its CI line runs under?". Reads a JSON array of repository paths
// from standard input and prints {"<path>": ["conformance", "any_app", ...]}: the names of ci.yml's plan job outputs that the
// change of that file alone would turn on (crates, ui, eslint, conformance, tools, icons, paddock_dev, any_app).
import { readFileSync } from 'node:fs';
import { adoptsPerMachine, affected, cargoGraph, npmGraph, readApps, ROOT, trackedFiles } from './affected.mjs';
import { isMain } from '../conformance/src/util/is-main.mjs';

/** The plan job outputs a plan turns on. */
export function outputsOf(plan) {
  const out = ['crates', 'ui', 'eslint', 'conformance', 'tools', 'icons'].filter((k) => plan[k]);
  if (plan.paddockDev) out.push('paddock_dev');
  if (plan.apps.length) out.push('any_app');
  return out;
}

/** {file: outputs} for a list of files. */
export function marks(files, root = ROOT) {
  const tracked = trackedFiles(root);
  const graph = { cargo: cargoGraph(tracked, root), npm: npmGraph(tracked, root), apps: readApps(tracked, root) };
  return Object.fromEntries(files.map((f) => [f, outputsOf(affected([f], graph, { perMachine: (x) => adoptsPerMachine(root, x) }))]));
}

if (isMain(import.meta.url)) {
  const files = JSON.parse(readFileSync(0, 'utf8'));
  process.stdout.write(`${JSON.stringify(marks(files))}\n`);
}
