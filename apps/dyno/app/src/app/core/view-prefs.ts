import { Injectable, signal } from '@angular/core';
import type { StartupFilter } from './api.types';

export type CpuView = 'overall' | 'cores';
const KEY = 'dyno.cpuView';
const STARTUP_KEY = 'dyno.startupFilter';

/** Per-user view choices that are not settings (they live in this browser/webview only). Storage may be blocked: it just forgets. */
@Injectable({ providedIn: 'root' })
export class ViewPrefs {
  readonly cpuView = signal<CpuView>(read());
  /** What the startup manager hides (remembered). */
  readonly startupFilter = signal<StartupFilter>(readStartupFilter());

  setStartupFilter(f: StartupFilter): void {
    this.startupFilter.set(f);
    try {
      localStorage.setItem(STARTUP_KEY, JSON.stringify(f));
    } catch {
      // Private mode or blocked storage.
    }
  }

  setCpuView(v: CpuView): void {
    this.cpuView.set(v);
    try {
      localStorage.setItem(KEY, v);
    } catch {
      // Private mode or blocked storage.
    }
  }
}

function read(): CpuView {
  try {
    return localStorage.getItem(KEY) === 'cores' ? 'cores' : 'overall';
  } catch {
    return 'overall';
  }
}

function readStartupFilter(): StartupFilter {
  const none = { hideOs: false, hideSigned: false };
  try {
    const v: unknown = JSON.parse(localStorage.getItem(STARTUP_KEY) ?? 'null');
    if (typeof v === 'object' && v !== null) {
      const { hideOs, hideSigned } = v as Record<string, unknown>;
      return { hideOs: hideOs === true, hideSigned: hideSigned === true };
    }
  } catch {
    // Missing, damaged or blocked: show everything.
  }
  return none;
}
