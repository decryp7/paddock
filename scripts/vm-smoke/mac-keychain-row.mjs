// The guest side of the keychain other-build measurement (release-rows.mjs keychainOtherBuildStep). Build A makes the MCP key itself, through the app's own UI
// (Settings > AI access (MCP) > switch on > "Add a client…" > Client name > "Create key", found by driving a clone on 2026-11-05), so the credential-store item
// carries build A's signing identity; build B is then installed over it. A window is "visible" when the dashboard's sidebar link exists (the page loaded); the keychain
// sheet is a SecurityAgent window. Guest only (a throw-away clone): it deletes the `Dyno` / `mcp:keys` item in the guest's login keychain, never the host's.
const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
const ITEM = 'security find-generic-password -s Dyno -a mcp:keys >/dev/null 2>&1';

/** @param o { guest, gui (MacGui), check (Check, for install()), share (guest path of the staged files), bundle, beforeDmg, afterDmg, log } */
export function macKeychainPrimitives({ guest, gui, check, share, bundle, beforeDmg = 'before.dmg', afterDmg = 'installer.dmg', log = () => {} }) {
  const exec = (c, o) => guest.exec(c, o);
  const quit = async () => { await exec(`osascript -e 'quit app "Dyno"' >/dev/null 2>&1; sleep 2; pkill -x dyno-app; pkill -x dyno-agent; pkill SecurityAgent; sleep 2; true`); };
  const install = async (name) => {
    const s = await check.install(`${share}/${name}`);
    if (s.status !== 'pass') throw new Error(`installing ${name} failed: ${s.summary}`);
  };
  const windowVisible = () => gui.has(gui.app, '^Dashboard$', 'AXLink');
  const waitWindow = async (ms, what) => { if (!(await gui.waitFor(windowVisible, ms)).ok) throw new Error(`${what}: no window within ${ms / 1000} s`); };
  return {
    now: () => guest.now(),
    sleep: (ms) => guest.sleep(ms),
    windowVisible,
    async sheetVisible() { return Number((await exec('axdrive windows | cut -f1 | grep -c SecurityAgent; true')).stdout.trim()) > 0; },
    async createKeyInBuildA() {
      await quit();
      await exec('security delete-generic-password -s Dyno -a mcp:keys >/dev/null 2>&1; true');
      await install(beforeDmg);
      await exec(`open -a ${sq(bundle)}`);
      await waitWindow(90_000, 'build A with no key');
      await gui.activate();
      const nav = await gui.openSettingsSection('^AI access');
      if (!nav.ok) throw new Error(nav.detail);
      if (await gui.has(gui.app, '^AI access is off', 'AXCheckBox')) await gui.press(gui.app, '^AI access is off', 'AXCheckBox');
      if (!(await gui.waitFor(() => gui.press(gui.app, '^Add a client', 'AXButton'), 15_000)).ok) throw new Error('no "Add a client…" button in Settings > AI access (MCP)');
      if (!(await gui.waitFor(async () => (await gui.fieldIndex(/^Client name/)) >= 0, 15_000)).ok) throw new Error('the Add a client form did not open');
      if (!(await gui.setField(/^Client name/, 'vm-keychain-check'))) throw new Error('could not type the client name');
      if (!(await gui.press(gui.app, '^Create key$', 'AXButton'))) throw new Error('no "Create key" button');
      if (!(await gui.waitFor(async () => (await exec(ITEM)).code === 0, 30_000)).ok) throw new Error('build A created no Dyno / mcp:keys item in the login keychain');
      log('keychain row: build A made its own key');
    },
    async reopenSameBuild() { await quit(); await exec(`open -a ${sq(bundle)}`); },
    async installBuildB() { await quit(); await install(afterDmg); },
    async openApp() { await exec(`open -a ${sq(bundle)}`); },
    /** Leaves the guest as it found it: no app running, no key item, the sheet gone. */
    async cleanup() { await quit(); await exec('security delete-generic-password -s Dyno -a mcp:keys >/dev/null 2>&1; true'); },
  };
}
