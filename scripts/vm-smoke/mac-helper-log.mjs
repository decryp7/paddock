// The macOS helper writes its start and its end to the unified log (family ADR 0054 amendment 1, `paddock_service::service_main`): "<name> is starting",
// then "<name> stopped cleanly after N ms" or the failure text with "exit code N". Read with `log show` after Turn on and Turn off.

/** Judges the `log show` text of the helper: the start line and an end line with its duration or exit code. */
export function judgeHelperLog(text) {
  const lines = text.split('\n').filter((l) => /DynoHelper|dyno-helper/i.test(l));
  const starting = lines.filter((l) => /DynoHelper is starting/.test(l));
  const clean = lines.filter((l) => /DynoHelper stopped cleanly after \d+ ms/.test(l));
  const failed = lines.filter((l) => /exit code \d+/.test(l));
  return {
    starting: starting.length,
    cleanStops: clean.length,
    exitCodeLines: failed.length,
    checks: [
      { ok: starting.length > 0, detail: starting.length ? `"DynoHelper is starting" logged ${starting.length} time(s)` : 'no "DynoHelper is starting" line in the unified log' },
      { ok: clean.length + failed.length > 0, detail: clean.length ? `"stopped cleanly after N ms" logged (${clean.length})` : failed.length ? `an exit-code line is logged (${failed[failed.length - 1].slice(-90)})` : 'no end line ("stopped cleanly after N ms" or an exit code) in the unified log' },
    ],
    evidence: lines.slice(-12),
  };
}

/** The step: reads the unified log of the last 30 minutes for the helper's program. */
export async function macHelperLogStep(guest) {
  // Only the helper's own lines (`eventMessage CONTAINS "DynoHelper"`): a predicate on the whole process plus `tail -80` lost the start line, because the helper's
  // CFOpenDirectory noise every 5 s pushed it out of the last 80 lines (2026-11-05: "stopped cleanly after 776012 ms" was kept, its start 13 minutes earlier was cut).
  const r = await guest.exec(`log show --last 30m --info --style compact --predicate 'eventMessage CONTAINS "DynoHelper"' 2>&1 | grep -E "DynoHelper|exit code" | tail -40`);
  const j = judgeHelperLog(r.stdout ?? '');
  const bad = j.checks.filter((c) => !c.ok);
  return { title: 'macOS helper lines in the unified log (log show)', status: bad.length ? 'fail' : 'pass', summary: bad.length ? bad.map((c) => c.detail).join('; ').slice(0, 240) : 'the start line and the end line (duration or exit code) are in the unified log', checks: j.checks, evidence: j.evidence };
}
