// Rule 71 rows (feat/dyno-mcp-listener, feat/dyno-quit-stop-agent): Dyno's MCP listener seen from OUTSIDE the app through `DYNO_MCP_KEY=<key> dyno mcp`,
// and "Quit and stop measuring". Pure judges over what the guest printed; the step functions take the guest's primitives as injected functions, so a
// dead state (no answer, old numbers after the agent was killed, a listener file that survives Quit) is a fixture that must not pass. GUEST ONLY: the
// primitives run in the VM, never on the developer's Mac. Key seeding, the mcp.json switch and the Quit trigger are per OS (see the hand-off: they need
// the product's keyring entry format and a non-GUI trigger); until a runner supplies them the rows are missing and the run is INCOMPLETE.
import { makeResult } from './verdict.mjs';

/** The ten tools Dyno lists with AI access on (mcp.rs); `wake_device` and `keep_awake` are actions, listed only when the policy allows them. */
export const MCP_TOOLS = ['alarms', 'cpu_memory', 'disks', 'history', 'network_summary', 'power_state', 'sensors', 'system_summary', 'top_processes', 'wol_devices'];
export const MCP_ACTIONS = ['wake_device', 'keep_awake'];

const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });

/** The JSON-RPC requests sent through `dyno mcp` on stdin: initialize, initialized, tools/list, then one call per entry of `calls`. */
export function mcpInput(calls = [{ name: 'system_summary', arguments: {} }]) {
  const msgs = [
    { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'vm-smoke', version: '1' } } },
    { jsonrpc: '2.0', method: 'notifications/initialized' },
    { jsonrpc: '2.0', id: 2, method: 'tools/list' },
    ...calls.map((c, i) => ({ jsonrpc: '2.0', id: 3 + i, method: 'tools/call', params: c })),
  ];
  return `${msgs.map((m) => JSON.stringify(m)).join('\n')}\n`;
}

/** stdout lines to responses by id (a line that is not JSON is ignored). */
export function parseResponses(stdout) {
  const by = new Map();
  for (const l of String(stdout ?? '').split('\n')) { try { const j = JSON.parse(l); if (j && j.id !== undefined) by.set(j.id, j); } catch { /* not JSON */ } }
  return by;
}

/** The payload of a tools/call result: `structuredContent`, or the JSON in the first text content. null when it is an error or unreadable. */
export function callPayload(resp) {
  if (!resp || resp.error || resp.result?.isError) return null;
  if (resp.result?.structuredContent) return resp.result.structuredContent;
  try { return JSON.parse(resp.result?.content?.[0]?.text ?? ''); } catch { return null; }
}

/** tools/list: all ten read tools, nothing unknown, the actions only when allowed. */
export function judgeToolsList(resp, { actionsAllowed = false } = {}) {
  const names = (resp?.result?.tools ?? []).map((t) => t.name);
  if (!names.length) return [chk(false, `tools/list returned no tools (${resp?.error ? `error: ${String(resp.error.message ?? '').slice(0, 80)}` : 'no answer'})`)];
  const missing = MCP_TOOLS.filter((n) => !names.includes(n));
  const extra = names.filter((n) => !MCP_TOOLS.includes(n) && !(actionsAllowed && MCP_ACTIONS.includes(n)));
  return [
    chk(missing.length === 0, missing.length ? `tools/list lacks ${missing.join(', ')}` : `tools/list names all ${MCP_TOOLS.length} tools`),
    chk(extra.length === 0, extra.length ? `tools/list names ${extra.join(', ')}, which are not allowed here` : `no action tool is listed${actionsAllowed ? ' beyond the allowed ones' : ' (actions are off)'}`),
  ];
}

/** system_summary: a CPU percent in 0..100, memory used below total, uptime above 0. */
export function judgeSystemSummary(resp) {
  const p = callPayload(resp);
  if (!p) return [chk(false, `system_summary gave no reading (${resp?.error?.message ?? resp?.result?.content?.[0]?.text ?? 'no answer'})`)];
  const cpu = p.cpu?.percent, used = p.memory?.usedBytes, total = p.memory?.totalBytes, up = p.uptimeSeconds;
  return [
    chk(typeof cpu === 'number' && cpu >= 0 && cpu <= 100, `cpu percent ${cpu} (must be 0 to 100)`),
    chk(Number.isFinite(used) && Number.isFinite(total) && total > 0 && used > 0 && used < total, `memory used ${used} of ${total} (used must be above 0 and below total)`),
    chk(Number.isFinite(up) && up > 0, `uptime ${up} s (must be above 0)`),
  ];
}

/** After the agent was killed the call must be an error that says so, never the earlier numbers. */
export function judgeAfterAgentKill(resp, { agentDead, restarted = false }) {
  // launchd/systemd restart a killed agent within seconds (measured: a new pid after every plain kill -9, also after `launchctl disable`): a reading from a NEW agent is not old numbers, but then the error path was not seen.
  if (agentDead && restarted && resp && callPayload(resp)) return [chk(false, 'the supervisor had already started a new agent when the call was made, so the error path with the agent gone was not tested', 'check')];
  if (!agentDead) return [chk(false, 'the agent was still running when the second call was made, so "no old numbers" was not tested', 'check')];
  if (!resp) return [chk(false, 'no answer at all after the agent was killed (a hang is not an error message)')];
  const stale = callPayload(resp);
  const text = JSON.stringify(resp.error ?? resp.result ?? {});
  return [
    chk(!stale, stale ? 'the call returned a reading with the agent dead: old numbers' : 'no reading was returned with the agent dead'),
    chk(/agent|not running|not connected|unavailable|stopped/i.test(text) && Boolean(resp.error || resp.result?.isError), `the error says the agent is gone: ${text.slice(0, 120)}`),
  ];
}

