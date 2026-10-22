import { Injectable, signal } from '@angular/core';

export type CpuView = 'overall' | 'cores';
export type DashboardView = 'standard' | 'widgets';
const KEY = 'dyno.cpuView';
const DASH_KEY = 'dyno.dashboardView';

/** Per-user view choices that are not settings (they live in this browser/webview only). Storage may be blocked: it just forgets. */
@Injectable({ providedIn: 'root' })
export class ViewPrefs {
  readonly cpuView = signal<CpuView>(read());
  readonly dashboardView = signal<DashboardView>(readDash());

  setCpuView(v: CpuView): void {
    this.cpuView.set(v);
    try {
      localStorage.setItem(KEY, v);
    } catch {
      // Private mode or blocked storage.
    }
  }

  setDashboardView(v: DashboardView): void {
    this.dashboardView.set(v);
    try {
      localStorage.setItem(DASH_KEY, v);
    } catch {
      // Private mode or blocked storage.
    }
  }
}

function readDash(): DashboardView {
  try {
    return localStorage.getItem(DASH_KEY) === 'widgets' ? 'widgets' : 'standard';
  } catch {
    return 'standard';
  }
}

function read(): CpuView {
  try {
    return localStorage.getItem(KEY) === 'cores' ? 'cores' : 'overall';
  } catch {
    return 'overall';
  }
}
