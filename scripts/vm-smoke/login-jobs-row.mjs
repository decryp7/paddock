// Row 5a of apps/dyno/docs/testing-manual.md (1.1.0 "Jobs view: changing Start at login shows Cancel while it works"): open Settings, change Start Dyno at login, the
// Jobs list shows the job with Cancel, Cancel ends it as Cancelled, a second change is accepted, and the switch ends where it began. One judge for all three OS; each OS
// brings a small driver over its own accessibility API (CDP on Windows, axdrive on macOS, AT-SPI on Linux). Pure over the driver and a clock (no sleeps of its own).
//
// A real change is quick on a quiet guest. The row cannot slow the service down without a product hook, so a change that ended before the Jobs list showed it is
// NOT MEASURED (with that reason), never a pass: ER-33 (the probe must have shown it can see the thing). When the driver can start a job that lasts (`startLongJob`: a Sustained
// processor benchmark, listed as "Running benchmarks"), the Jobs-list Cancel is measured on that job instead; the row passes only when the Start at login job itself was listed.
import { makeResult } from './verdict.mjs';

export const TITLE_5A = 'Row 5a: Start at login change shows in the Jobs list with Cancel';
export const JOB_NAME = /Start Dyno at login/i;
const CANCELLED = /cancell?ed/i;
export const LONG_JOB = /Running benchmarks/i;

/**
 * @param d driver: { readSwitch() -> 'on'|'off'|'unknown', pressSwitch() -> bool, openJobs() -> bool, jobsText() -> string, pressCancel() -> bool }
 * @param c clock: { now(), sleep(ms) } (guest time)
 */
export async function loginJobsRow(d, c, { seenGuardMs = 20_000, endGuardMs = 30_000, intervalMs = 400 } = {}) {
  const checks = [];
  const evidence = [];
  const until = async (fn, guard) => { const t0 = c.now(); for (;;) { const v = await fn(); if (v) return v; if (c.now() - t0 >= guard) return null; await c.sleep(intervalMs); } };
  const before = await d.readSwitch();
  if (before === 'unknown') return makeResult(TITLE_5A, [{ ok: false, kind: 'check', detail: 'the "Start Dyno at login" switch could not be found or read, so nothing was changed' }]);
  checks.push({ ok: true, info: true, detail: `"Start Dyno at login" was ${before}` });
  if (!(await d.pressSwitch())) return makeResult(TITLE_5A, [...checks, { ok: false, kind: 'check', detail: 'the "Start Dyno at login" switch could not be pressed' }]);
  // The switch edits a draft; "Save changes" starts the job (settings-page.ts). Pressed without waiting, so the running job can be seen.
  if (d.save) checks.push({ ok: await d.save(), kind: 'check', detail: 'pressed "Save changes" after the switch' });
  const opened = await d.openJobs();
  checks.push({ ok: opened, kind: 'check', detail: opened ? 'the Jobs list is open' : 'the Jobs list could not be opened' });
  const seen = opened ? await until(async () => { const t = await d.jobsText(); return JOB_NAME.test(t) ? t : null; }, seenGuardMs) : null;
  if (opened && !seen) {
    await until(async () => (await d.readSwitch()) !== before, endGuardMs);
    const after = await d.readSwitch();
    await restore(d, before, after, until, endGuardMs);
    const lj = d.startLongJob ? await longJobCancel(d, until, { seenGuardMs, endGuardMs }) : null;
    return { title: TITLE_5A, status: 'not-measured', cls: 'check', summary: 'not measured: the change finished before the Jobs list showed it (a quiet guest answers at once), so Cancel could not be pressed; the row needs a slow login service', checks: [...checks, { ok: false, kind: 'check', detail: `no "${JOB_NAME.source}" job in the Jobs list within ${seenGuardMs / 1000} s` }, ...(lj ? lj.checks.map((c) => ({ ...c, info: true, ok: true, detail: `(long job, information) ${c.detail}` })) : [])], evidence: [...evidence, ...(lj?.evidence ?? [])] };
  }
  if (!seen) return makeResult(TITLE_5A, checks, evidence);
  evidence.push(`jobs list: ${seen.slice(0, 300)}`);
  checks.push({ ok: true, detail: 'the Jobs list shows the Start Dyno at login job' });
  const cancelled = await d.pressCancel();
  if (!cancelled) {
    // Gone or finished already: it was too quick to cancel. The switch is put back first (the change ran to its end), then Cancel is measured on a job that lasts.
    const text = await d.jobsText();
    await until(async () => (await d.readSwitch()) !== before, endGuardMs);
    await restore(d, before, await d.readSwitch(), until, endGuardMs);
    const final = await d.readSwitch(); // read while Settings is still open (the long job leaves it for the Benchmarks page)
    const lj = d.startLongJob ? await longJobCancel(d, until, { seenGuardMs, endGuardMs }) : null;
    if (lj && lj.measured) {
      checks.push({ ok: true, info: true, detail: `the Start at login job had ended before Cancel could be pressed: ${String(text).slice(0, 160)}` }, ...lj.checks,
        { ok: final === before, kind: 'check', detail: final === before ? `the switch is back to ${before}` : `the switch could not be put back to ${before} (it reads ${final})` });
      return makeResult(TITLE_5A, checks, [...evidence, ...lj.evidence], 'the Start at login job showed in the Jobs list (it ended before Cancel could be pressed); Cancel measured on a long benchmark job: it ended as Cancelled');
    }
    return { title: TITLE_5A, status: 'not-measured', cls: 'check', summary: `not measured: the job showed in the list but had no Cancel button when pressed (it had already ended)${lj ? `; the long job: ${lj.why}` : ''}`, checks: [...checks, { ok: false, kind: 'check', detail: `no Cancel button on the job: ${String(text).slice(0, 160)}` }, ...(lj?.checks ?? []).map((c) => ({ ...c, kind: 'check' }))], evidence: [...evidence, ...(lj?.evidence ?? [])] };
  }
  const ended = await until(async () => { const t = await d.jobsText(); return CANCELLED.test(t) ? t : null; }, endGuardMs);
  checks.push({ ok: Boolean(ended), detail: ended ? 'Cancel pressed; the job ends as Cancelled' : `Cancel pressed but the job never showed Cancelled within ${endGuardMs / 1000} s` });
  if (ended) evidence.push(`after Cancel: ${ended.slice(0, 300)}`);
  // The second change is accepted and runs.
  const second = (await d.pressSwitch()) && (d.save ? await d.save() : true);
  checks.push({ ok: second, detail: second ? 'a second change is accepted (the switch can be pressed again)' : 'the switch could not be pressed again after Cancel' });
  const settled = second ? await until(async () => { const s = await d.readSwitch(); return s !== 'unknown' ? s : null; }, endGuardMs) : null;
  checks.push({ ok: Boolean(settled), kind: settled ? undefined : 'check', detail: settled ? `after the second change the switch reads ${settled}` : 'the switch could not be read after the second change' });
  await restore(d, before, settled ?? 'unknown', until, endGuardMs);
  const final = await d.readSwitch();
  checks.push({ ok: final === before, kind: 'check', detail: final === before ? `the switch is back to ${before}` : `the switch could not be put back to ${before} (it reads ${final}); later rows start from this state` });
  return makeResult(TITLE_5A, checks, evidence, 'the job showed with Cancel, ended as Cancelled, and a second change was accepted');
}

