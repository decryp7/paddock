#!/usr/bin/env node
// What the planner marks for each of many files, one answer per file, from one graph read (ADR 0060 section 2.4): C100 asks
// "is this check's own file an input of the planner output its CI line runs under?". Reads a JSON array of repository paths
// from standard input and prints {"<path>": ["conformance", "any_app", ...]}: the names of ci.yml's plan job outputs that the
// change of that file alone would turn on (crates, ui, eslint, conformance, tools, icons, paddock_dev, c99, any_app).
import { readFileSync } from 'node:fs';
import { adoptsPerMachine, affected, cargoGraph, npmGraph, readApps, ROOT, trackedFiles } from './affected.mjs';
import { isMain } from '../conformance/src/util/is-main.mjs';

/** The plan job outputs a plan turns on. */
export function outputsOf(plan) {
  const out = ['crates', 'ui', 'eslint', 'conformance', 'tools', 'icons'].filter((k) => plan[k]);
  if (plan.paddockDev) out.push('paddock_dev');
  if (plan.c99) out.push('c99');
  if (plan.apps.length) out.push('any_app');
  return out;
}

/** {file: outputs} for a list of files. */
export function marks(files, root = ROOT, timings = null) {
  const tracked = trackedFiles(root);
  const graph = { cargo: cargoGraph(tracked, root, timings), npm: npmGraph(tracked, root), apps: readApps(tracked, root) };
  return Object.fromEntries(files.map((f) => [f, outputsOf(affected([f], graph, { perMachine: (x) => adoptsPerMachine(root, x) }))]));
}

if (isMain(import.meta.url)) {
  const files = JSON.parse(readFileSync(0, 'utf8'));
  // Each `cargo metadata` call is timed on stderr: a call that waits on cargo's package-cache lock shows as one slow line (C100 prints its tail).
  const timings = { push: (t) => process.stderr.write(`affected-marks: cargo metadata ${t.manifest} ${t.ms} ms\n`) }; // written as each call ends, so a hang shows the last one that finished
  const answer = marks(files, ROOT, timings);
  process.stdout.write(`${JSON.stringify(answer)}\n`);
}
