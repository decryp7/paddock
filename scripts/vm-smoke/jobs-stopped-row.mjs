// Release row (post-1.1.0): the Jobs panel's own recovery. The app is quit, the agent is killed, the supervisor's new agent is stopped (SIGSTOP, identity
// verified by pid and start time) and the app is launched, so its first Jobs read fails or times out and the panel says "The jobs could not be read". After
// about 10 s the agent is continued; the panel must clear by itself (no Retry press) within the limit and must not show the error again.
// ER-33: if the error was never seen while the agent was stopped, the clearing proves nothing and the row is check-broken. The agent is continued in a finally.
import { makeResult } from './verdict.mjs';

export const JOBS_STOPPED_TITLE = 'Release row: the Jobs panel clears by itself after the first read failed (agent stopped, then continued)';
const BAD = /jobs could not be read/i;
const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });

/**
 * @param {{quitApp:()=>Promise<void>, killLaunchAndStop:()=>Promise<{pid:number,start:string}|null>, openJobs:()=>Promise<boolean>,
 *   jobsText:()=>Promise<string>, cont:(a:{pid:number,start:string})=>Promise<void>, guestClock:()=>Promise<string>}} d
 */
export async function jobsStoppedStep(d, c, { holdMs = 10_000, clearMaxMs = 10_000, stepMs = 1000, flashWatchMs = 5000 } = {}) {
  const ev = [];
  const out = (checks) => makeResult(JOBS_STOPPED_TITLE, checks, ev, 'the panel cleared by itself');
  let agent = null;
  try {
    await d.quitApp();
    agent = await d.killLaunchAndStop(); // kills the old agent, launches the app, stops the agent that appears (nothing respawns an agent while the app is quit)
    if (!agent) return out([chk(false, 'precondition NOT met: no new agent appeared after the launch to be stopped', 'check')]);
    ev.push(`new agent ${agent.pid} (started ${agent.start}) stopped at guest time ${await d.guestClock()}`);
    const opened = await d.openJobs();
    const t0 = c.now(); let errorAt = null; let samples = [];
    while (c.now() - t0 < holdMs) {
      const text = await d.jobsText();
      samples.push(`+${Math.round((c.now() - t0) / 1000)}s ${BAD.test(text) ? 'ERROR' : 'clear'}`);
      if (errorAt === null && BAD.test(text)) errorAt = c.now();
      await c.sleep(stepMs);
    }
    ev.push(`panel while the agent is stopped (opened ${opened}): ${samples.join(', ')}`);
    ev.push(`error first shown ${errorAt === null ? 'never' : `${Math.round((errorAt - t0) / 1000)} s after the launch`}`);
    const contClock = await d.guestClock();
    const contAt = c.now();
    await d.cont(agent); agent = null;
    ev.push(`SIGCONT at guest time ${contClock}`);
    let clearedAt = null; samples = [];
    while (c.now() - contAt < clearMaxMs + flashWatchMs) {
      const bad = BAD.test(await d.jobsText());
      samples.push(`+${Math.round((c.now() - contAt) / 1000)}s ${bad ? 'ERROR' : 'clear'}`);
      if (!bad && clearedAt === null) clearedAt = c.now();
      if (clearedAt !== null && bad) { ev.push(`panel after SIGCONT: ${samples.join(', ')}`); return out([chk(errorAt !== null, 'error seen while stopped', 'check'), chk(false, 'the error showed again after the panel had cleared')]); }
      if (clearedAt === null && c.now() - contAt >= clearMaxMs) break;
      if (clearedAt !== null && c.now() - clearedAt >= flashWatchMs) break;
      await c.sleep(stepMs);
    }
    ev.push(`panel after SIGCONT: ${samples.join(', ')}`);
    if (errorAt === null) return out([chk(false, 'precondition NOT met: "The jobs could not be read" never showed while the agent was stopped, so the clearing proves nothing', 'check')]);
    const secs = clearedAt === null ? null : Math.round((clearedAt - contAt) / 1000);
    ev.push(`seconds from SIGCONT to a clear panel: ${secs ?? 'never within ' + clearMaxMs / 1000}`);
    return out([chk(true, 'the error showed while the agent was stopped'), chk(clearedAt !== null, clearedAt !== null ? `the panel cleared by itself ${secs} s after SIGCONT, with no Retry pressed` : `the panel still said "The jobs could not be read" ${clearMaxMs / 1000} s after SIGCONT`)]);
  } finally {
    if (agent) await d.cont(agent).catch(() => {});
  }
}
