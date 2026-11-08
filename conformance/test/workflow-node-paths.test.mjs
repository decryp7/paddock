import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { parseJobs, parseSteps } from '../src/util/workflows.mjs';

// A `node ../<script>` (a path up from the working directory; plain and `.paddock/` paths are checked by their own tests) in a workflow run step resolves from the step's working directory (ADR 0062 review P1-1: Debrief's
// Install step called ../../conformance/... from apps/debrief/app, a file that does not exist).
const root = fileURLToPath(new URL('../..', import.meta.url));
const wfDir = join(root, '.gitea', 'workflows');

const wd = (text) => /^\s*working-directory:\s*(.+?)\s*$/m.exec(text)?.[1];

export function nodeCalls(file) {
  const text = readFileSync(join(wfDir, file), 'utf8');
  const workflowDefault = wd((/^defaults:\n(?: {2}.*\n?)+/m.exec(text) ?? [''])[0]);
  const jobs = parseJobs(text);
  const out = [];
  for (const step of parseSteps(text)) {
    if (!step.run) continue;
    const job = [...jobs].reverse().find((j) => j.line <= step.line);
    const jobDefault = wd((/^ {4}defaults:\n(?: {6}.*\n?)+/m.exec(job?.text ?? '') ?? [''])[0]);
    const dir = (/^\s+working-directory:\s*(.+?)\s*$/m.exec(step.text)?.[1] ?? jobDefault ?? workflowDefault ?? '.');
    for (const m of step.run.matchAll(/(?:^|[\s(;&|])node\s+(?:--[\w-]+\s+)*(\.\.\/[^\s"'$`)*]+\.mjs)/g)) {
      out.push({ file, job: job?.id, step: step.name, dir, script: m[1] });
    }
  }
  return out;
}

test('every relative `node <script>` in a workflow run step exists from the step\'s working directory', () => {
  const missing = [];
  for (const file of readdirSync(wfDir).filter((f) => f.endsWith('.yml'))) {
    for (const c of nodeCalls(file)) {
      if (c.dir.includes('${{')) continue;
      if (!existsSync(resolve(root, c.dir, c.script))) missing.push(`${c.file} ${c.job} "${c.step ?? ''}": node ${c.script} from ${c.dir}`);
    }
  }
  assert.deepEqual(missing, []);
});
