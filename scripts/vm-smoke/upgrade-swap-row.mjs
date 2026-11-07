// Release row: "install 1.1.0 over a running older build" (product fix fix/dyno-agent-version-swap). An older build runs with the helper on; the new build is installed
// over it (macOS: replace the .app; Windows: the new setup; Linux: dpkg -i); the app is opened; within a bounded wait (60 s) the RUNNING dyno-agent and the helper must be
// the new version, and no process from the old binary may be left, found by path and start time (rule 53: never by name alone).
//
// ER-33 preconditions, each checked: the old build really ran (an agent answered, version read) and its version differs from the new one (else the row cannot tell the
// two apart), and the helper was on before. A precondition that does not hold is a broken check, not a pass; an old build that cannot be installed is NOT MEASURED.
import { makeResult } from './verdict.mjs';

export const TITLE_SWAP = 'Release row: installing the new build over a running older one swaps the agent and the helper within 60 s';
export const SWAP_WAIT_MS = 60_000;
const VERSION = /\b(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)\b/;

/** The first version in a command's output, or null. */
export const versionOf = (text) => VERSION.exec(String(text ?? ''))?.[1] ?? null;

/**
 * Pure judge. f: { oldVersion, newVersion, helperWasOn, agentVersion, helperVersion (null: unreadable, or no helper), helperReadable, tookMs, waited, procs:[{name,pid,exe,startMs}], installedAtMs, newExePrefix }.
 * A process counts as left over from the old binary when it started before the install finished (startMs < installedAtMs) and is still alive.
 */
export function judgeSwap(f) {
  const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });
  const buildsDiffer = Boolean(f.oldBuild && f.newBuild && f.oldBuild !== f.newBuild);
  const sameVersion = f.oldVersion === f.newVersion;
  const pre = [
    chk(Boolean(f.oldVersion), f.oldVersion ? `precondition: the older build ran and reported ${f.oldVersion}` : 'precondition NOT met: the older build\'s version could not be read', 'check'),
    chk(Boolean(f.oldVersion && f.newVersion && (f.oldVersion !== f.newVersion || buildsDiffer)), f.oldVersion !== f.newVersion ? `precondition: the old (${f.oldVersion}) and new (${f.newVersion}) versions differ` : buildsDiffer ? `precondition: the old and new build report the same version (${f.newVersion}) but are different builds (agent file ${f.oldBuild?.slice(0, 12)} then ${f.newBuild?.slice(0, 12)}); the swap is judged by process identity` : `precondition NOT met: the old and new build report the same version (${f.newVersion}) and the agent file is the same (or unreadable), so a swap cannot be seen`, 'check'),
    chk(f.helperWasOn === true, f.helperWasOn ? 'precondition: the helper was on before the install' : 'precondition NOT met: the helper was not on before the install', 'check'),
  ];
  if (pre.some((c) => !c.ok)) return pre;
  const checks = [...pre];
  const fresh = (re) => (f.procs ?? []).some((p) => re.test(p.name) && p.startMs >= f.installedAtMs);
  if (sameVersion) {
    // Same version string, different build: the version cannot tell old from new, so the swap is judged by process identity (a process that started after the install).
    checks.push(chk(fresh(/agent/), fresh(/agent/) ? 'the running dyno-agent started after the install (it was swapped)' : 'no dyno-agent that started after the install is running: the old agent was not swapped'));
    if (f.helperReadable || f.helperWasOn) checks.push(chk(fresh(/helper/), fresh(/helper/) ? 'the running helper started after the install (it was swapped)' : 'no helper that started after the install is running: the old helper was not swapped'));
  } else {
  checks.push(chk(f.agentVersion === f.newVersion, f.agentVersion === f.newVersion ? `the running agent reports ${f.newVersion} ${Math.round((f.tookMs ?? 0) / 1000)} s after the app was opened` : `the running agent reports ${f.agentVersion ?? 'nothing'} after ${Math.round((f.waited ?? 0) / 1000)} s, not ${f.newVersion}: the old agent was not swapped`));
  if (!f.helperReadable) checks.push(chk(false, 'the helper\'s version could not be read, so its swap was not measured', 'check'));
  else checks.push(chk(f.helperVersion === f.newVersion, f.helperVersion === f.newVersion ? `the helper reports ${f.newVersion}` : `the helper reports ${f.helperVersion ?? 'nothing'}, not ${f.newVersion}: the old helper was not swapped`));
  }
  const old = (f.procs ?? []).filter((p) => p.startMs < f.installedAtMs);
  checks.push(chk(old.length === 0, old.length ? `${old.length} process(es) from before the install still run: ${old.map((p) => `${p.name} pid ${p.pid} ${p.exe} started ${new Date(p.startMs).toISOString()}`).join('; ')}` : 'no process started before the install is still running (by pid, path and start time)'));
  return checks;
}

/**
 * The step. p (primitives of one OS): { now(), sleep(ms), installOld() , startOldWithHelper() -> {oldVersion, helperWasOn}, installNew() -> installedAtMs, openApp(), newVersion(),
 * agentVersion() , helperVersion() -> {version, readable}, procs() -> [{name,pid,exe,startMs}] }. Any primitive that throws makes the row NOT MEASURED with its message.
 */
export async function upgradeSwapStep(p) {
  let ctx;
  try {
    await p.installOld();
    const old = await p.startOldWithHelper();
    const newVersion = await p.newVersion();
    const oldBuild = p.buildId ? await p.buildId().catch(() => null) : null;
    const installedAtMs = await p.installNew();
    const newBuild = p.buildId ? await p.buildId().catch(() => null) : null;
    await p.openApp();
    const t0 = p.now();
    let agentVersion = null, helper = { version: null, readable: false };
    for (;;) {
      agentVersion = await p.agentVersion();
      helper = await p.helperVersion();
      const procs = await p.procs();
      const sameVersion = old.oldVersion === newVersion;
      const swapped = procs.length > 0 && procs.every((x) => x.startMs >= installedAtMs) && procs.some((x) => /agent/.test(x.name)) && (!old.helperWasOn || procs.some((x) => /helper/.test(x.name)));
      if (sameVersion ? swapped : (agentVersion === newVersion && (!helper.readable || helper.version === newVersion))) break;
      if (p.now() - t0 >= SWAP_WAIT_MS) break;
      await p.sleep(2000);
    }
    ctx = { ...old, oldBuild, newBuild, newVersion, installedAtMs, agentVersion, helperVersion: helper.version, helperReadable: helper.readable, tookMs: p.now() - t0, waited: p.now() - t0, procs: await p.procs() };
  } catch (e) {
    return { title: TITLE_SWAP, status: 'not-measured', cls: 'check', summary: `not measured: ${String(e.message ?? e).split('\n')[0]}`, checks: [{ ok: false, kind: 'check', detail: String(e.message ?? e) }], evidence: [] };
  }
  return makeResult(TITLE_SWAP, judgeSwap(ctx), [JSON.stringify({ ...ctx, procs: ctx.procs.length })], 'the agent and the helper are the new build within 60 s and nothing of the old one runs');
}

/** A row nobody can drive in this run, with its reason. */
export const swapNotDriven = (reason) => ({ title: TITLE_SWAP, status: 'not-measured', cls: 'check', summary: `not measured: ${reason}`, checks: [{ ok: false, kind: 'check', detail: reason }], evidence: [] });
