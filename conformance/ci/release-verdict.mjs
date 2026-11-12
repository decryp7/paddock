#!/usr/bin/env node
// What a release workflow run proves about the installers (rule 54, "green that proves something"). A run whose `installers +
// release` job was skipped is green but built nothing; before this, nothing said so (125 of 133 green release runs built no
// installer, issues research 2026-10-30, X4). The verdict job of every release workflow and of the nightly runs this and
// writes one line to the job summary: "installers: built" or "installers: not built (why)".
//
// A nightly skipped by its change gate says why. A run that was ASKED to build installers (a nightly, a manual run, a tag) and did not fails. A push to main whose change does
// not reach the app is allowed to build nothing and says so. A push the plan said to build, with no installers, warns: it is
// the shape of the original gap.
//
//   EVENT=schedule REF=refs/heads/main APP=dyno PLAN_RUN=true RELEASE_RESULT=skipped node release-verdict.mjs
import { appendFileSync } from 'node:fs';
import { isMain } from '../src/util/is-main.mjs';

/**
 * @param {{app:string, event:string, ref?:string, planRun?:string, skipReason?:string, releaseResult?:string, gatesResult?:string, failedGates?:string[]}} i planRun and releaseResult are the
 *   `needs.*` values ('true'/'false'; 'success'/'skipped'/'failure'/'cancelled'; empty when the job did not exist). skipReason
 *   'train': the gate ended a scheduled run early because a train was in CI (./dev ci schedule-skip).
 * @returns {{ok:boolean, level:'notice'|'warning'|'error', line:string}}
 */
export function verdict({ app, event, ref = '', planRun = '', skipReason = '', releaseResult = '', gatesResult = '', failedGates = [] }) {
  const isTag = ref.startsWith('refs/tags/');
  const asked = event === 'schedule' || event === 'workflow_dispatch' || isTag;
  if (releaseResult === 'success') return { ok: true, level: 'notice', line: `installers: built (${app}, every platform's bundle job passed)` };
  if (releaseResult === 'cancelled') return { ok: true, level: 'notice', line: `installers: not built (${app}): the run was cancelled` };
  if (releaseResult === 'failure') return { ok: false, level: 'error', line: `installers: build FAILED (${app}); see the bundle jobs` };
  // skipped, or no such job. A tag whose gates were not green builds and releases nothing: say which gates.
  if (isTag && (gatesResult === 'failure' || gatesResult === 'cancelled')) {
    const names = failedGates.length ? `: ${failedGates.join(', ')}` : '';
    return { ok: false, level: 'error', line: `installers: not built (${app}): the gates were ${gatesResult}${names}, so the tag was not released` };
  }
  // A nightly ended early because a train was in CI: nothing was measured, which a warning says (it is not a green night).
  if (planRun === 'false' && event === 'schedule' && skipReason === 'train') return { ok: true, level: 'warning', line: `installers: not built (${app}): a train was in CI, so this night measured nothing (not measured, not green); the next scheduled night builds them` };
  // A nightly whose gate found main and the toolchain image unchanged since the last nightly that built the installers (rule 50).
  if (planRun === 'false' && event === 'schedule') return { ok: true, level: 'notice', line: `installers: not built (${app}): main and the toolchain image are unchanged since the last nightly that built them` };
  if (planRun === 'false' && !asked) return { ok: true, level: 'notice', line: `installers: not built (${app}): the change does not reach ${app}; nothing was asked for` };
  if (asked) {
    const why = event === 'schedule' ? 'a nightly run builds them' : event === 'workflow_dispatch' ? 'a manual run builds them' : 'a tag builds them';
    return { ok: false, level: 'error', line: `installers: not built (${app}) although ${why}: the installers job was ${releaseResult || 'missing'}. This run proves nothing about the release path.` };
  }
  return { ok: true, level: 'warning', line: `installers: not built (${app}): the plan said to build them for this push but the installers job was ${releaseResult || 'missing'}; this green run proves nothing about the release path` };
}

/** Names of the failed jobs of this run (best effort, 10 s): the nested jobs of the gates are listed in the run's job list. */
export async function failedJobNames(env, fetchImpl = fetch) {
  try {
    const url = `${env.SERVER_URL}/api/v1/repos/${env.REPOSITORY}/actions/runs/${env.RUN_ID}/jobs`;
    const res = await fetchImpl(url, { headers: env.TOKEN ? { Authorization: `token ${env.TOKEN}` } : {}, signal: AbortSignal.timeout(10_000) });
    if (!res.ok) return [];
    return ((await res.json()).jobs ?? []).filter((j) => j.conclusion === 'failure' && !/^installers verdict/.test(j.name)).map((j) => j.name);
  } catch {
    return [];
  }
}

if (isMain(import.meta.url)) {
  const e = process.env;
  const gatesResult = e.GATES_RESULT ?? '';
  const failedGates = gatesResult === 'failure' && (e.REF ?? '').startsWith('refs/tags/')
    ? await failedJobNames({ SERVER_URL: e.SERVER_URL, REPOSITORY: e.REPOSITORY, RUN_ID: e.RUN_ID, TOKEN: e.TOKEN }) : [];
  const v = verdict({ app: e.APP ?? 'app', event: e.EVENT ?? '', ref: e.REF ?? '', planRun: e.PLAN_RUN ?? '', skipReason: e.SKIP_REASON ?? '', releaseResult: e.RELEASE_RESULT ?? '', gatesResult, failedGates });
  console.log(`::${v.level}::${v.line}`);
  if (e.GITHUB_STEP_SUMMARY) appendFileSync(e.GITHUB_STEP_SUMMARY, `**${v.line}**\n\n`);
  process.exit(v.ok ? 0 : 1);
}
