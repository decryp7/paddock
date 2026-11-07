// The guest primitives of the upgrade-swap row (upgrade-swap-row.mjs) for macOS and Linux. `exec(cmd, opts?) -> Promise<{code, stdout}>` runs a bash command in the guest as the session
// user. Times are the GUEST clock (the boundary "before the install" is read on the guest just before the installer starts). The agent's build is agent.json's `build` field, which the
// product sets to the first 16 hex of the SHA-256 of its agent file; the expected value is that hash of the new bundle's dyno-agent.
import { etimeSeconds } from './judge.mjs';

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });
const guestNowMs = async (exec) => Number((await exec('python3 -c "import time;print(int(time.time()*1000))" 2>/dev/null || date +%s000')).stdout.trim().split('\n').at(-1));
const build16 = (text) => { try { const b = JSON.parse(String(text).slice(String(text).indexOf('{'))).build; return typeof b === 'string' && /^[0-9a-f]{16}$/.test(b) ? b : null; } catch { return null; } };

async function agentProcs(exec, nowMs) {
  const r = await exec(`ps -axo pid=,etime=,command= | grep -E 'dyno-agent( run)?$' | grep -v grep`);
  return r.stdout.split('\n').map((l) => l.trim()).filter(Boolean).map((l) => { const [pid, etime] = l.split(/\s+/); return { pid: Number(pid), startMs: nowMs - etimeSeconds(etime) * 1000 }; });
}

