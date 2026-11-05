// The required steps of every VM run, per OS and suite (as data). A run is PASS only when each of them ran and passed (verdict.mjs), so a flag left
// out (--ui, --reboot-check) or a suite that was dropped from a chain turns into "INCOMPLETE: missing ..." instead of a shorter PASS.
// Adding a step to a suite adds a row here; coverage cannot shrink silently. `release` (default) requires the full set whatever the flags say;
// `partial` requires only what the flags ask for and the result says it is not a release verdict.
import { READINGS } from './helper-readings.mjs';

const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const row = (id, re) => ({ id, match: re });
const readings = (os) => READINGS.filter((r) => r.os.includes(os)).map((r) => row(`reading:${r.id}`, new RegExp(`^Helper reading \\(${os}\\): ${esc(r.title)}$`)));
const BUILD = row('build-binding', /^Build binding:/);
// Rule 71 rows (mcp-judges.mjs): required on the final candidate of every OS; a runner that does not supply them leaves the run INCOMPLETE.
const RULE71 = [row('mcp', /^Rule 71: Dyno MCP from outside/), row('quit-stop', /^Rule 71: Quit and stop measuring/)];

const MAC_START = [
  row('install', /^Install$/), BUILD,
  row('cold-start', /^Cold start \(disk cache dropped\)/), row('second-launch', /^Second launch while running keeps one instance/),
  row('quit-window', /^Quit with a window open/), row('start-hidden', /^Start without a window/), row('quit-hidden', /^Quit with no window/),
  row('relaunch', /^Relaunch after Quit/), row('quit-relaunch', /^Quit after the relaunch/),
  row('login-handover', /^Launch at login on, off, on/), row('update-host-blackholed', /^Start with the update host black-holed/),
];
const MAC_POWER = [
  row('install', /^Install$/), BUILD, row('cold-start', /^Cold start for the power checks/), row('helper-on', /^Admin helper: Turn on/),
  row('keep-awake', /^Keep awake: held by the OS/), row('scheduled-wake', /^Scheduled wake: listed by the OS/), row('wake-on-lan', /^Wake-on-LAN: magic packet/),
  row('tour-menu-bar', /^GUI: menu-bar items/), row('tour-dashboard', /^GUI: dashboard customize/), row('tour-benchmark', /^GUI: short benchmark/),
  row('idle-action', /^Idle action: countdown/), row('helper-off', /^Admin helper: Turn off/), row('helper-log', /^macOS helper lines in the unified log/),
  row('quit', /^Quit after the power checks/),
];
const MAC_HELPER = [
  row('install', /^Install$/), BUILD, row('cold-start', /^Cold start for the power checks/), row('helper-on', /^Admin helper: Turn on/),
  row('tour-menu-bar', /^GUI: menu-bar items/), row('helper-off', /^Admin helper: Turn off/), row('helper-log', /^macOS helper lines in the unified log/),
  row('quit', /^Quit after the power checks/),
];
const LINUX = [
  row('install', /^Install \(\.deb/), BUILD, row('cold-start', /^Cold start, one instance/), row('keep-awake', /^Keep awake: held by the OS/),
  row('helper-on', /^Admin helper Turn on \(polkit\)/), ...readings('linux'), row('helper-off', /^Admin helper Turn off removes it/), row('quit', /^Quit \(File > Quit\)/),
];
const LINUX_REBOOT = [
  row('reboot-before', /^Restart check: before the reboot/), row('reboot-after', /^Restart check: Dyno after the reboot/),
  row('reboot-tray', /^Restart check: tray icons on GNOME/), row('reboot-card', /^Restart check: card and a helper reading/),
];
const LINUX_TRAY = [row('tray-host-on', /^Linux tray host: AppIndicator extension ON/), row('tray-host-off', /^Linux tray host: AppIndicator extension OFF/), row('tray-host-back-on', /^Linux tray host: extension turned back ON/)];
const WIN_BASE = [row('install', /^Install \(per-machine/), BUILD, row('cold-start', /^Cold start, one instance/), row('second-launch', /^Second launch while running/)];
const WIN_FULL = [
  row('keep-awake', /^Keep awake: held by the OS/), row('scheduled-wake', /^Scheduled wake:/), row('idle-action', /^Idle action:/), row('quit', /^Quit: nothing of the app is left/),
];
const WIN_UI = [
  row('snap', /^Windows: Aero snap/), row('dom-tour', /^Windows: shared screenshot check on every page/), row('tray-menu', /^Windows: tray right-click menu/),
  row('tray-countdown', /^Windows: tray menu during an idle countdown/),
  row('helper-on', /^Windows: admin helper Turn on with a planted junction/), row('tray-tooltips', /^Windows: tray icons and their live tooltips/),
  row('wp-sec', /^Windows: WP-SEC folder protection/), row('helper-acl', /^Windows: helper data folder is not writable/), row('exploit', /^Windows: exploit probe/),
  row('repeat-install', /^Windows: repeat install of the disk health tool/), row('disk-health', /^Windows: disk health install/), ...readings('windows'),
  row('scoping', /^Windows: D6 scoping/), row('served', /^Windows: the helper reports which readings it served/),
  row('repair', /^Windows: Repair through the card/), row('helper-off', /^Windows: admin helper Turn off/),
  row('quit-exit', /^Windows: Quit \(File > Exit\)/), row('same-version-refused', /^Windows: an update from the same version is refused/), row('uninstall', /^Windows: uninstall removes/),
];
const WIN_REBOOT = [
  row('helper-on', /^Windows: admin helper Turn on/), row('restart-before', /^Windows restart: helper, Launch at login and tray before the restart/),
  row('restart-quit', /^Windows restart: the helper after Dyno is quit/), row('restart-autostart', /^Windows restart: auto start with Launch at login (ON|switched ON)/),
  row('restart-tray', /^Windows restart: tray icons after the restart/), row('restart-helper', /^Windows restart: the helper works after a real restart/),
];

/**
 * The required steps of one run.
 * @param {{os:'windows'|'linux'|'macos', suite?:'start'|'power'|'helper', ui?:boolean, reboot?:boolean, trayHost?:boolean, release?:boolean, partial?:boolean}} o
 */
export function requiredSteps({ os, suite = 'start', ui = false, reboot = false, trayHost = false, release = true, partial = false }) {
  const full = release && !partial; // a release run needs every part, whether or not the flag was given
  if (os === 'windows') {
    const rebootRun = reboot && !ui;
    if (rebootRun) return [...WIN_BASE, ...WIN_REBOOT];
    return [...WIN_BASE, ...WIN_FULL, ...(ui || full ? [...WIN_UI, ...RULE71] : [])];
  }
  if (os === 'linux') return [...LINUX, ...RULE71, ...(reboot || full ? LINUX_REBOOT : []), ...(trayHost ? LINUX_TRAY : [])];
  if (os === 'macos') {
    const base = suite === 'power' ? MAC_POWER : suite === 'helper' ? MAC_HELPER : MAC_START;
    if (suite === 'start') return base;
    return [...base, ...(suite === 'power' ? RULE71 : []), ...(reboot ? [row('restart-menu-bar', /^Restart check: menu-bar icons after the restart/)] : readings('macos'))];
  }
  throw new Error(`no manifest for ${os}`);
}
