import { ApplicationRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { startupLoadsSettled } from '../app/extensions/startup-loads';

/**
 * Resolves when the code loads that start-up began (the lazy command sets' chunks, see `app/extensions/startup-loads.ts`)
 * are done and the application has nothing else pending. For a spec that boots `appConfig` and wants start-up finished
 * at a known point; it waits on the loads themselves, never on a clock, and never starts a load of its own.
 */
export async function settleStartup(): Promise<void> {
  await startupLoadsSettled();
  await TestBed.inject(ApplicationRef).whenStable();
}

/**
 * The shared `afterEach` of `test-setup.ts`: waits for the loads start-up began before TestBed tears the module down, so no
 * chunk is imported into a torn-down environment (Vitest reports that as an `EnvironmentTeardownError` against whichever
 * file runs next: debrief.yml run 8170).
 *
 * It reads the record only: it never touches TestBed (a hook that built the module could throw, and Vitest would then
 * skip Angular's own cleanup and blame the next test), never starts a load, never rejects (a failed start-up load is
 * swallowed by its caller by design, and `lazy-commands.spec.ts` asserts that every set loads) and waits on real
 * promises only, so it ends under fake timers. A hook that times out (a load that never settles) still skips Angular's own
 * cleanup for that one test; the record is drained first, so no later test waits on it. Covered: the lazy command sets' chunks, which are Debrief's only start-up
 * `import()`s (it has no app-context prefetch; its initializers import nothing lazily).
 */
export async function settleBootedStartup(): Promise<void> {
  await startupLoadsSettled();
}
