// The stage gates of a staged workflow (ci.yml, debrief.yml; docs/train-checklist.md "Staged CI"). A run has two stages: stage 1
// (fast: formatting, lints, conformance and the unit and integration tests on every OS) and stage 2 (long: end-to-end tests, the
// installers and the real-app smoke tests). Stage 2 starts only when the `stage1` gate job passed. Two modes, both fed the job's
// `needs` context as JSON (`NEEDS: ${{ toJSON(needs) }}`):
//
//   node conformance/ci/stage-gate.mjs stage1   every stage-1 job (the gate's `needs`): fails when one failed or was cancelled.
//                                               A skipped job is one the plan left out; it passes.
//   node conformance/ci/stage-gate.mjs final    the last job (`needs`: plan, stage1 and every stage-2 job): prints the line
//                                               `stage verdict: stage1=<passed|failed> stage2=<complete|not-run|failed>` that
//                                               `./dev ci verdict` reads (a run counts as full only with `stage2=complete`, ER-33).
//                                               With STAGE=fast (the workflows' `stage` dispatch input) stage 2 is left out on purpose:
//                                               `stage2=not-run`, exit 0.
//
// A gate that saw no jobs, or an unreadable context, is broken and fails: it never passes on nothing (ER-33).
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

/** Stage 1: pass only when no job failed or was cancelled. */
export function stage1(needs) {
  const bad = notPassed(needs);
  const ran = Object.values(needs).filter((n) => n.result === 'success').length;
  if (bad.length) {
    return {
      ok: false,
      lines: [
        `::error::stage 1 failed: ${bad.join(', ')}. Stage 2 (end-to-end, WebKit, installers, smoke tests) does not run. Next: fix these jobs (their logs name the failing step), rerun them locally, push once.`,
      ],
    };
  }
  return { ok: true, lines: [`stage 1: passed (${ran} job(s) ran, ${Object.keys(needs).length - ran} left out by the plan); stage 2 starts`] };
}

/** The final job: the verdict line, and a failure for anything but a complete stage 2 after a passed stage 1. */
export function final(needs, stage = '') {
  if (!needs.stage1) {
    throw new Error('the needs context has no `stage1` job: the final job must need the stage-1 gate. Fix: add stage1 to its `needs:`');
  }
  if (needs.stage1.result !== 'success') {
    return {
      ok: false,
      lines: [
        `${STAGE_LINE}stage1=failed stage2=not-run`,
        `::error::stage 1 did not pass (${needs.stage1.result || 'no result'}): stage 2 did not run, so this run tested less than a full run. Next: fix stage 1 and run again.`,
      ],
    };
  }
  if (stage === 'fast') {
    // A fast dispatch (only stage 1 was asked for): stage 2 was left out on purpose, so the run passes but is never complete.
    return { ok: true, lines: [`${STAGE_LINE}stage1=passed stage2=not-run`, '::notice::stage=fast: stage 2 did not run, so this is not a full run'] };
  }
  const bad = notPassed(needs, ['stage1']);
  if (bad.length) {
    return {
      ok: false,
      lines: [`${STAGE_LINE}stage1=passed stage2=failed`, `::error::stage 2 failed: ${bad.join(', ')}. Next: fix these jobs and run again.`],
    };
  }
  return { ok: true, lines: [`${STAGE_LINE}stage1=passed stage2=complete`] };
}

if (isMain(import.meta.url)) {
  const mode = process.argv[2];
  try {
    if (mode !== 'stage1' && mode !== 'final') throw new Error(`usage: stage-gate.mjs stage1|final (got ${mode ?? 'nothing'})`);
    const needs = parseNeeds(process.env.NEEDS);
    const r = mode === 'stage1' ? stage1(needs) : final(needs, process.env.STAGE ?? '');
    for (const l of r.lines) console.log(l);
    process.exit(r.ok ? 0 : 1);
  } catch (e) {
    console.log(`::error::${e.message}`);
    process.exit(2);
  }
}
