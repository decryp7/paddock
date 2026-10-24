#!/usr/bin/env node
// The `plan` job's decision (plan X10, from Debrief's build.yml): is this run needed at all?
//   - a nightly (schedule) run is skipped while main is unchanged since the last fully green nightly (the marker cache
//     `nightly-green-<sha>` exists, see nightly-mark);
//   - a push to main whose commit carries a v* tag is covered by the tag run, so it is skipped.
// Everything else runs. Output: `run=true|false` to $GITHUB_OUTPUT, a ::notice line saying why when skipped.
//
//   EVENT=schedule SHA=... REF=refs/heads/main GREEN=true TAGS="$(git ls-remote --tags origin 'refs/tags/v*')" node plan.mjs
import { appendFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

/** The tag name for `sha` in `git ls-remote --tags` output (annotated tags list the peeled commit as `^{}`), or null. */
export function tagFor(sha, lsRemote) {
  for (const line of String(lsRemote ?? '').split('\n')) {
    const [id, ref] = line.trim().split(/\s+/);
    if (id === sha && ref?.startsWith('refs/tags/')) return ref.slice('refs/tags/'.length).replace(/\^\{\}$/, '');
  }
  return null;
}

/** { run, reason } for an event. `green` is the marker cache's `cache-hit` output ("true" when found). */
export function decide({ event, ref, sha, green, tags, mainRef = 'refs/heads/main' }) {
  if (event === 'schedule' && green === 'true') return { run: false, reason: `main has not changed since the last green run (${sha}); skipping.` };
  if (event === 'push' && ref === mainRef) {
    const tag = tagFor(sha, tags);
    if (tag) return { run: false, reason: `commit is tagged ${tag}; the tag run is the gate.` };
  }
  return { run: true, reason: null };
}

function main(env, log = console.log) {
  const r = decide({ event: env.EVENT, ref: env.REF, sha: env.SHA, green: env.GREEN, tags: env.TAGS, mainRef: env.MAIN_REF || undefined });
  if (r.reason) log(`::notice::${r.reason}`);
  if (env.GITHUB_OUTPUT) appendFileSync(env.GITHUB_OUTPUT, `run=${r.run}\n`);
  else log(`run=${r.run}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main(process.env);
