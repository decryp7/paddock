import { inject, Injectable } from '@angular/core';
import { MemoryActivityBackend } from '@decryp7/paddock-ui/shell/undo';
import { GreetingLog } from './greeting-log';

/**
 * The demo's stand-in for the engine's activity journal (`paddock_jobs::journal`): a greeting is logged as an action that
 * can be undone, and undoing it from the Activity page takes the greeting back.
 */
@Injectable({ providedIn: 'root' })
export class DemoActivity {
  private readonly log = inject(GreetingLog);
  readonly backend = new MemoryActivityBackend(() => this.log.undoGreet());

  recordGreeting(n: number): void {
    this.backend.record({ kind: 'greeting', title: `Said hello (${n})`, detail: 'From the Say hello command', undo: 'available', atMs: Date.now() });
  }
}
