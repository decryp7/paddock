// The exit status of a conformance run (ADR 0089 section 2, JS host): a check that could not look is its own status, and no
// waiver, warning or other finding turns it into a pass.
export const EXIT = { clean: 0, findings: 1, notMeasured: 6, checkBroken: 7 };

// A pass carries its evidence (ADR 0089 amendment 2026-11-13, JS mirror of `paddock_cli::outcome`): `measure` is the only
// maker of a "clean" or "findings" outcome, and it brands the object in a module-private WeakSet, so a result that merely
// says `status: 'clean'` (a literal, a parsed JSON, a copy) is not a pass.
const made = new WeakSet();

/** Why a probe could not give a result: `kind` is 'not-measured' (did not look) or 'broken' (looked, control failed). */
export class Unmeasured extends Error {
  constructor(kind, reason, next = '') {
    super(reason);
    this.kind = kind;
    this.next = next;
  }
}

/**
 * Runs `fn` (sync or async; it returns a list of findings) and gives its outcome: `clean` or `findings` with the evidence
 * {check, subject, tool, at} (branded), `not-measured` or `check-broken` with a reason. A thrown `Unmeasured` says which; any
 * other throw is a crash, `check-broken`. `subject` is { kind: 'commit', sha, tree } or { kind: 'files', head, digest }.
 */
export async function measure(check, subject, tool, fn, now = Date.now()) {
  try {
    const found = await fn();
    const evidence = Object.freeze({ check, subject, tool, at: Math.floor(now / 1000) });
    const outcome = Object.freeze({ status: found.length ? 'findings' : 'clean', evidence, findings: found });
    made.add(outcome);
    return outcome;
  } catch (e) {
    if (e instanceof Unmeasured) return { status: e.kind === 'broken' ? 'check-broken' : 'not-measured', reason: e.message, next: e.next };
    return { status: 'check-broken', reason: `the check crashed: ${String(e?.message ?? e)}`, next: 'Fix the check or the input it could not read; a check that cannot run never passes.' };
  }
}

/** Whether `outcome` is a pass or findings made by `measure`. */
export const isMeasured = (outcome) => made.has(outcome);

/**
 * 7 when a check is broken (it crashed, or its positive control failed), else 1 when an error finding is left (or a warning of a
 * check named in `strict`), else 6 when a check did not look (incomplete, tool missing, nothing probed), else 0: warnings are
 * report-only and never hide a missing measurement. `result` is what `runChecks` returns.
 */
export function exitStatus(result, strict = new Set()) {
  const findings = result.findings ?? [];
  if (findings.some((f) => f.outcome === 'check-broken')) return EXIT.checkBroken;
  if (findings.some((f) => (f.severity === 'error' && !f.outcome) || (f.severity === 'warn' && strict.has(f.check)))) return EXIT.findings;
  if (findings.some((f) => f.outcome === 'not-measured')) return EXIT.notMeasured;
  // An empty run, or a check with no outcome made by `measure` (a bare "clean"), is not a pass (row 6 of the amendment).
  const checks = result.checks ?? [];
  const outcomes = result.outcomes ?? [];
  if (checks.length === 0) return EXIT.notMeasured;
  if (!checks.every((id) => outcomes.some((o) => made.has(o) && o.evidence.check === id))) return EXIT.notMeasured;
  return EXIT.clean;
}
