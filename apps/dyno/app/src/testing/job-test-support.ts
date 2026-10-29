import { vi } from 'vitest';
import type { JobStatus } from '../app/core/api.types';
import type { MockBackend } from '../app/core/mock-backend';

/**
 * Test support: from now on the mock reports the job exactly as it is at the next read, forever (a step that takes a long time and
 * reports nothing new), so a spec can let minutes pass and see "still working" and no error.
 */
export function freezeJobs(mock: MockBackend): void {
  const real = mock.jobs.bind(mock);
  let frozen: JobStatus[] | null = null;
  vi.spyOn(mock, 'jobs').mockImplementation(async () => (frozen ??= await real()));
}
