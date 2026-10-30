#!/usr/bin/env node
// What a release workflow run proves about the installers (rule 54, "green that proves something"). A run whose `installers +
// release` job was skipped is green but built nothing; before this, nothing said so (125 of 133 green release runs built no
// installer, issues research 2026-10-30, X4). The verdict job of every release workflow and of the nightly runs this and
// writes one line to the job summary: "installers: built" or "installers: not built (why)".
//
// A run that was ASKED to build installers (a nightly, a manual run, a tag) and did not fails. A push to main whose change does
// not reach the app is allowed to build nothing and says so. A push the plan said to build, with no installers, warns: it is
// the shape of the original gap.
//
//   EVENT=schedule REF=refs/heads/main APP=dyno PLAN_RUN=true RELEASE_RESULT=skipped node release-verdict.mjs
import { appendFileSync } from 'node:fs';
import { isMain } from '../src/util/is-main.mjs';

/**
 * @param {{app:string, event:string, ref?:string, planRun?:string, releaseResult?:string}} i planRun and releaseResult are the
 *   `needs.*` values ('true'/'false'; 'success'/'skipped'/'failure'/'cancelled'; empty when the job did not exist).
 * @returns {{ok:boolean, level:'notice'|'warning'|'error', line:string}}
 */
export function verdict({ app, event, ref = '', planRun = '', releaseResult = '' }) {
  const asked = event === 'schedule' || event === 'workflow_dispatch' || ref.startsWith('refs/tags/');
  if (releaseResult === 'success') return { ok: true, level: 'notice', line: `installers: built (${app}, every platform's bundle job passed)` };
  if (releaseResult === 'cancelled') return { ok: true, level: 'notice', line: `installers: not built (${app}): the run was cancelled` };
  if (releaseResult === 'failure') return { ok: false, level: 'error', line: `installers: build FAILED (${app}); see the bundle jobs` };
  // skipped, or no such job
  if (planRun === 'false' && !asked) return { ok: true, level: 'notice', line: `installers: not built (${app}): the change does not reach ${app}; nothing was asked for` };
  if (asked) {
    const why = event === 'schedule' ? 'a nightly run builds them' : event === 'workflow_dispatch' ? 'a manual run builds them' : 'a tag builds them';
    return { ok: false, level: 'error', line: `installers: not built (${app}) although ${why}: the installers job was ${releaseResult || 'missing'}. This run proves nothing about the release path.` };
  }
  return { ok: true, level: 'warning', line: `installers: not built (${app}): the plan said to build them for this push but the installers job was ${releaseResult || 'missing'}; this green run proves nothing about the release path` };
}

if (isMain(import.meta.url)) {
  const e = process.env;
  const v = verdict({ app: e.APP ?? 'app', event: e.EVENT ?? '', ref: e.REF ?? '', planRun: e.PLAN_RUN ?? '', releaseResult: e.RELEASE_RESULT ?? '' });
  console.log(`::${v.level}::${v.line}`);
  if (e.GITHUB_STEP_SUMMARY) appendFileSync(e.GITHUB_STEP_SUMMARY, `**${v.line}**\n\n`);
  process.exit(v.ok ? 0 : 1);
}