async function restore(d, before, now, until, guard) {
  // An unreadable switch is read again before anything is pressed: a press on a guess can flip a switch that was already right.
  if (now === 'unknown') { now = (await until(async () => { const v = await d.readSwitch(); return v !== 'unknown' ? v : null; }, guard)) ?? 'unknown'; }
  if (before !== 'unknown' && now !== 'unknown' && now !== before) { await d.pressSwitch(); if (d.apply) await d.apply(); else if (d.save) await d.save(); await until(async () => (await d.readSwitch()) === before, guard); }
}

/**
 * The Jobs-list Cancel on a job that lasts: start it, see "Running benchmarks" in the list, press the list's Cancel, see it end as Cancelled. `measured` is false when the job
 * could not be started, never showed, or had no Cancel to press (then it is not evidence either way); a Cancel that does not end the job is a measured failure.
 */
async function longJobCancel(d, until, { seenGuardMs, endGuardMs }) {
  const checks = [];
  const evidence = [];
  try {
    const st = await d.startLongJob();
    const started = st === true || st?.ok === true;
    if (st?.detail) evidence.push(`long job: ${st.detail}`);
    if (!started) return { measured: false, why: `the long benchmark job could not be started${st?.detail ? ` (${st.detail.slice(0, 160)})` : ''}`, checks: [{ ok: false, detail: `the long benchmark job could not be started${st?.detail ? `: ${st.detail.slice(0, 200)}` : ''}` }], evidence };
    await d.openJobs();
    const seen = await until(async () => { const t = await d.jobsText(); return LONG_JOB.test(t) ? t : null; }, seenGuardMs);
    if (!seen) return { measured: false, why: 'the benchmark job never showed in the Jobs list', checks: [{ ok: false, detail: `no "${LONG_JOB.source}" job in the Jobs list within ${seenGuardMs / 1000} s` }], evidence };
    evidence.push(`jobs list with the benchmark: ${seen.slice(0, 300)}`);
    checks.push({ ok: true, detail: 'a long job (Sustained processor benchmark) shows in the Jobs list' });
    if (!(await d.pressCancel())) return { measured: false, why: 'the benchmark job had no Cancel button in the Jobs list', checks: [...checks, { ok: false, detail: 'no Cancel button on the benchmark job in the Jobs list' }], evidence };
    const ended = await until(async () => { const t = await d.jobsText(); return CANCELLED.test(t) || !LONG_JOB.test(t) ? t : null; }, endGuardMs);
    const cancelled = Boolean(ended && CANCELLED.test(ended));
    checks.push({ ok: cancelled, detail: cancelled ? 'the Jobs list Cancel ended the benchmark job as Cancelled' : ended ? `the benchmark job left the list without saying Cancelled: ${ended.slice(0, 120)}` : `the benchmark job was still running ${endGuardMs / 1000} s after Cancel` });
    if (ended) evidence.push(`after Cancel: ${ended.slice(0, 300)}`);
    return { measured: true, checks, evidence };
  } finally {
    await d.stopLongJob?.().catch?.(() => {});
  }
}
