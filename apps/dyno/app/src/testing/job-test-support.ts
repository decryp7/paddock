import type { EnvironmentProviders } from '@angular/core';
import { MemoryJobsBackend, provideShellJobs } from '@decryp7/paddock-ui/shell/jobs';
import { MockBackend, type MockOptions } from '../app/core/mock-backend';

/**
 * Test support: a mock that plays the agent and the shell's in-memory jobs backend it pushes its job statuses into (what the
 * browser build wires too), so a spec follows a job the way the app does: through the shell's jobs store. Cancel from the
 * store reaches the mock.
 */
export function mockWithJobs(options: MockOptions = {}): { mock: MockBackend; jobs: MemoryJobsBackend } {
  const jobs = new MemoryJobsBackend((id) => void mock.cancelJob(id));
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, ...options, jobs });
  return { mock, jobs };
}

/** The shell's jobs store, follow helper and backend over `jobs` (what `provideShellPages` and the dialog windows provide). */
export function jobsProviders(jobs: MemoryJobsBackend, stillWorkingAfterMs = 8_000): EnvironmentProviders {
  return provideShellJobs({ backend: () => jobs, stillWorkingAfterMs });
}

/** Lets `ms` of fake time pass and the shell's jobs store run its effects (its "Still working" clock starts in one). */
export async function pump(ms: number): Promise<void> {
  const { TestBed } = await import('@angular/core/testing');
  const { vi } = await import('vitest');
  await vi.advanceTimersByTimeAsync(ms);
  TestBed.tick();
}
