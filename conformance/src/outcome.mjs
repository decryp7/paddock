// The exit status of a conformance run (ADR 0089 section 2, JS host): a check that could not look is its own status, and no
// waiver, warning or other finding turns it into a pass.
export const EXIT = { clean: 0, findings: 1, notMeasured: 6, checkBroken: 7 };

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
  return EXIT.clean;
}
