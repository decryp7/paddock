import type { UpdateCheckStatus } from '../../core/api.types';
import { checkSummary } from './update-text';

const NOW = 1_800_000_000_000;
const HOUR = 3_600_000;
const status = (p: Partial<UpdateCheckStatus> = {}): UpdateCheckStatus => ({
  settings: { enabled: true, schedule: 'weekly' },
  lastCheckedAt: null,
  lastAttemptAt: null,
  lastMessage: null,
  nextCheckAt: null,
  running: false,
  notices: 0,
  host: 'huggingface.co',
  maxRepos: 40,
  ...p,
});

describe('checkSummary', () => {
  it('says when it last ran and when it runs next', () => {
    expect(checkSummary(status({ lastCheckedAt: NOW - 3 * HOUR, lastAttemptAt: NOW - 3 * HOUR, nextCheckAt: NOW + 6 * 24 * HOUR }), NOW)).toBe('Last checked 3 h ago. The next check runs in 6 days.');
  });

  it('says it was never checked, and that the first check runs soon once on', () => {
    expect(checkSummary(status({ nextCheckAt: NOW }), NOW)).toBe('Never checked. The next check runs soon.');
  });

  it('explains why nothing is scheduled: off', () => {
    expect(checkSummary(status({ settings: { enabled: false, schedule: 'weekly' } }), NOW)).toBe('Never checked. Scheduled checks are off; Check now still works.');
  });

  it('reports an attempt that did not finish', () => {
    const s = status({ lastCheckedAt: NOW - 30 * HOUR, lastAttemptAt: NOW - HOUR, lastMessage: 'Hugging Face could not be reached.', nextCheckAt: NOW + HOUR });
    expect(checkSummary(s, NOW)).toContain('The last attempt did not finish: Hugging Face could not be reached.');
    expect(checkSummary(status({ lastCheckedAt: NOW, lastAttemptAt: NOW, lastMessage: 'Asked Hugging Face about 3 repositories.' }), NOW)).not.toContain('did not finish');
  });
});
