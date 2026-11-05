// The guest side of the rule 71 MCP row: seeds what the PRODUCT reads, so no product flag is needed. Read from origin/dyno/wp-int (2026-11-05):
//   - the switch: `mcp.json` in the app's config folder, a versioned JSON file ({"version":1,"enabled":true,"resources":true,"disabledTools":[],"allowedActions":[]})
//     (crates/paddock-mcp/src/control.rs McpSettings, crates/paddock-env VersionedFile);
//   - the keys: ONE credential-store entry, service "Dyno", account "mcp:keys" (paddock-secrets Name "mcp"/"keys"), whose value is the JSON list of
//     {id, client, sha256, created_ms, revoked}; the key itself is never stored, only its SHA-256 (hex). A key is `pmk_` + 64 hex digits (keys.rs).
//   - the app reads both at START (control.rs load_keys), so they are written while the app is stopped, then the app is started.
//   - the client: `DYNO_MCP_KEY=<key> dyno mcp` speaks JSON-RPC lines on stdio to the app's loopback listener found through `<config>/mcp-listener.json`.
// The credential-store commands are per OS (security / secret-tool / cmdkey) and UNVERIFIED on a real guest until a run shows the app accepting the key.
// Guest only: never on the developer's Mac.
import { createHash, randomBytes } from 'node:crypto';
import { mcpInput, mcpStep } from './mcp-judges.mjs';

export const KEY_SERVICE = 'Dyno';
export const KEY_ACCOUNT = 'mcp:keys';

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

/**
 * The shell command that stores the key entry in the OS credential store for the user of the session.
 * macOS: an entry made by /usr/bin/security gets the partition `apple-tool:` only, which the app (another signing identity) is not in, so the app blocks
 * on the keychain ("Dyno wants to use your confidential information") before it shows a window. Measured in a clone of the macOS image, 2026-11-05, with the
 * ad-hoc 1.1.0 build: `-T` alone, an empty partition list, `apple-tool:,apple:` and `...,unsigned:` (each with `-A` too) all still blocked (no window in 30 s);
 * `apple-tool:,apple:,cdhash:<the app's CDHash>` after the add opened the window in 4 s (with `-T <bundle>`) and 8 s (with `-A`). So the app's own CDHash is
 * read from the installed bundle in the guest and put into the partition list, with the login keychain password of the throw-away guest (`password`).
 * The product's own case (a key made by one build blocking another) is NOT covered by this: it is the release-rows.mjs measurement `keychainOtherBuild`.
 */
export function storeEntryCommand(os, entries, password = 'admin', bundle = '/Applications/Dyno.app') {
  if (os === 'macos') {
    return `H=$(codesign -dvvv ${sq(bundle)} 2>&1 | sed -n 's/^CDHash=//p'); [ -n "$H" ] && security add-generic-password -U -T ${sq(bundle)} -s ${KEY_SERVICE} -a ${sq(KEY_ACCOUNT)} -w ${sq(entries)} && security set-generic-password-partition-list -S "apple-tool:,apple:,cdhash:$H" -s ${KEY_SERVICE} -a ${sq(KEY_ACCOUNT)} -k ${sq(password)} >/dev/null`;
  }
  if (os === 'linux') return `printf %s ${sq(entries)} | secret-tool store --label=${KEY_SERVICE} service ${KEY_SERVICE} username ${sq(KEY_ACCOUNT)}`;
  // Windows Credential Manager: cmdkey refuses a network (SSH) logon ("Credentials cannot be saved from this logon session"), so it runs in the test user's
  // console session through a scheduled task (/it); the entry is passed base64 so no quoting can damage it, and the task's own output is read back.
  const b64 = Buffer.from(entries, 'utf8').toString('base64');
  const script = `$j = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${b64}')); cmdkey /generic:'${KEY_ACCOUNT}.${KEY_SERVICE}' /user:'${KEY_ACCOUNT}' ("/pass:" + $j.Replace('"', '\\"')) *> C:\\paddock-test\\vmcheck\\store-key.out; 'DONE' | Add-Content C:\\paddock-test\\vmcheck\\store-key.out`;
  return `Remove-Item C:\\paddock-test\\vmcheck\\store-key.out -ErrorAction SilentlyContinue; Set-Content C:\\paddock-test\\vmcheck\\store-key.ps1 ${psq(script)}; schtasks /create /tn DynoStoreKey /sc once /st 00:00 /ru paddock-test /it /f /tr "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\store-key.ps1" | Out-Null; schtasks /run /tn DynoStoreKey | Out-Null; for ($i = 0; $i -lt 30; $i++) { Start-Sleep 1; if ((Get-Content C:\\paddock-test\\vmcheck\\store-key.out -ErrorAction SilentlyContinue) -match 'DONE') { break } }; $o = Get-Content C:\\paddock-test\\vmcheck\\store-key.out -Raw -ErrorAction SilentlyContinue; $o; if ($o -notmatch 'successfully') { exit 1 }`;
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
      await must('storing the key entry in the credential store', storeEntryCommand(p.os, entries, p.password));
      await p.startApp();
      return key;
    },
    async mcp(key, input) { const r = splitExit((await p.sh(mcpCommand(p.os, key, input, p.dyno))).stdout); return r; },
    killAgent: () => p.killAgent(),
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