/** After Quit: `dyno mcp` says Dyno isn't open and exits 1; mcp-listener.json is gone or its pid fails verification. */
export function judgeAfterQuit({ code, out, discovery }) {
  return [
    chk(code === 1, `dyno mcp exit ${code} (expected 1)`),
    chk(/isn.t open/i.test(out ?? ''), /isn.t open/i.test(out ?? '') ? 'dyno mcp says Dyno isn\'t open' : `dyno mcp did not say "isn't open": ${String(out ?? '').slice(0, 100) || '(nothing)'}`),
    chk(discovery === 'gone' || discovery === 'pid-fails', discovery === 'gone' ? 'mcp-listener.json is gone' : discovery === 'pid-fails' ? 'mcp-listener.json is left but its pid/start time fails verification' : `mcp-listener.json is still there and names a live app (${discovery})`),
  ];
}

/**
 * "Quit and stop measuring": for 60 s after the action no Dyno process (app, agent) runs; reopening brings the agent back; a reboot brings it back.
 * @param {{samples:{t:number,count:number}[], reopened:number|null, rebooted:number|null}} f counts of Dyno processes; null: not measured
 */
export function judgeQuitStop({ samples, reopened, rebooted }) {
  const span = samples.length ? samples.at(-1).t - samples[0].t : 0;
  // A process may still be exiting in the first seconds after the click: only a Dyno process later than GRACE_S counts, and the time to reach zero is reported.
  const GRACE_S = 20;
  const alive = samples.filter((s) => s.count > 0 && s.t > GRACE_S);
  return [
    chk(span >= 60 && samples.length >= 3, `observed ${samples.length} samples over ${span} s (60 s needed)`, span >= 60 && samples.length >= 3 ? undefined : 'check'),
    chk(alive.length === 0, alive.length ? `a Dyno process was running ${alive[0].t} s after Quit and stop (${alive[0].count})` : 'no Dyno process for the whole window'),
    reopened === null ? chk(false, 'reopening was not tried', 'check') : chk(reopened > 0, reopened > 0 ? 'reopening Dyno brought the agent back' : 'reopening Dyno did not bring the agent back'),
    rebooted === null ? chk(false, 'the reboot was not tried', 'check') : chk(rebooted > 0, rebooted > 0 ? 'a reboot brought the agent back (the service was not removed)' : 'after a reboot the agent did not come back'),
  ];
}

/**
 * The MCP step. `g` primitives (guest only): enable() sets mcp.json enabled and seeds a key (returns the key), mcp(key, input, env) runs `dyno mcp`
 * -> {code, out}, killAgent(), agentDead(), quitApp(), discovery() -> 'gone' | 'pid-fails' | 'live'.
 */
export async function mcpStep(g) {
  const title = 'Rule 71: Dyno MCP from outside the app (dyno mcp)';
  const checks = [];
  let key;
  try { key = await g.enable(); } catch (e) { return makeResult(title, [chk(false, `could not enable AI access and seed a key in the guest: ${e.message}`, 'check')]); }
  if (!key) return makeResult(title, [chk(false, 'no key was seeded in the guest, so dyno mcp cannot authenticate', 'check')]);
  const first = await g.mcp(key, mcpInput());
  if (first.code === 127 || first.code === null) return makeResult(title, [chk(false, `dyno mcp could not be run in the guest (exit ${first.code}: ${String(first.out ?? '').slice(0, 100)}): the CLI path or the shell is wrong, not the product`, 'check')]);
  const r1 = parseResponses(first.out);
  checks.push(...judgeToolsList(r1.get(2)), ...judgeSystemSummary(r1.get(3)));
  await g.killAgent();
  const second = parseResponses((await g.mcp(key, mcpInput())).out);
  checks.push(...judgeAfterAgentKill(second.get(3), { agentDead: await g.agentDead(), restarted: await g.agentRestarted?.() }));
  await g.quitApp();
  const third = await g.mcp(key, mcpInput());
  checks.push(...judgeAfterQuit({ code: third.code, out: third.out, discovery: await g.discovery() }));
  await g.restoreAgent?.();
  return makeResult(title, checks, [], 'ten tools listed, a live system_summary, an error with the agent dead, "isn\'t open" after Quit');
}

/** The Quit-and-stop step; `g`: trigger() (a menu click over AX/VNC), count() Dyno processes, sleep(ms), reopen(), reboot(). */
export async function quitStopStep(g) {
  const title = 'Rule 71: Quit and stop measuring stops the agent and it comes back';
  try { await g.trigger(); } catch (e) { return makeResult(title, [chk(false, `Quit and stop measuring could not be triggered: ${e.message}`, 'check')]); }
  const samples = [];
  const t0 = g.now();
  for (let i = 0; i < 7; i++) { samples.push({ t: Math.round((g.now() - t0) / 1000), count: await g.count() }); if (i < 6) await g.sleep(10_000); }
  const reopened = await g.reopen().catch(() => null);
  const rebooted = await g.reboot().catch(() => null);
  return makeResult(title, judgeQuitStop({ samples, reopened, rebooted }), [JSON.stringify(samples)], 'no Dyno process for 60 s; reopening and a reboot bring the agent back');
}
