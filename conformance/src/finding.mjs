/** One finding. `fix` is the fix in one sentence (conformance proposal). */
export function finding(check, severity, rule, file, line, message, fix) {
  return { check, severity, rule, file, line: line ?? 1, message, fix };
}
export const error = (check, rule, file, line, message, fix) => finding(check, 'error', rule, file, line, message, fix);
export const warn = (check, rule, file, line, message, fix) => finding(check, 'warn', rule, file, line, message, fix);
/**
 * An error that no waiver can cover: the check could not look (ER-33). `outcome` is 'check-broken' (it crashed, or its positive
 * control failed; the default, because it fails closed with the higher exit status) or 'not-measured' (it did not look).
 */
export const unwaivable = (f, outcome = 'check-broken') => ({ ...f, unwaivable: true, outcome });
