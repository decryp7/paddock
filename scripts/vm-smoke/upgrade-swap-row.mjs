// Release row: "install the new build over a running older one" (fix/dyno-agent-version-swap and the dy.update.restart chip). Judged by what the product promises, not by pids:
//  - the AGENT: agent.json `build` equals the first 16 hex of the SHA-256 of the new bundle's dyno-agent within 60 s of opening the app, and no restart loop follows (at most one
//    agent that started after the install began, watched for 2 minutes). The boundary "before the install" is taken on the GUEST clock just before the installer starts.
//  - the HELPER and the APP are OS specific (extraChecks of the primitives): Windows by the helper file's hash and its refresh log line; macOS by the helper card (Update available,
//    "Update the helper" with one prompt, then the build matches), the "Restart Dyno to finish" chip and log line, and no process from before the install after Quit and reopen;
//    Linux by the dpkg result and a helper that restarted.
// ER-33 preconditions, each checked: the old build really ran (agent answered, version read), its agent file differs from the new bundle's (else old and new cannot be told apart) and the
// helper was on before. A precondition that does not hold is a broken check; an install that fails is NOT MEASURED with the installer's own output.
import { makeResult } from './verdict.mjs';

export const TITLE_SWAP = 'Release row: installing the new build over a running older one swaps the agent and the helper within 60 s';
export const SWAP_WAIT_MS = 60_000;
export const LOOP_SAMPLES = 12;
const VERSION = /\b(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)\b/;
const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });

/** The first version in a command's output, or null. */
export const versionOf = (text) => VERSION.exec(String(text ?? ''))?.[1] ?? null;

/**
 * Pure judge. f: { oldVersion, helperWasOn, oldAgentBuild16, expectedBuild16, agentBuild (last read), tookMs, agentStarts:[ids of agents that started after the boundary], extra:[checks] }.
 */
export function judgeSwap(f) {
  const pre = [
    chk(Boolean(f.oldVersion), f.oldVersion ? `precondition: the older build ran and reported ${f.oldVersion}` : 'precondition NOT met: the older build\'s version could not be read', 'check'),
    chk(Boolean(f.expectedBuild16) && Boolean(f.oldAgentBuild16) && f.oldAgentBuild16 !== f.expectedBuild16,
      f.expectedBuild16 && f.oldAgentBuild16 && f.oldAgentBuild16 !== f.expectedBuild16 ? `precondition: the old agent build (${f.oldAgentBuild16}) differs from the new bundle's (${f.expectedBuild16})` : `precondition NOT met: the old agent build (${f.oldAgentBuild16 ?? 'unreadable'}) and the new bundle's (${f.expectedBuild16 ?? 'unreadable'}) are the same or unreadable, so a swap cannot be seen`, 'check'),
    chk(f.helperWasOn === true, f.helperWasOn ? 'precondition: the helper was on before the install' : 'precondition NOT met: the helper was not on before the install', 'check'),
  ];
  if (pre.some((c) => !c.ok)) return pre;
  const checks = [...pre];
  checks.push(chk(f.agentBuild === f.expectedBuild16, f.agentBuild === f.expectedBuild16 ? `agent.json build is ${f.expectedBuild16} (the new bundle's) ${Math.round((f.tookMs ?? 0) / 1000)} s after the app was opened` : `agent.json build is ${f.agentBuild ?? 'unreadable'} after ${Math.round((f.tookMs ?? 0) / 1000)} s, not ${f.expectedBuild16}: the old agent was not swapped`));
  const n = f.agentStarts?.length ?? 0;
  checks.push(chk(n <= 1, n <= 1 ? `no restart loop: ${n} agent started after the install began, over ${LOOP_SAMPLES * 10 - 10} s of watching (${(f.agentStarts ?? []).join(', ') || 'none seen'})` : `restart loop: ${n} different agents started after the install began (${f.agentStarts.join(', ')})`));
  checks.push(...(f.extra ?? []));
  return checks;
}

/**
 * The step. p (primitives of one OS): { now(), sleep(ms), installOld(), startOldWithHelper() -> {oldVersion, helperWasOn}, oldAgentBuild16(), expectedAgentBuild16(), beginInstall() -> guest ms,
 * installNew() (throws with the installer output when it fails), openApp(), agentBuild16() -> string|null, agentProcs() -> [{pid,startMs}] (agents of this install), extraChecks({boundaryMs}) -> checks[] }.
 * Any primitive that throws makes the row NOT MEASURED with its message.
 */
export async function upgradeSwapStep(p) {
  let ctx;
  try {
    await p.installOld();
    const old = await p.startOldWithHelper();
    const oldAgentBuild16 = await p.oldAgentBuild16();
    const boundaryMs = await p.beginInstall();
    await p.installNew();
    const expectedBuild16 = await p.expectedAgentBuild16();
    await p.openApp();
    const t0 = p.now();
    let agentBuild = null;
    for (;;) {
      agentBuild = await p.agentBuild16();
      if (agentBuild === expectedBuild16 || p.now() - t0 >= SWAP_WAIT_MS) break;
      await p.sleep(2000);
    }
    const tookMs = p.now() - t0;
    const seen = new Map();
    for (let i = 0; i < LOOP_SAMPLES; i++) { for (const x of await p.agentProcs()) if (x.startMs >= boundaryMs - 2000) seen.set(String(x.pid), x); // keyed by pid: a start time computed from `ps` elapsed time jitters by a second between looks if (i < LOOP_SAMPLES - 1) await p.sleep(10_000); }
    const extra = p.extraChecks ? await p.extraChecks({ boundaryMs }) : [];
    ctx = { ...old, oldAgentBuild16, expectedBuild16, agentBuild, tookMs, agentStarts: [...seen.keys()], extra };
  } catch (e) {
    return { title: TITLE_SWAP, status: 'not-measured', cls: 'check', summary: `not measured: ${String(e.message ?? e).split('\n')[0]}`, checks: [{ ok: false, kind: 'check', detail: String(e.message ?? e) }], evidence: [] };
  }
  return makeResult(TITLE_SWAP, judgeSwap(ctx), [JSON.stringify({ ...ctx, extra: ctx.extra.length })], 'the agent is the new build within 60 s without a restart loop, and the OS specific parts hold');
}

/** A row nobody can drive in this run, with its reason. */
export const swapNotDriven = (reason) => ({ title: TITLE_SWAP, status: 'not-measured', cls: 'check', summary: `not measured: ${reason}`, checks: [{ ok: false, kind: 'check', detail: reason }], evidence: [] });
