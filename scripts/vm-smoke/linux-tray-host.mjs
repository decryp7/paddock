// The Linux tray-host checks (feat/dyno-linux-tray-availability) on the GNOME base: Dyno's tray icons need an AppIndicator host. Three states:
// 1. extension ON: icons are shown, Settings > Menu bar has no "isn't showing tray icons" note, the log has `tray host:` lines;
// 2. extension OFF: the note appears in Settings > Menu bar and as the status-bar health line, and a `--hidden` start opens the window;
// 3. extension turned back ON while Dyno runs: the icons appear without restarting Dyno (same pid).
// `h` = { g, ses, atspi, press, sleep, log }: guest commands as root-ish (g) and in the user's session (ses), AT-SPI helper and a press(name, role) helper.
import { makeResult } from './verdict.mjs';
import { judgeTrayNote } from './linux-judges.mjs';
const result = makeResult;
const NOTE = /isn.t showing tray icons/i;
// The status bar's health chip names the stopped dependency, its cause is in the chip's popover: "Tray icons: stopped".
const HEALTH = /Tray icons: (stopped|not shown)/i; // 7538 worded the chip "stopped", 7550 says "not shown"
// An indicator of Dyno in the panel is a menu of its own ("menu dyno-app", "menu tray-icon ..."); the top bar also shows the focused application's name as "label Dyno" and a "push button Dyno" around it.
const INDICATOR = /menu\s+(dyno-app|tray-icon)/i;

export async function trayHostChecks(h) {
  const { g, ses, atspi, press, sleep } = h;
  const steps = [];
  const ext = ses('gnome-extensions list 2>/dev/null | grep -i appindicator | head -1').stdout.trim();
  if (!ext) return [result('Linux tray host: the AppIndicator extension is not installed in the base', [{ ok: false, detail: 'no appindicator extension found by gnome-extensions list (run the base maintenance first)' }])];
  const topBar = () => atspi('tree gnome-shell 16 2>&1 | grep -i -E "dyno" | head -8').stdout.trim();
  const trayLog = () => g('grep -rhi "tray host:" ~/.local/share/dyno ~/.cache/dyno ~/.local/state/dyno 2>/dev/null | tail -6').stdout.trim();
  const pid = () => g('pgrep -x dyno-app | head -1').stdout.trim();
  const settingsText = async () => { press('^File$', 'menu item'); await sleep(1000); press('^Settings…$', 'menu item'); await sleep(3500); /* Settings opens on its first page; the Menu bar card is on the Dyno page */ press('^Dyno$', 'push button'); await sleep(2500); /* the Dyno page holds the Menu bar card far down and WebKit keeps only what is near the viewport in the accessibility tree: scroll the Settings window to it */ ses('xdotool search --onlyvisible --name "^Settings" | head -1 | xargs -I{} sh -c "xdotool windowactivate {} && xdotool key --repeat 30 Next"'); await sleep(2500); return atspi('tree dyno 90 text').stdout; };
  const closeSettings = () => press('^Close$');
  const start = async (args = '') => { ses(`systemd-run --user --collect --unit=dyno-tray${Date.now() % 1000} -E DISPLAY=:0 -E XAUTHORITY=$XAUTHORITY /usr/bin/dyno-app ${args} >/dev/null 2>&1`); for (let i = 0; i < 30 && ses('dyno status >/dev/null 2>&1; echo $?').stdout.trim() !== '0'; i++) await sleep(4000); await sleep(8000); };
  const stop = async () => { g('pkill -x dyno-app; pkill -x dyno-agent; true'); await sleep(5000); };

  // 1. ON
  ses(`gnome-extensions enable ${ext}`); await sleep(4000);
  await stop(); await start();
  let bar = topBar(), text = await settingsText(), log = trayLog();
  steps.push(result('Linux tray host: AppIndicator extension ON', [
    { ok: INDICATOR.test(bar), detail: INDICATOR.test(bar) ? 'Dyno indicators are in the GNOME top bar' : 'no Dyno indicator in the GNOME top bar with the extension on' },
    ...judgeTrayNote(text, false), // absence counts only when the reader saw the Menu bar card
    { ok: /tray host:/i.test(log), detail: /tray host:/i.test(log) ? 'the log has "tray host:" lines' : 'no "tray host:" line in the log' },
  ], [`extension: ${ext}`, '--- top bar', bar, '--- log', log], 'icons shown, no note, tray host logged'));
  closeSettings();

  // 2. OFF
  ses(`gnome-extensions disable ${ext}`); await sleep(6000);
  await stop(); await start('--hidden');
  const win = atspi('tree dyno 6 2>&1 | grep -i -E "frame" | head -2').stdout.trim();
  text = await settingsText(); bar = topBar();
  const health = atspi('tree dyno 80 2>&1 | grep -i -E "tray icons|menu bar" | head -4').stdout.trim();
  steps.push(result('Linux tray host: AppIndicator extension OFF', [
    ...judgeTrayNote(text, true),
    { ok: HEALTH.test(health) || NOTE.test(health), detail: HEALTH.test(health) || NOTE.test(health) ? 'the status-bar health chip says the tray icons stopped' : 'the status-bar health chip "Tray icons: stopped" was not found' },
    { ok: /frame/i.test(win), detail: /frame/i.test(win) ? 'a --hidden start opened the window (there is no tray to find it in)' : 'a --hidden start left Dyno with no window and no tray' },
    { ok: !INDICATOR.test(bar), detail: INDICATOR.test(bar) ? 'a Dyno indicator is in the top bar although the extension is off' : 'no Dyno indicator in the top bar with the extension off' },
  ], ['--- window', win, '--- health line', health, '--- top bar', bar, '--- Settings > Menu bar note', (text.split('\n').find((l) => NOTE.test(l)) ?? 'none').trim()], 'note shown, window opened'));
  closeSettings();

  // 3. back ON while Dyno runs
  const before = pid();
  ses(`gnome-extensions enable ${ext}`);
  let appeared = false;
  for (let i = 0; i < 15 && !appeared; i++) { await sleep(3000); appeared = INDICATOR.test(topBar()); }
  const after = pid();
  steps.push(result('Linux tray host: extension turned back ON while Dyno runs', [
    { ok: appeared, detail: appeared ? 'the icons appeared in the top bar without restarting Dyno' : 'no Dyno indicator 45 s after the extension was enabled again' },
    { ok: before !== '' && before === after, detail: `Dyno pid ${before} before, ${after} after (must be the same: no restart)` },
  ], [topBar()], 'icons appeared by themselves'));
  return steps;
}
