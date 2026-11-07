// Release row (Dyno 1.1.0, build d92bbea40 on): the agent rewrites its discovery file (agent.json) within 30 s when it is deleted or names another agent.
// Steps: read agent.json (pid of the live agent), delete it; poll until it exists again naming the SAME live pid; then overwrite it with a copy that names another pid and
// poll again. The agent must not restart (same pid and start time throughout) and `dyno status` must answer at the end. A precondition that does not hold is a broken check (ER-33).
import { makeResult } from './verdict.mjs';

export const TITLE_DISCOVERY = 'Release row: a deleted or foreign agent.json is rewritten within 30 s';
export const REWRITE_LIMIT_MS = 30_000;
const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });
export const pidOf = (text) => { try { const n = Number(JSON.parse(String(text).slice(String(text).indexOf('{'))).pid); return Number.isInteger(n) && n > 0 ? n : null; } catch { return null; } };

/** Pure judge. f: { livePid, startBefore, startAfter, deleted:{rewritten, ms, pid}, foreign:{rewritten, ms, pid}, statusOk } */
export function judgeDiscovery(f) {
  const out = [];
  out.push(chk(Boolean(f.livePid), f.livePid ? `precondition: agent.json names the live agent (pid ${f.livePid})` : 'precondition NOT met: agent.json could not be read or names no pid', 'check'));
  if (!f.livePid) return out;
  for (const [label, r] of [['deleted', f.deleted], ['naming another agent', f.foreign]]) {
    if (!r) { out.push(chk(false, `agent.json ${label}: this part was not run`, 'check')); continue; }
    out.push(chk(r.rewritten && r.ms <= REWRITE_LIMIT_MS, r.rewritten ? `agent.json ${label}: rewritten after ${Math.round(r.ms / 1000)} s (limit 30 s)` : `agent.json ${label}: not rewritten within ${Math.round(r.ms / 1000)} s`));
    if (r.rewritten) out.push(chk(r.pid === f.livePid, r.pid === f.livePid ? `agent.json ${label}: the rewritten file names the live agent ${f.livePid}` : `agent.json ${label}: the rewritten file names pid ${r.pid}, the live agent is ${f.livePid}`));
  }
  out.push(chk(f.startBefore != null && f.startBefore === f.startAfter, f.startBefore === f.startAfter ? 'the agent did not restart (same pid and start time)' : `the agent restarted (start ${f.startBefore} then ${f.startAfter})`));
  out.push(chk(f.statusOk === true, f.statusOk ? 'dyno status answers after the rewrites' : 'dyno status does not answer after the rewrites'));
  return out;
}

/** p: { readFile()->text|'' , deleteFile(), writeFile(text), agent()->{pid,start}|null, statusOk()->bool, now(), sleep(ms) } */
export async function discoveryStep(p) {
  try {
    const text = await p.readFile();
    const livePid = pidOf(text);
    const a0 = await p.agent();
    if (!livePid || !a0) return makeResult(TITLE_DISCOVERY, judgeDiscovery({ livePid: null }), [], '');
    const wait = async () => {
      const t0 = p.now();
      for (;;) {
        const t = await p.readFile(); const pid = pidOf(t);
        if (pid) return { rewritten: true, ms: p.now() - t0, pid };
        if (p.now() - t0 >= REWRITE_LIMIT_MS + 15_000) return { rewritten: false, ms: p.now() - t0, pid: null };
        await p.sleep(2000);
      }
    };
    await p.deleteFile();
    const deleted = await wait();
    // naming another agent: the same file with the pid replaced
    const other = String(text).replace(/("pid"\s*:\s*)\d+/, `$1${livePid + 7919}`);
    await p.writeFile(other);
    const t1 = p.now(); let foreign = { rewritten: false, ms: 0, pid: null };
    for (;;) {
      const pid = pidOf(await p.readFile());
      if (pid === livePid) { foreign = { rewritten: true, ms: p.now() - t1, pid }; break; }
      if (p.now() - t1 >= REWRITE_LIMIT_MS + 15_000) { foreign = { rewritten: false, ms: p.now() - t1, pid }; break; }
      await p.sleep(2000);
    }
    const a1 = await p.agent();
    return makeResult(TITLE_DISCOVERY, judgeDiscovery({ livePid, startBefore: `${a0.pid}@${a0.start}`, startAfter: a1 ? `${a1.pid}@${a1.start}` : null, deleted, foreign, statusOk: await p.statusOk() }), [], 'the discovery file is rewritten within 30 s in both cases and the agent keeps running');
  } catch (e) {
    return { title: TITLE_DISCOVERY, status: 'not-measured', cls: 'check', summary: `not measured: ${String(e.message ?? e).split('\n')[0]}`, checks: [{ ok: false, kind: 'check', detail: String(e.message ?? e) }], evidence: [] };
  }
}

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
/** macOS and Linux: `exec(cmd)->{code,stdout}` runs as the session user; `file` the agent.json path (shell syntax allowed, e.g. "$HOME/..."), `dyno` the CLI. */
export function unixDiscoveryPrims({ exec, sleep, now, file, dyno }) {
  return {
    now, sleep,
    readFile: async () => (await exec(`cat "${file}" 2>/dev/null`)).stdout,
    deleteFile: async () => { await exec(`rm -f "${file}"`); },
    writeFile: async (t) => { await exec(`printf %s ${sq(t)} > "${file}"`); },
    agent: async () => { const r = (await exec(`pid=$(pgrep -x dyno-agent | head -1); [ -n "$pid" ] && echo "$pid $(ps -o lstart= -p $pid | sed 's/ *$//' | tr ' ' '_')"`)).stdout.trim().split(/\s+/); return r[0] ? { pid: Number(r[0]), start: r[1] } : null; },
    statusOk: async () => (await exec(`${dyno} status >/dev/null 2>&1; echo $?`)).stdout.trim().split('\n').at(-1) === '0',
  };
}

/** Windows (PowerShell over ssh as the session user). */
export function windowsDiscoveryPrims({ ssh, sleep, now, dyno = 'C:\\Program Files\\Dyno\\dyno.exe', file = 'C:\\Users\\paddock-test\\AppData\\Local\\Dyno\\agent.json' }) {
  return {
    now, sleep,
    readFile: async () => ssh(`Get-Content -Raw '${file}' -ErrorAction SilentlyContinue`),
    deleteFile: async () => { ssh(`Remove-Item '${file}' -Force -ErrorAction SilentlyContinue`); },
    writeFile: async (t) => { ssh(`[IO.File]::WriteAllText('${file}', '${t.replace(/'/g, "''")}', (New-Object Text.UTF8Encoding $false))`); },
    agent: async () => { const l = ssh("$p = Get-Process dyno-agent -ErrorAction SilentlyContinue | Select-Object -First 1; if ($p) { '' + $p.Id + ' ' + $p.StartTime.ToFileTimeUtc() }").trim().split('\n').at(-1).split(/\s+/); return /^\d+$/.test(l[0] ?? '') ? { pid: Number(l[0]), start: l[1] } : null; },
    statusOk: async () => /"cpuPercent"/.test(ssh(`& '${dyno}' status --json 2>$null`)),
  };
}