/** macOS: `gui` a MacGui, `check` the Check (install), `share` the guest path of the staged dmgs. */
export function macSwapPrimitives({ guest, gui, check, share, bundle, beforeDmg = 'before.dmg', afterDmg = 'installer.dmg', helperFile = '/Library/PrivilegedHelperTools/net.decryptology.dyno.helper' }) {
  const exec = (c, o) => guest.exec(c, o);
  const bin = (n) => `${bundle}/Contents/MacOS/${n}`;
  const data = '"$HOME/Library/Application Support/Dyno"';
  const quit = () => exec(`osascript -e 'quit app "Dyno"' >/dev/null 2>&1; sleep 2; pkill -x dyno-app; pkill -x dyno-agent; true`);
  const install = async (name) => { const s = await check.install(`${share}/${name}`); if (s.status !== 'pass') throw new Error(`the build ${name} cannot be installed in this VM: ${s.summary}`); };
  const helperProcs = async (nowMs) => (await exec(`ps -axo pid=,etime=,command= | grep 'net.decryptology.dyno.helper' | grep -v grep`)).stdout.split('\n').map((l) => l.trim()).filter(Boolean).map((l) => { const [pid, etime] = l.split(/\s+/); return { pid: Number(pid), startMs: nowMs - etimeSeconds(etime) * 1000 }; });
  return {
    now: () => guest.now(), sleep: (ms) => guest.sleep(ms),
    async installOld() { await quit(); await install(beforeDmg); },
    async startOldWithHelper() {
      await exec(`open -a ${sq(bundle)}`);
      if (!(await gui.waitFor(() => gui.has(gui.app, '^Dashboard$', 'AXLink'), 90_000)).ok) throw new Error('the older build showed no window');
      const on = await gui.helperOn();
      const v = (await exec(`defaults read ${sq(`${bundle}/Contents/Info`)} CFBundleShortVersionString`)).stdout.trim();
      return { oldVersion: /^\d+\.\d+\.\d+/.test(v) ? v : null, helperWasOn: on.status === 'pass' };
    },
    async oldAgentBuild16() { return build16((await exec(`cat ${data}/agent.json`)).stdout); },
    async expectedAgentBuild16() {
      const r = await exec(`mp=/tmp/vmsmoke-swap-$$; mkdir -p $mp; hdiutil attach -nobrowse -readonly -mountpoint $mp ${sq(`${share}/${afterDmg}`)} >/dev/null 2>&1 && shasum -a 256 $mp/${bundle.split('/').at(-1)}/Contents/MacOS/dyno-agent | cut -c1-16; hdiutil detach $mp -force >/dev/null 2>&1; rmdir $mp 2>/dev/null; true`);
      const h = r.stdout.trim().split('\n').at(-1);
      return /^[0-9a-f]{16}$/.test(h) ? h : null;
    },
    async beginInstall() { return guestNowMs(exec); },
    async installNew() { await install(afterDmg); }, // over the RUNNING older build: no quit first
    async openApp() { await exec(`open -a ${sq(bundle)}`); }, // without -n: it only focuses the running (old) app
    async agentBuild16() { return build16((await exec(`cat ${data}/agent.json`)).stdout); },
    async agentProcs() { return agentProcs(exec, await guestNowMs(exec)); },
    /** App chip + log line, helper card (Update available) and "Update the helper" with one prompt, then Quit and reopen leaves nothing from before the install. */
    async extraChecks({ boundaryMs }) {
      const out = [];
      await gui.ensureMainWindow().catch(() => {});
      const chip = await gui.waitFor(async () => (await gui.texts()).some((t) => /Dyno was updated\. Restart Dyno to finish/.test(t.text)), 60_000);
      out.push(chip.ok ? chk(true, 'the chip "Dyno was updated. Restart Dyno to finish." is shown (the old app still runs)') : chk(false, 'the chip "Dyno was updated. Restart Dyno to finish." is not shown 60 s after the install'));
      const log = (await exec(`cat "$HOME/Library/Logs/Dyno/"dyno*.log 2>/dev/null | grep -c "program files changed while it ran"`)).stdout.trim().split('\n').at(-1);
      out.push(chk(Number(log) > 0, Number(log) > 0 ? `the log line "Dyno's program files changed while it ran" is there (${log} time(s))` : 'the log line "Dyno\'s program files changed while it ran" is missing'));
      // The helper: another build of the same version reads "Update available"; "Update the helper" asks for the password once; then the installed helper is the bundled one.
      const nav = await gui.openSettingsSection('^Dyno$');
      const outdated = await gui.waitFor(async () => (await gui.texts()).some((t) => /Update available|Outdated|another build/i.test(t.text)), 30_000);
      out.push(outdated.ok ? chk(true, 'the helper card reads "Update available" (the helper is another build)') : chk(false, `the helper card does not read "Update available" (${nav.ok ? 'Settings opened' : nav.detail})`));
      if (outdated.ok) {
        const pressed = await gui.press(gui.app, '^Update the helper$');
        out.push(chk(pressed, pressed ? '"Update the helper" pressed' : 'no "Update the helper" button on the card'));
        if (pressed) {
          const sheet = await gui.answerPasswordSheet();
          const second = await gui.waitFor(async () => (await gui.has('coreautha', '^(OK|Modify Settings|Install Helper)$')) || (await gui.has('SecurityAgent', '^(OK|Modify Settings|Install Helper)$')), 6_000);
          out.push(chk(sheet.ok && !second.ok, sheet.ok && !second.ok ? 'one password prompt, answered' : `the password prompt: ${sheet.detail}${second.ok ? '; a second prompt appeared' : ''}`));
          const same = await gui.waitFor(async () => { const r = (await exec(`a=$(shasum -a 256 ${sq(helperFile)} 2>/dev/null | cut -c1-64); b=$(shasum -a 256 ${sq(bin('dyno-helper'))} | cut -c1-64); [ -n "$a" ] && [ "$a" = "$b" ] && echo same`)).stdout; return /same/.test(r); }, 90_000);
          out.push(chk(same.ok, same.ok ? 'the installed helper file is the bundled dyno-helper (same SHA-256)' : 'the installed helper file differs from the bundled dyno-helper 90 s after "Update the helper"'));
          const card = await gui.waitFor(async () => (await gui.texts()).some((t) => /^On$/.test(t.text)), 60_000);
          out.push(chk(card.ok, card.ok ? 'the helper card reads "On" after the update' : 'the helper card does not read "On" after the update'));
        }
      }
      await gui.closeSettings().catch(() => {});
      // Quit and reopen: nothing from before the install may remain (the helper was just replaced).
      await exec(`osascript -e 'quit app "Dyno"' >/dev/null 2>&1; sleep 5; true`);
      await exec(`open -a ${sq(bundle)}`);
      for (let i = 0; i < 30; i++) { if ((await exec(`${bin('dyno-agent')} status >/dev/null 2>&1`)).code === 0) break; await guest.sleep(4000); }
      await guest.sleep(5000);
      const now = await guestNowMs(exec);
      const old = [...(await agentProcs(exec, now)), ...(await helperProcs(now))].filter((x) => x.startMs < boundaryMs);
      const appOld = (await exec(`ps -axo pid=,etime=,command= | grep '${bundle}/Contents/MacOS/dyno-app' | grep -v grep`)).stdout.split('\n').map((l) => l.trim()).filter(Boolean).map((l) => { const [pid, etime] = l.split(/\s+/); return { pid: Number(pid), startMs: now - etimeSeconds(etime) * 1000 }; }).filter((x) => x.startMs < boundaryMs);
      const all = [...old, ...appOld];
      out.push(chk(all.length === 0, all.length ? `after Quit and reopen ${all.length} process(es) from before the install remain: ${all.map((x) => x.pid).join(', ')}` : 'after Quit and reopen no process from before the install remains'));
      return out;
    },
  };
}

