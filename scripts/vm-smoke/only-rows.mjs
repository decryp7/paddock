// `run-linux.mjs --only <row numbers>`: the numbers are the rows of the Linux report table of a full `--reboot-check` run (so "row 7" in a report
// and `--only 7` are the same thing). A partial run drives the chosen rows plus the setup they stand on, requires only the chosen rows, and is not a
// release verdict. Preconditions are set and checked first (ER-33): a broken one stops the run at once as "check broken: <precondition>".
import { READINGS } from './helper-readings.mjs';

const readings = READINGS.filter((r) => r.os.includes('linux')).map((r) => `reading:${r.id}`);
/** Row number -> the manifest ids the row stands for (manifest.mjs). */
export const LINUX_ROWS = {
  1: ['install'], 2: ['build-binding'], 3: ['cold-start'], 4: ['keep-awake'], 5: ['helper-on'], 6: ['login-switch'],
  7: ['reboot-before'], 8: ['reboot-after'], 9: ['reboot-tray'], 10: ['reboot-card'],
  ...Object.fromEntries(readings.map((id, i) => [11 + i, [id]])),
  17: ['mcp'], 18: ['helper-off'], 19: ['login-jobs'], 20: ['disk-space'], 21: ['discovery'], 22: ['quit'], 23: ['quit-stop'],
  24: ['hung-page'], 25: ['linux-log-dir'], 26: ['upgrade-swap'],
};
const LAST = 26;

/** "23,7,10" -> [7, 10, 23]. */
export function parseOnly(text) {
  const parts = String(text ?? '').split(',').map((s) => s.trim());
  if (parts.some((p) => !/^\d+$/.test(p))) throw new Error(`--only takes row numbers separated by commas (for example 7,10,19,23), got "${text}"`);
  const rows = [...new Set(parts.map(Number))].sort((a, b) => a - b);
  const bad = rows.filter((n) => n < 1 || n > LAST);
  if (bad.length) throw new Error(`--only: no such row ${bad.join(', ')} (rows are 1 to ${LAST})`);
  return rows;
}

/** The manifest rows a chosen set of row numbers requires. */
export function onlyRequired(required, rows) {
  const ids = new Set(rows.flatMap((n) => LINUX_ROWS[n] ?? []));
  return required.filter((r) => ids.has(r.id));
}

/** The row number a step title belongs to, by the manifest's own patterns; null when none matches. */
export function stepRow(title, required) {
  const hit = required.find((r) => r.match.test(title));
  if (!hit) return null;
  const n = Object.entries(LINUX_ROWS).find(([, ids]) => ids.includes(hit.id));
  return n ? Number(n[0]) : null;
}

const PRE = [
  { key: 'installed', name: 'the .deb is installed', rows: null },
  { key: 'commitMatches', name: 'the installed build is the expected commit', rows: null },
  { key: 'helperActive', name: 'the administrator helper is active (dyno-helper.service)', rows: [7, 8, 9, 10] },
  { key: 'autostartEntry', name: 'Launch at login is on: ~/.config/autostart/Dyno.desktop exists', rows: [8, 9, 10], unless: 7 },
  { key: 'windowReadable', name: 'the Dyno window is readable over AT-SPI', rows: [7, 10, 19, 23] },
];
/** The first precondition that does not hold for the chosen rows (null: all hold). `rows: null` is demanded by every row. */
export function firstBrokenPrecondition(facts, rows) {
  for (const p of PRE) if ((p.rows === null || rows.some((n) => p.rows.includes(n))) && !(p.unless && rows.includes(p.unless)) && !facts[p.key]) return p.name;
  return null;
}

/** True when the Jobs list evidence (the lines before the Dyno log section) shows the heading "The jobs could not be read". */
export function staleJobsReadError(evidence) {
  const cut = evidence.findIndex((l) => /^--- Dyno log lines/.test(l));
  return (cut < 0 ? evidence : evidence.slice(0, cut)).some((l) => /jobs could not be read/i.test(l));
}
