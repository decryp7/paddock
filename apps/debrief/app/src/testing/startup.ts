import { ApplicationRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ShellContextHost } from '@decryp7/paddock-ui/shell/contributions';
import { LAZY_COMMAND_SETS } from '../app/extensions/lazy-registration';

/**
 * Resolves when the app's start-up work that loads code is done: the app context's chunk, every lazy command set's
 * chunk, and whatever the application still has pending. It waits on the loads themselves, never on a clock, and only
 * waits: a load that fails is the concern of the spec that asked for it (`lazy-commands.spec.ts`).
 */
export async function settleStartup(): Promise<void> {
  await settleLoads();
  await TestBed.inject(ApplicationRef).whenStable();
}

/** The chunk loads alone: they are real promises, so this also ends under fake timers (`whenStable` would not). */
async function settleLoads(): Promise<void> {
  await Promise.allSettled([TestBed.inject(ShellContextHost).load()]);
  await Promise.allSettled(TestBed.inject(LAZY_COMMAND_SETS, []).map((set) => set.load()));
}

/**
 * The shared `afterEach` of `test-setup.ts`: when the test booted the app's lazy registrations, waits for their chunks
 * before TestBed tears the module down. Start-up begins those loads on its own (idle work, a gated key's microtask), so a
 * chunk that is still loading when the module goes away is imported after the environment is torn down, and Vitest
 * reports an `EnvironmentTeardownError` against whichever file runs next (debrief.yml run 8170).
 */
export async function settleBootedStartup(): Promise<void> {
  if (TestBed.inject(LAZY_COMMAND_SETS, null) === null) return;
  await settleLoads();
}
