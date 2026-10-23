#!/usr/bin/env node
// Turns kit templates into an app's colours: node diagrams/tools/apply-accent.mjs <app> <template.drawio> [out.drawio]
// <app>: debrief | pitcrew | pitwall | dyno | sighter | spotter. Replaces the accent, core and UI role colours (stroke and tint, light and
// dark) and the footer name; with no out file the result goes to stdout.
import { readFileSync, writeFileSync } from 'node:fs';
import { theme } from './themes.mjs';

const [app, input, output] = process.argv.slice(2);
if (!app || !input) {
  console.error('usage: apply-accent.mjs <app> <template.drawio> [out.drawio]');
  process.exit(2);
}
const from = theme('kit'), to = theme(app);
const map = new Map();
for (const role of ['accent', 'core', 'ui']) {
  for (const k of ['s', 'f']) from[role][k].forEach((hex, i) => map.set(hex.toLowerCase(), to[role][k][i]));
}
const re = new RegExp([...map.keys()].join('|'), 'gi');
const out = readFileSync(input, 'utf8').replace(re, (m) => map.get(m.toLowerCase())).replaceAll(from.app, to.app);
if (output) writeFileSync(output, out);
else process.stdout.write(out);
