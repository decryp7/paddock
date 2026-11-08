// Release row: after the agent is killed and the app relaunched, the Jobs panel must not keep "The jobs could not be read" once the agent is back
// (the shared Jobs-list store fix). Driver ports keep it OS-neutral and testable; the preconditions (the panel opens, the agent was killed and came back)
// are checked first and end the row as check-broken (ER-33), never as a pass.
import { makeResult } from './verdict.mjs';

export const JOBS_RESTART_TITLE = 'Release row: the Jobs panel recovers after the agent is killed and the app relaunched';
const BAD = /jobs could not be read/i;
const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });

/**
 * @param {{openJobs:()=>Promise<boolean>, jobsText:()=>Promise<string>, killAgent:()=>Promise<boolean>, relaunchApp:()=>Promise<void>, agentUp:()=>Promise<boolean>}} d
 * @param {{now:()=>number, sleep:(ms:number)=>Promise<void>}} c
 */
export async function jobsRestartStep(d, c, { settleMs = 60_000, agentWaitMs = 60_000, stepMs = 2000 } = {}) {
  const ev = [];
  const stop = (checks) => makeResult(JOBS_RESTART_TITLE, checks, ev, 'the Jobs panel recovered');
  if (!(await d.openJobs())) return stop([chk(false, 'precondition NOT met: the Jobs panel could not be opened before the kill', 'check')]);
  const before = await d.jobsText();
  ev.push(`panel before the kill: ${before || '(no job lines)'}`);
  if (BAD.test(before)) return stop([chk(false, 'precondition NOT met: the panel already says "could not be read" before the kill', 'check')]);
  if (d.startJob) { // the stricter case: a job is in flight when the agent dies
    const sj = await d.startJob();
    ev.push(`job started before the kill: ${sj ? 'yes' : 'no'}; panel: ${await d.jobsText()}`);
    if (!sj) return stop([chk(false, 'precondition NOT met: no job could be started before the kill', 'check')]);
  }
  if (!(await d.killAgent())) return stop([chk(false, 'precondition NOT met: the agent was not killed (no pid found)', 'check')]);
  await d.relaunchApp();
  const t0 = c.now();
  let up = false;
  while (c.now() - t0 < agentWaitMs) { if (await d.agentUp()) { up = true; break; } await c.sleep(stepMs); }
  if (!up) return stop([chk(false, `precondition NOT met: no live agent within ${agentWaitMs / 1000} s of the relaunch`, 'check')]);
  if (!(await d.openJobs())) return stop([chk(false, 'the Jobs panel could not be opened after the relaunch', 'check')]);
  const t1 = c.now();
  let last = '', sawBad = false, clearSince = null, recovered = false;
  while (c.now() - t1 < settleMs) {
    last = await d.jobsText();
    if (BAD.test(last)) { sawBad = true; clearSince = null; } else { clearSince ??= c.now(); if (c.now() - clearSince >= stepMs) { recovered = true; break; } }
    await c.sleep(stepMs);
  }
  if (d.diag) ev.push(...(await d.diag())); // always: the evidence of a pass is what a later failure is compared with
  ev.push(`seconds from the agent being back (and the panel opened) to a clear panel: ${recovered ? Math.round((clearSince - t1) / 1000) : 'never within ' + settleMs / 1000}`);
  ev.push(`panel after the agent is back: ${last || '(no job lines)'}${sawBad ? ' (the message showed for a while and then cleared)' : ''}`);
  return stop([
    chk(true, 'the agent was killed, the app relaunched and a live agent is back'),
    chk(recovered, recovered ? `the Jobs panel reads fine${sawBad ? ' after the message cleared' : ''}` : `the Jobs panel still says "The jobs could not be read" ${settleMs / 1000} s after the agent is back`),
  ]);
}
