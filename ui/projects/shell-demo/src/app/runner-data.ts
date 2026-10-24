import { Injectable } from '@angular/core';

/** One build runner of the demo's sample fleet (made-up, plausible manager data for screenshots). */
export interface Runner {
  readonly name: string;
  readonly host: string;
  readonly state: 'online' | 'busy' | 'offline';
  readonly jobsToday: number;
  readonly lastJob: string;
}

const RUNNERS: readonly Runner[] = [
  { name: 'build-mac-1', host: 'mac-mini-01.lan', state: 'busy', jobsToday: 14, lastJob: '2 min ago' },
  { name: 'build-mac-2', host: 'mac-mini-02.lan', state: 'online', jobsToday: 9, lastJob: '18 min ago' },
  { name: 'build-linux-1', host: 'nuc-03.lan', state: 'online', jobsToday: 22, lastJob: '5 min ago' },
  { name: 'build-win-1', host: 'win-vm-01.lan', state: 'offline', jobsToday: 0, lastJob: 'Yesterday, 18:40' },
];

/** The demo's fleet: a root service, so the status item and the page badge read it without the page. */
@Injectable({ providedIn: 'root' })
export class RunnerData {
  readonly runners = RUNNERS;
  readonly online = RUNNERS.filter((r) => r.state !== 'offline').length;
  /** Runners that need the user (offline). */
  readonly attention = RUNNERS.filter((r) => r.state === 'offline').length;
}
