// The guest side of the rule 71 MCP row: seeds what the PRODUCT reads, so no product flag is needed. Read from origin/dyno/1.1.0-rc (a19d8d288, 2026-11-07):
//   - the switch: `mcp.json` in the app's config folder, a versioned JSON file ({"version":1,"enabled":true,"resources":true,"disabledTools":[],"allowedActions":[]})
//     (crates/paddock-mcp/src/control.rs McpSettings, crates/paddock-env VersionedFile);
//   - the keys: `mcp-keys.json` in the same config folder, an owner-only versioned file {"version":1,"keys":[{id, client, sha256, created_ms, revoked}]}
//     (crates/paddock-mcp/src/key_list.rs FileKeys, commit 3d03ede45; apps/dyno/app/src-tauri/src/mcp.rs). NOT the OS credential store: 1.0-era seeding
//     through security / secret-tool / cmdkey is gone. On Unix the file must be owned by the user running the app and not writable by group or others
//     (mode 600), and its folder not writable by others, or the list is refused and no key works. The key itself is never stored, only its SHA-256 (hex).
//     A key is `pmk_` + 64 hex digits (keys.rs).
//   - the app reads both at START (control.rs load_keys), so they are written while the app is stopped, then the app is started.
//   - the client: `DYNO_MCP_KEY=<key> dyno mcp` speaks JSON-RPC lines on stdio to the app's loopback listener found through `<config>/mcp-listener.json`.
// Guest only: never on the developer's Mac.
import { createHash, randomBytes } from 'node:crypto';
import { mcpInput, mcpStep } from './mcp-judges.mjs';

export const KEYS_FILE = 'mcp-keys.json';

/** A key in the product's format and the credential entry that makes the product accept it. */
export function makeKey(entropy = randomBytes(32), now = Date.now()) {
  const key = `pmk_${Buffer.from(entropy).toString('hex')}`;
  const entries = JSON.stringify([{ id: 1, client: 'vm-smoke', sha256: createHash('sha256').update(key).digest('hex'), created_ms: now, revoked: false }]);
  return { key, entries };
}

/** The versioned `mcp.json` with AI access on; `allowedActions` names the action tools to allow. */
export const settingsJson = (allowedActions = []) => JSON.stringify({ version: 1, enabled: true, resources: true, disabledTools: [], allowedActions });

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`; // POSIX single quote
const psq = (s) => `'${String(s).replace(/'/g, "''")}'`; // PowerShell single quote

/** The versioned key-list file the product reads (`entries` is the JSON list made by makeKey). */
export const keysFileJson = (entries) => JSON.stringify({ version: 1, keys: JSON.parse(entries) });

/**
 * The shell command that writes the key-list file next to `mcp.json`, owner-only (mode 600 on Unix, as the product demands; Windows: the per-user
 * config folder is already private). `dir` is the config folder found at run time.
 */
export function writeKeysCommand(os, dir, entries) {
  const json = keysFileJson(entries);
  if (os === 'windows') return `[IO.File]::WriteAllText((Join-Path ${psq(dir)} '${KEYS_FILE}'), ${psq(json)}, (New-Object Text.UTF8Encoding $false))`;
  const f = sq(`${dir}/${KEYS_FILE}`);
  return `(umask 077; printf %s ${sq(json)} > ${f}) && chmod 600 ${f}`;
}

/** The command that writes `mcp.json` next to the app's settings (the folder found at run time, never guessed). */
export function writeSettingsCommand(os, dir, json) {
  if (os === 'windows') return `[IO.File]::WriteAllText((Join-Path ${psq(dir)} 'mcp.json'), ${psq(json)}, (New-Object Text.UTF8Encoding $false))`;
  return `printf %s ${sq(json)} > ${sq(`${dir}/mcp.json`)}`;
}

/** Where `dyno mcp` runs: stdin from `input`, the key only in the environment (never an argument). */
export function mcpCommand(os, key, input, dyno) {
  if (os === 'windows') return `$env:DYNO_MCP_KEY = ${psq(key)}; ${psq(input)} | & ${psq(dyno)} mcp 2>&1 | Out-String; "EXIT=$LASTEXITCODE"`;
  return `printf %s ${sq(input)} | DYNO_MCP_KEY=${sq(key)} perl -e 'alarm 30; exec @ARGV' ${sq(dyno)} mcp 2>&1; echo "EXIT=$?"`;
}

/** `... EXIT=<n>` to { code, out }. */
export function splitExit(text) {
  const m = /EXIT=(-?\d+)\s*$/.exec(String(text ?? '').trim());
  return { code: m ? Number(m[1]) : null, out: String(text ?? '').replace(/EXIT=-?\d+\s*$/, '').trim() };
}

/** The discovery file's content to 'gone' | 'pid-fails' | 'live' given whether its pid is alive now. */
export function discoveryState(content, pidAlive) {
  if (!content || !content.trim()) return 'gone';
  try { const j = JSON.parse(content); return Number.isInteger(j.pid) && pidAlive(j.pid) ? 'live' : 'pid-fails'; } catch { return 'pid-fails'; }
}

/**
 * The guest primitives of mcpStep for one OS. `p`: { os, sh(cmd) -> {code,stdout}, configDir(), dyno (path of the dyno CLI), stopApp(), startApp(), killAgent(),
 * agentAlive() -> boolean, quitApp(), pidAlive(pid) -> boolean }. Every step that cannot run throws, which mcpStep reports as a broken check.
 */
export function makeMcpGuest(p) {
  const must = async (what, cmd) => { const r = await p.sh(cmd); if (r.code !== 0) throw new Error(`${what} failed (exit ${r.code}): ${String(r.stdout ?? '').slice(-120)}`); return r; };
  return {
    async enable() {
      const dir = await p.configDir();
      if (!dir) throw new Error('the app\'s config folder was not found (no settings.json yet: the app has not been started)');
      const { key, entries } = makeKey();
      await p.stopApp();
      await must('writing mcp.json', writeSettingsCommand(p.os, dir, settingsJson()));
      await must('writing the owner-only key list file', writeKeysCommand(p.os, dir, entries));
      await p.startApp();
      return key;
    },
    async mcp(key, input) { const r = splitExit((await p.sh(mcpCommand(p.os, key, input, p.dyno))).stdout); return r; },
    killAgent: () => p.killAgent(),
    restoreAgent: () => p.restoreAgent?.(),
    agentRestarted: async () => (await p.agentRestarted?.()) === true,
    agentDead: async () => !(await p.agentAlive()),
    quitApp: () => p.quitApp(),
    async discovery() {
      const dir = await p.configDir();
      const r = await p.sh(p.os === 'windows' ? `Get-Content (Join-Path ${psq(dir)} 'mcp-listener.json') -Raw -ErrorAction SilentlyContinue` : `cat ${sq(`${dir}/mcp-listener.json`)} 2>/dev/null`);
      const content = r.stdout ?? '';
      let alive = false;
      try { const pid = JSON.parse(content).pid; alive = Number.isInteger(pid) && (await p.pidAlive(pid)); } catch { /* not JSON */ }
      return discoveryState(content, () => alive);
    },
  };
}

export { mcpInput, mcpStep };
