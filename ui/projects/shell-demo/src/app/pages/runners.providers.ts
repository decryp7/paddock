import { computed, inject, Injectable, signal, type Provider } from '@angular/core';
import { RunnerData, type Runner } from '../runner-data';

let visits = 0;

/**
 * The Runners page's own state (rule 32): declared in the page's `providers` loader, so it is a lazy chunk of its own,
 * created when the page opens and destroyed when it closes. Each visit gets a new one.
 */
@Injectable()
export class RunnersPageState {
  readonly visit = ++visits;
  private readonly data = inject(RunnerData);
  readonly sort = signal<'name' | 'jobs'>('name');
  readonly rows = computed<readonly Runner[]>(() =>
    [...this.data.runners].sort((a, b) => (this.sort() === 'jobs' ? b.jobsToday - a.jobsToday : a.name.localeCompare(b.name))),
  );
}

export const RUNNERS_PROVIDERS: readonly Provider[] = [RunnersPageState];
