// `run-win.mjs --only <rows>`: run only the named release rows of the Windows guest (whole rows, by name or by their number in the 1.1.0 table), after
// one install. Pure: it maps the names to the phases and the manifest ids a partial run must find; run-win.mjs and win-ui.mjs do the work.
// A token that is not a row name is a part of win-suite.ps1's own -Only (e.g. `start`) and is passed through when no row is named.
export const WIN_ROWS = {
  'tray-menu': { aliases: ['10', '11', 'tray'], phase: 'ui', manifest: ['tray-menu', 'tray-countdown'] },
  'login-jobs': { aliases: ['20', '5a'], phase: 'ui', manifest: ['login-jobs'] },
  'jobs-restart': { aliases: ['jobs-agent'], phase: 'ui', manifest: ['jobs-restart'] },
  mcp: { aliases: ['35', '36'], phase: 'ui', manifest: ['mcp'] },
  'disk-space': { aliases: ['21'], phase: 'ui', manifest: ['disk-space'] },
  exploit: { aliases: ['16'], phase: 'ui', needs: ['helper'], manifest: ['exploit'] },
  scoping: { aliases: ['29', 'd6'], phase: 'ui', needs: ['helper'], manifest: ['scoping'] },
  'hung-page': { aliases: ['42'], phase: 'after', manifest: ['hung-page'] },
  'install-rows': { aliases: ['38', '39', 'kill-installer', 'install-again', 'reinstall'], phase: 'after', manifest: ['win-reinstall', 'kill-installer', 'install-again'] },
  'upgrade-swap': { aliases: ['40', 'swap'], phase: 'after', needs: ['old-installer'], manifest: ['upgrade-swap'] },
  'second-user': { aliases: ['41'], phase: 'after', manifest: ['second-user'] },
  'restart-tray': { aliases: [], phase: 'reboot', manifest: ['restart-tray'] },
};

const byToken = new Map(Object.entries(WIN_ROWS).flatMap(([k, r]) => [[k, k], ...r.aliases.map((a) => [a, k])]));

/**
 * @param {string|undefined} only the --only value: comma-separated row names or numbers, or a win-suite.ps1 part.
 * @returns {{rows: string[], suitePart: string|null, startPart: string, ui: string[], after: string[], reboot: boolean, helper: boolean, oldInstaller: boolean, manifest: string[]}}
 */
export function selectRows(only) {
  const tokens = String(only ?? '').split(',').map((t) => t.trim().toLowerCase()).filter(Boolean);
  const rows = [...new Set(tokens.filter((t) => byToken.has(t)).map((t) => byToken.get(t)))];
  const others = tokens.filter((t) => !byToken.has(t));
  if (rows.length && others.length) throw new Error(`--only mixes rows with ${others.join(', ')}: name rows (${Object.keys(WIN_ROWS).join(', ')}) or one win-suite.ps1 part, not both`);
  const pick = (phase) => Object.keys(WIN_ROWS).filter((k) => rows.includes(k) && WIN_ROWS[k].phase === phase); // the table's order is the run order
  const needs = (n) => rows.some((k) => WIN_ROWS[k].needs?.includes(n));
  if (pick('reboot').length && rows.length > 1) throw new Error('restart-tray runs its own restart phase: name it alone');
  return {
    rows, suitePart: rows.length ? null : tokens[0] ?? null, startPart: rows.includes('tray-menu') ? 'start-tray' : 'start', ui: pick('ui'), after: pick('after'), reboot: pick('reboot').length > 0,
    helper: needs('helper'), oldInstaller: needs('old-installer'), manifest: Object.keys(WIN_ROWS).filter((k) => rows.includes(k)).flatMap((k) => WIN_ROWS[k].manifest),
  };
}
