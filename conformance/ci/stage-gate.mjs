// The stage gates of a staged workflow (ci.yml, debrief.yml; docs/train-checklist.md "Staged CI"). A run has two stages: stage 1 (the
// fast checks: lint, conformance, unit and integration tests; minutes) and stage 2 (long: end-to-end tests, installers, smoke tests).
// There is one gate job per operating system (`stage1-<os>`, conformance/ci/stages.json is the table); a stage-2 job waits only for
// the gates of its own OS. Fast jobs in no gate run at the same time and stay required: a failed one fails the run. Two modes, both fed the job's `needs` context as JSON
// (`NEEDS: ${{ toJSON(needs) }}`):
//
//   node conformance/ci/stage-gate.mjs stage1   every stage-1 job (the gate's `needs`): fails when one failed or was cancelled. A
//                                               skipped job is one the plan left out; it passes. It prints `result=<passed|failed|
//                                               failed(report-only)>` to GITHUB_OUTPUT. Report-only (ER-41) while docs/gates.toml
//                                               has `[[report_only]] id = "gate:<WORKFLOW>/<GATE>"`: the job passes with a warning
//                                               and the result says failed(report-only), never passed. Removing the entry (the step
//                                               that records the gate's first green run) makes it block.
//   node conformance/ci/stage-gate.mjs final    the last job (`needs`: plan, the stage1-<os> gates and the stage-2 jobs): prints the line
//                                               `stage verdict: stage1=<passed|failed|failed(report-only)> stage2=<complete|not-run|
//                                               incomplete|failed>` that `./dev ci verdict` reads (a run counts as full only with
//                                               `stage1=passed stage2=complete`, ER-33). EXPECTED is the plan's list of stage-2 jobs
//                                               (its `stage2_jobs` output): each must be `success`, a skipped one is not complete.
//                                               With STAGE=fast (the workflows' `stage` dispatch input) stage 2 is left out on
//                                               purpose: `stage2=not-run`, exit 0.
//
// A gate that saw no jobs, an unreadable context or an unknown STAGE is broken and fails: it never passes on nothing (ER-33).
import { appendFileSync, readFileSync } from 'node:fs';
import { isMain } from '../src/util/is-main.mjs';

/** The line `./dev ci verdict` looks for in the final job's log (tools/paddock-dev/src/ci_runs/verdict.rs, STAGE_LINE). */
export const STAGE_LINE = 'stage verdict: ';

const PASS = new Set(['success', 'skipped']);

/** The jobs of a `needs` context that did not pass: a failure, a cancel, or no result at all. */
export function notPassed(needs, except = []) {
  return Object.entries(needs)
    .filter(([id]) => !except.includes(id))
    .filter(([, n]) => !PASS.has(n?.result))
    .map(([id, n]) => `${id} (${n?.result || 'no result'})`);
}

/** Parses the context; anything but an object with at least one job is a broken gate. */
export function parseNeeds(text) {
  let needs;
  try {
    needs = JSON.parse(text ?? '');
  } catch (e) {
    throw new Error(`the needs context is not JSON (${e.message}). Fix: the job must set NEEDS: \${{ toJSON(needs) }}`);
  }
  if (needs === null || typeof needs !== 'object' || Array.isArray(needs) || Object.keys(needs).length === 0) {
    throw new Error('the needs context lists no job: a stage gate that waits for nothing would pass on nothing. Fix: list the stage\'s jobs in `needs:`');
  }
  return needs;
}

/** Whether docs/gates.toml registers this workflow's stage-1 gate as report-only: a `[[report_only]]` table whose `id` is `gate:<workflow>/<gate job>`. */
export function gateIsReportOnly(gatesText, workflow, gate = 'stage1-linux') {
  const want = `gate:${workflow}/${gate}`;
  let inTable = false;
  for (const raw of String(gatesText ?? '').split('\n')) {
    const line = raw.trim();
    if (line.startsWith('[[')) inTable = line === '[[report_only]]';
    else if (inTable && line === `id = "${want}"`) return true;
  }
  return false;
}

/** Stage 1: pass only when no job failed or was cancelled; `enforce` false is report-only (ER-41). `result` is what the final job reads. */
export function stage1(needs, enforce = true) {
  const bad = notPassed(needs);
  const ran = Object.values(needs).filter((n) => n.result === 'success').length;
  if (bad.length && !enforce) {
    return {
      ok: true,
      result: 'failed(report-only)',
      lines: [`::warning::stage 1 failed: ${bad.join(', ')}. Report-only: stage 2 still runs and the run's verdict says stage1=failed(report-only). The gate blocks once its [[report_only]] entry gate:<workflow>/stage1 is removed from docs/gates.toml (after its first recorded green run).`],
    };
  }
  if (bad.length) {
    return {
      ok: false,
      result: 'failed',
      lines: [
        `::error::stage 1 failed: ${bad.join(', ')}. Stage 2 (end-to-end, WebKit, installers, smoke tests) does not run. Next: fix these jobs (their logs name the failing step), rerun them locally, push once.`,
      ],
    };
  }
  return { ok: true, result: 'passed', lines: [`stage 1: passed (${ran} job(s) ran, ${Object.keys(needs).length - ran} left out by the plan); stage 2 starts`] };
}

