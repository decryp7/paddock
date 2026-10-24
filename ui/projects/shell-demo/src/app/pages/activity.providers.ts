import { Injectable, type Provider } from '@angular/core';

let visits = 0;

/**
 * A page-scoped service (rule 32): the Activity page declares it in its `providers` loader, so it is a lazy chunk of its
 * own, created when the page opens and destroyed when the page closes. Each visit gets a new one.
 */
@Injectable()
export class ActivityVisit {
  readonly number = ++visits;
  readonly openedAt = new Date();
}

export const ACTIVITY_PROVIDERS: readonly Provider[] = [ActivityVisit];
