import { Injectable, signal } from '@angular/core';

export type CpuView = 'overall' | 'cores';
const KEY = 'dyno.cpuView';

/** Per-user view choices that are not settings (they live in this browser/webview only). Storage may be blocked: it just forgets. */
@Injectable({ providedIn: 'root' })
export class ViewPrefs {
  readonly cpuView = signal<CpuView>(read());

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
