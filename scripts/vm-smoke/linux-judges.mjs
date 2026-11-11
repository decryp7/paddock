// Pure judges of the Linux suite (run-linux.mjs, linux-tray-host.mjs): what the AT-SPI reader printed in, checks out. A check with `kind: 'check'` says the
// reader or the control failed, which is not a statement about the product (verdict.mjs).
const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });

/** `atspi.py state` output ("checked: yes|no", "not found") to 'on' | 'off' | 'unknown'. */
export function parseSwitchState(text) {
  const m = /^checked: (yes|no)\s*$/m.exec(String(text ?? ''));
  return m ? (m[1] === 'yes' ? 'on' : 'off') : 'unknown';
}

/**
 * "Start Dyno at login" is ON by default since Dyno 1.1.0: pressing it blind turns it OFF. The state is read first, pressed only when OFF, and
 * read again; an unreadable switch is a broken check.
 * @param {{before:'on'|'off'|'unknown', after:'on'|'off'|'unknown', pressed:boolean}} f
 */
export function judgeLoginSwitch({ before, after, pressed }) {
  if (before === 'unknown') return [chk(false, 'the "Start Dyno at login" switch could not be found or its state read, so it was not touched', 'check')];
  if (before === 'on') return [chk(after === 'on' && !pressed, after === 'on' && !pressed ? '"Start Dyno at login" was already ON (the default) and was left alone' : `"Start Dyno at login" was ON but ended ${after}${pressed ? ' after being pressed' : ''}`)];
  return [chk(pressed && after === 'on', pressed && after === 'on' ? '"Start Dyno at login" was OFF; pressed once, it is now ON' : `"Start Dyno at login" was OFF and is ${after} after ${pressed ? 'the press' : 'no press'}`)];
}

/**
 * The administrator-readings card after the reboot: ON means the card offers "Turn off administrator readings". An empty read (the window was not
 * up) is a broken check, not an "on"; "Turn on administrator readings" is the helper being off.
 */
export function judgeCard(cardText) {
  const t = String(cardText ?? '').trim();
  if (!t) return chk(false, 'the card could not be read (the Dyno window was not up or the reader saw nothing)', 'check');
  if (/Turn off administrator readings/.test(t)) return chk(true, `card is ON: ${t.split('\n').slice(0, 3).join(' | ')}`);
  if (/Turn on administrator readings/.test(t)) return chk(false, `the card offers "Turn on administrator readings": the helper is not on after the reboot (${t.split('\n')[0]})`);
  return chk(false, `the card was read but shows neither Turn on nor Turn off (${t.split('\n')[0].slice(0, 80)})`, 'check');
}

const CARD = /Menu bar/i;
const NOTE = /isn.t showing tray icons/i;

/**
 * The "no tray icons" note of Settings > Menu bar. Absence means something only when the reader could see the card: the text must contain the card
 * ("Menu bar"), else the reader looked at the wrong page and the check is broken.
 * @param {string} text the Settings text
 * @param {boolean} expectNote the host is missing (extension off): the note must be there; with a host it must be absent
 */
export function judgeTrayNote(text, expectNote) {
  const sees = CARD.test(text) || NOTE.test(text);
  if (expectNote) return [chk(NOTE.test(text), NOTE.test(text) ? 'Settings > Menu bar shows the "Your desktop isn\'t showing tray icons" note' : 'no note in Settings > Menu bar with the extension off', NOTE.test(text) || sees ? undefined : 'check')];
  if (!sees) return [chk(false, 'the reader could not see the Settings > Menu bar card, so the absence of the note proves nothing', 'check')];
  return [chk(!NOTE.test(text), NOTE.test(text) ? 'Settings > Menu bar shows the "no tray icons" note although a host is present' : 'the Menu bar card is visible and has no "no tray icons" note')];
}
