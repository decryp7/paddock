import { mockJobs, resetMockJobs } from '../app/jobs/mock-jobs';

/**
 * Ends the mock jobs a test started: cancels every one still queued or running and drops the service, so none holds its kind's
 * slot or keeps a step waiting after the test. A spec that starts jobs (an export, an import, a re-read) and does not wait for
 * them registers it as `afterEach(endMockJobs)`; the leak detector names a test that leaves jobs without it
 * (`test-setup.ts` registers the mock jobs as a singleton).
 */
export function endMockJobs(): void {
  mockJobs().cancelWhere(() => true);
  resetMockJobs();
}
