/** One finding. `fix` is the fix in one sentence (conformance proposal). */
export function finding(check, severity, rule, file, line, message, fix) {
  return { check, severity, rule, file, line: line ?? 1, message, fix };
}
export const error = (check, rule, file, line, message, fix) => finding(check, 'error', rule, file, line, message, fix);
export const warn = (check, rule, file, line, message, fix) => finding(check, 'warn', rule, file, line, message, fix);