/** The plan's expected stage-2 jobs: a JSON list of job ids. Anything else is a broken gate. */
function parseExpected(text) {
  let list;
  try {
    list = JSON.parse(text);
  } catch {
    list = null;
  }
  if (!Array.isArray(list) || !list.every((j) => typeof j === 'string')) {
    throw new Error(`the plan's stage2_jobs output is not a JSON list of job ids (${JSON.stringify(text)}). Fix: the plan step must write stage2_jobs`);
  }
  return list;
}

/** The gate jobs of a `needs` context: one per operating system, `stage1-<os>`. */
export const gateIds = (needs) => Object.keys(needs).filter((id) => id.startsWith('stage1-'));

/** The final job: the verdict line, and a failure for anything but a complete stage 2 after passed gates. */
export function final(needs, stage = '', expectedText = '[]') {
  const gates = gateIds(needs);
  if (gates.length === 0) {
    throw new Error('the needs context has no stage1-<os> gate job: the final job must need the gates. Fix: add them to its `needs:`');
  }
  const expected = parseExpected(expectedText);
  const broken = gates.filter((g) => needs[g].result !== 'success');
  if (broken.length) {
    return {
      ok: false,
      lines: [
        `${STAGE_LINE}stage1=failed stage2=not-run`,
        `::error::stage-1 gate(s) did not pass (${broken.map((g) => `${g}: ${needs[g].result || 'no result'}`).join(', ')}): stage 2 did not run for them, so this run tested less than a full run. Next: fix stage 1 and run again.`,
      ],
    };
  }
  // Each gate says passed, failed or failed(report-only) in its output; a missing output is a gate that could not say.
  const results = gates.map((g) => needs[g].outputs?.result ?? 'unknown');
  const s1 = results.every((r) => r === 'passed') ? 'passed' : results.some((r) => r === 'failed' || r === 'unknown') ? 'failed' : 'failed(report-only)';
  if (stage === 'fast') {
    // A fast dispatch (only stage 1 was asked for): stage 2 was left out on purpose, so the run passes but is never complete.
    return { ok: s1 === 'passed', lines: [`${STAGE_LINE}stage1=${s1} stage2=not-run`, '::notice::stage=fast: stage 2 did not run, so this is not a full run'] };
  }
  const bad = notPassed(needs, gates);
  if (bad.length) {
    return {
      ok: false,
      lines: [`${STAGE_LINE}stage1=${s1} stage2=failed`, `::error::a job of this run failed: ${bad.join(', ')}. Next: fix these jobs and run again.`],
    };
  }
  const missing = expected.filter((j) => needs[j]?.result !== 'success');
  if (missing.length) {
    return {
      ok: false,
      lines: [
        `${STAGE_LINE}stage1=${s1} stage2=incomplete`,
        `::error::expected stage-2 job(s) that did not succeed: ${missing.join(', ')}. A skipped job is not a pass here. Next: find why the job did not run (its if, a missing need) or fix the plan's stage2_jobs.`,
      ],
    };
  }
  return { ok: s1 === 'passed', lines: [`${STAGE_LINE}stage1=${s1} stage2=complete`] };
}

if (isMain(import.meta.url)) {
  const mode = process.argv[2];
  try {
    if (mode !== 'stage1' && mode !== 'final') throw new Error(`usage: stage-gate.mjs stage1|final (got ${mode ?? 'nothing'})`);
    const stage = process.env.STAGE ?? '';
    if (!['', 'all', 'fast'].includes(stage)) throw new Error(`the stage input is ${JSON.stringify(stage)}: it must be empty, all or fast. Fix: dispatch with stage=fast or stage=all`);
    const needs = parseNeeds(process.env.NEEDS);
    let r;
    if (mode === 'stage1') {
      const workflow = process.env.WORKFLOW ?? '';
      if (!workflow) throw new Error('WORKFLOW is not set: the gate cannot read its mode from docs/gates.toml. Fix: set WORKFLOW: ci.yml (or debrief.yml) in the step');
      let gates;
      try {
        gates = readFileSync(process.env.GATES_FILE ?? 'docs/gates.toml', 'utf8');
      } catch (e) {
        throw new Error(`docs/gates.toml cannot be read (${e.message}): the gate cannot tell if it is report-only. Fix: check the job's sparse checkout lists docs/gates.toml`);
      }
      const gate = process.env.GATE ?? '';
      if (!gate) throw new Error('GATE is not set: the gate cannot read its mode. Fix: set GATE: stage1-linux (the job id) in the step');
      r = stage1(needs, !gateIsReportOnly(gates, workflow, gate));
      if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `result=${r.result}\n`);
    } else {
      r = final(needs, stage, process.env.EXPECTED ?? '');
    }
    for (const l of r.lines) console.log(l);
    process.exit(r.ok ? 0 : 1);
  } catch (e) {
    console.log(`::error::${e.message}`);
    process.exit(2);
  }
}
