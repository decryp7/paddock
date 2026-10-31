// CI diagnostics for Node scripts (rule 56: logs explain every failure); the bash twin is ci-log.sh. Wrapping a native program
// to get its real exit code, NTSTATUS and stderr tail is run-native.mjs's job, not this file's.
//
//   import { ciError, phase, writeSummary } from '../../conformance/ci/ci-log.mjs';
//   ciError('3 test folders left in $TMP', 'fix the tests named above; they must clean up on panic');
//   const plan = await phase('plan', () => computePlan());            // "ci: plan: ok in 1.2 s" (or FAILED, then rethrows)
//   writeSummary('### Plan\n\n...');                                    // $GITHUB_STEP_SUMMARY when set, and the log
//
// Every failure prints exactly one `::error::` line, cause then "Next step:". The prefix is assembled here, so grepping a log
// for `::error::` finds what failed, never the echoed text of a script.
import { appendFileSync } from 'node:fs';

/** "12.3 s" for a duration in milliseconds. */
export function formatDuration(ms) {
  const v = Math.max(0, Number(ms) || 0);
  return `${(Math.floor(v / 100) / 10).toFixed(1)} s`;
}

/** The one-line annotation for a failure: its cause and the next step, newlines folded so it stays one line. */
export function formatError(cause, next = '') {
  const msg = next ? `${cause}. Next step: ${next}` : String(cause);
  return `::${'error'}::${msg.replace(/\r/g, '').replace(/\n/g, ' / ')}`;
}

/** Prints `formatError(cause, next)` (to stdout, where the runner reads annotations). */
export function ciError(cause, next = '', out = (l) => console.log(l)) {
  out(formatError(cause, next));
}

/**
 * Runs one phase of a long step and logs its outcome and duration; a failure is logged with its message and rethrown.
 * Works for sync and async functions.
 */
export async function phase(label, fn, { log = (l) => console.log(l), now = () => Date.now() } = {}) {
  const start = now();
  log(`ci: ${label}: started`);
  try {
    const value = await fn();
    log(`ci: ${label}: ok in ${formatDuration(now() - start)}`);
    return value;
  } catch (e) {
    log(`ci: ${label}: FAILED in ${formatDuration(now() - start)}: ${e?.message ?? e}`);
    throw e;
  }
}

/**
 * Writes Markdown to the job summary when the runner sets $GITHUB_STEP_SUMMARY, and always to the log (whether a Gitea server
 * renders job summaries depends on its and the runner's version, go-gitea/gitea#23721). Never throws.
 */
export function writeSummary(markdown, { env = process.env, log = (l) => console.log(l) } = {}) {
  for (const line of String(markdown).trimEnd().split('\n')) log(`ci: ${line}`);
  const file = env.GITHUB_STEP_SUMMARY;
  if (!file) return false;
  try {
    appendFileSync(file, `${String(markdown).trimEnd()}\n\n`);
    return true;
  } catch (e) {
    log(`ci: could not write the step summary to ${file} (${e.code ?? e.message}); the log above has it`);
    return false;
  }
}
