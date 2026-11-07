// The guest primitives of the upgrade-swap row (upgrade-swap-row.mjs) for macOS and Linux. `exec(cmd, opts?) -> Promise<{code, stdout}>` runs a bash command in the guest as
// the session user. Versions of the RUNNING agent come from `dyno status` text (an on-disk `--version` would already show the new file for an old process); the helper's
// from its own `--version`. Processes carry their executable path and a start time computed from the guest clock, so an old process is found by path + start time.
// UNVERIFIED on a live guest until a run shows the status text carries a version (otherwise the agent check fails with "reports nothing", which is visible, not a pass).
import { etimeSeconds } from './judge.mjs';
import { versionOf } from './upgrade-swap-row.mjs';

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;

async function guestProcs(exec, nowMs, exeCmd) {
  const r = await exec(`ps -axo pid=,etime=,comm= | grep -E 'dyno-(app|agent|helper)|net\\.decryptology\\.dyno\\.helper' | grep -v grep`);
  const out = [];
  for (const l of r.stdout.split('\n').map((x) => x.trim()).filter(Boolean)) {
    const [pid, etime, ...rest] = l.split(/\s+/);
    const exe = (await exec(exeCmd(pid))).stdout.trim() || rest.join(' ');
    out.push({ name: rest.join(' ').split('/').at(-1), pid: Number(pid), exe, startMs: nowMs - etimeSeconds(etime) * 1000 });
  }
  return out;
}
const guestNowMs = async (exec) => Number((await exec('date +%s')).stdout.trim()) * 1000;

/** macOS: `gui` a MacGui, `check` the Check (install), `share` the guest path of the staged dmgs. */
export function macSwapPrimitives({ guest, gui, check, share, bundle, beforeDmg = 'before.dmg', afterDmg = 'installer.dmg', helperPath = '/Library/PrivilegedHelperTools/net.decryptology.dyno.helper' }) {
  const exec = (c, o) => guest.exec(c, o);
  const quit = () => exec(`osascript -e 'quit app "Dyno"' >/dev/null 2>&1; sleep 2; pkill -x dyno-app; pkill -x dyno-agent; true`);
  const install = async (name) => { const s = await check.install(`${share}/${name}`); if (s.status !== 'pass') throw new Error(`the build ${name} cannot be installed in this VM: ${s.summary}`); };
  const bin = (n) => `${bundle}/Contents/MacOS/${n}`;
  return {
    now: () => guest.now(), sleep: (ms) => guest.sleep(ms),
    async installOld() { await quit(); await install(beforeDmg); },
    async startOldWithHelper() {
      await exec(`open -a ${sq(bundle)}`);
      if (!(await gui.waitFor(() => gui.has(gui.app, '^Dashboard$', 'AXLink'), 90_000)).ok) throw new Error('the older build showed no window');
      const on = await gui.helperOn();
      const oldVersion = versionOf((await exec(`${sq(bin('dyno-agent'))} status 2>&1`)).stdout) ?? versionOf((await exec(`defaults read ${sq(`${bundle}/Contents/Info`)} CFBundleShortVersionString`)).stdout);
      return { oldVersion, helperWasOn: on.status === 'pass' };
    },
    async newVersion() {
      const r = await exec(`hdiutil detach /tmp/vmsmoke-swap >/dev/null 2>&1; mkdir -p /tmp/vmsmoke-swap; hdiutil attach -nobrowse -readonly -mountpoint /tmp/vmsmoke-swap ${sq(`${share}/${afterDmg}`)} >/dev/null 2>&1; defaults read /tmp/vmsmoke-swap/*.app/Contents/Info CFBundleShortVersionString; hdiutil detach /tmp/vmsmoke-swap >/dev/null 2>&1`);
      return versionOf(r.stdout);
    },
    async buildId() { return (await exec(`shasum -a 256 ${sq(bin('dyno-agent'))} | cut -c1-64`)).stdout.trim() || null; },
    async installNew() { await install(afterDmg); return guestNowMs(exec); }, // over the RUNNING older build: no quit first
    async openApp() { await exec(`open -a ${sq(bundle)}`); },
    async agentVersion() { return versionOf((await exec(`${sq(bin('dyno-agent'))} status 2>&1`)).stdout); },
    async helperVersion() { const r = await exec(`sudo ${sq(helperPath)} --version 2>&1`); const v = versionOf(r.stdout); return { version: v, readable: v !== null }; },
    async procs() { return guestProcs(exec, await guestNowMs(exec), (pid) => `lsof -p ${pid} -Fn 2>/dev/null | sed -n 's/^n//p' | grep -m1 -E 'dyno|helper'`); },
  };
}

/** Linux: `oldDeb` / `newDeb` are guest paths of the .deb files. */
export function linuxSwapPrimitives({ exec, sleep, now, oldDeb, newDeb, startApp }) {
  return {
    now, sleep,
    async installOld() { const r = await exec(`sudo DEBIAN_FRONTEND=noninteractive dpkg -i --force-downgrade ${sq(oldDeb)} 2>&1 | tail -3; dpkg-query -W -f='\${Version}\\n' dyno`); if (!/^\d+\.\d+\.\d+/.test(r.stdout.trim().split('\n').at(-1))) throw new Error(`the older .deb cannot be installed in this VM: ${r.stdout.trim().slice(-120)}`); },
    async startOldWithHelper() {
      await startApp();
      await exec('sudo /usr/bin/dyno-helper install --for uid:1000 >/dev/null 2>&1; true');
      let active = false;
      for (let i = 0; i < 20 && !active; i++) { active = (await exec('systemctl is-active dyno-helper.service')).stdout.trim() === 'active'; if (!active) await sleep(3000); }
      return { oldVersion: versionOf((await exec(`dpkg-query -W -f='\${Version}\\n' dyno`)).stdout), helperWasOn: active };
    },
    async buildId() { return (await exec('sha256sum /usr/bin/dyno-agent | cut -c1-64')).stdout.trim() || null; },
    async newVersion() { return versionOf((await exec(`dpkg-deb -f ${sq(newDeb)} Version`)).stdout); },
    async installNew() { const r = await exec(`sudo DEBIAN_FRONTEND=noninteractive dpkg -i ${sq(newDeb)} 2>&1 | tail -3`); if (r.code !== 0) throw new Error(`dpkg -i of the new .deb failed: ${r.stdout.trim().slice(-120)}`); return guestNowMs(exec); },
    async openApp() { await startApp(); },
    async agentVersion() { return versionOf((await exec('dyno status 2>&1')).stdout); },
    async helperVersion() { const v = versionOf((await exec('/usr/bin/dyno-helper --version 2>&1')).stdout); return { version: v, readable: v !== null }; },
    async procs() { return guestProcs(exec, await guestNowMs(exec), (pid) => `readlink /proc/${pid}/exe`); },
  };
}