/** Linux: `oldDeb` / `newDeb` are guest paths of the .deb files. */
export function linuxSwapPrimitives({ exec, sleep, now, oldDeb, newDeb, startApp }) {
  const data = '"$HOME/.local/share/dyno"';
  const dpkg = async (flags, deb) => {
    // The exit code of dpkg itself (not of a pipe after it) and its whole output: a failed install must never read as a swap that did not happen.
    const r = await exec(`sudo DEBIAN_FRONTEND=noninteractive dpkg -i ${flags} ${sq(deb)} > /tmp/dpkg-swap.out 2>&1; rc=$?; echo "rc=$rc"; tail -n 25 /tmp/dpkg-swap.out`);
    const rc = /rc=(\d+)/.exec(r.stdout)?.[1];
    if (rc !== '0') throw new Error(`dpkg -i ${deb} failed (rc=${rc ?? 'unknown'}): ${r.stdout.replace(/rc=\d+\n?/, '').trim().slice(-600)}`);
    return r.stdout;
  };
  let installLog = '';
  return {
    now, sleep,
    async installOld() { await dpkg('--force-downgrade', oldDeb); },
    async startOldWithHelper() {
      await startApp();
      await exec('sudo /usr/bin/dyno-helper install --for uid:1000 >/dev/null 2>&1; true');
      let active = false;
      for (let i = 0; i < 20 && !active; i++) { active = (await exec('systemctl is-active dyno-helper.service')).stdout.trim() === 'active'; if (!active) await sleep(3000); }
      return { oldVersion: versionOf((await exec(`dpkg-query -W -f='\${Version}\\n' dyno`)).stdout), helperWasOn: active };
    },
    async oldAgentBuild16() { return build16((await exec(`cat ${data}/agent.json`)).stdout); },
    async expectedAgentBuild16() { const h = (await exec(`dpkg-deb --fsys-tarfile ${sq(newDeb)} | tar -xO ./usr/bin/dyno-agent | sha256sum | cut -c1-16`)).stdout.trim().split('\n').at(-1); return /^[0-9a-f]{16}$/.test(h) ? h : null; },
    async beginInstall() { return guestNowMs(exec); },
    async installNew() {
      installLog = await dpkg('', newDeb);
      const want = (await exec(`dpkg-deb --fsys-tarfile ${sq(newDeb)} | tar -xO ./usr/bin/dyno-agent | sha256sum | cut -c1-16`)).stdout.trim().split('\n').at(-1);
      const disk = (await exec('sha256sum /usr/bin/dyno-agent | cut -c1-16')).stdout.trim();
      if (want !== disk) throw new Error(`dpkg -i reported success but /usr/bin/dyno-agent (${disk}) is not the new .deb's (${want}); dpkg output: ${installLog.replace(/\n/g, ' | ').slice(-500)}`);
    },
    async openApp() { await startApp(); },
    async agentBuild16() { return build16((await exec(`cat ${data}/agent.json`)).stdout); },
    async agentProcs() { return agentProcs(exec, await guestNowMs(exec)); },
    async extraChecks({ boundaryMs }) {
      const nowG = await guestNowMs(exec);
      const hp = (await exec(`ps -eo pid=,etimes=,args= | grep -E 'dyno-helper serve' | grep -v grep`)).stdout.split('\n').map((l) => l.trim()).filter(Boolean).map((l) => { const [pid, et] = l.split(/\s+/); return { pid: Number(pid), startMs: nowG - Number(et) * 1000 }; });
      const active = (await exec('systemctl is-active dyno-helper.service')).stdout.trim();
      return [chk(active === 'active', `dyno-helper.service is ${active}`), chk(hp.length > 0 && hp.every((x) => x.startMs >= boundaryMs - 5000), hp.length ? `helper pid(s) ${hp.map((x) => x.pid).join(', ')} ${hp.every((x) => x.startMs >= boundaryMs - 5000) ? 'started after the install began' : 'started BEFORE the install began (not restarted)'}` : 'no helper process is running after the install'), chk(true, `dpkg output: ${installLog.replace(/\n/g, ' | ').slice(-240)}`)];
    },
  };
}
const versionOf = (t) => /\b(\d+\.\d+\.\d+)\b/.exec(String(t ?? ''))?.[1] ?? null;
