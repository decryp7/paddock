import { InjectionToken } from '@angular/core';
import type { ContributionKind } from './kinds';

/** One registered batch of one kind. */
export interface ContributionEntry {
  readonly kind: string;
  readonly items: readonly unknown[];
}

/** Every batch of every kind (multi provider): what `provideContributions` adds and the registry reads. */
export const SHELL_CONTRIBUTIONS = new InjectionToken<readonly ContributionEntry[]>('SHELL_CONTRIBUTIONS');

/** Contribution kinds besides the shell's own (multi provider, one list per provider; the workbench's, plan W1). */
export const SHELL_CONTRIBUTION_KINDS = new InjectionToken<readonly (readonly ContributionKind[])[]>('SHELL_CONTRIBUTION_KINDS');
