import type { UpdateCheckStatus } from '../../core/api.types';
import { ago } from '../../shared/format';

/** Settings text for the state of the checks: when it last ran, when it runs next, and why it cannot. */
export function checkSummary(s: UpdateCheckStatus, now: number): string {
  const last = s.lastCheckedAt === null ? 'Never checked.' : `Last checked ${ago(s.lastCheckedAt, now)}.`;
  let next = '';
  if (s.offline) next = ' Offline mode is on, so no check can run.';
  else if (s.nextCheckAt !== null) next = s.nextCheckAt <= now ? ' The next check runs soon.' : ` The next check runs ${ago(s.nextCheckAt, now)}.`;
  else if (!s.settings.enabled) next = ' Scheduled checks are off; Check now still works.';
  const failed = s.lastAttemptAt !== null && s.lastAttemptAt !== s.lastCheckedAt && s.lastMessage ? ` The last attempt did not finish: ${s.lastMessage}` : '';
  return `${last}${next}${failed}`;
}
