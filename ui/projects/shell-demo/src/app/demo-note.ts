import { Injectable, signal } from '@angular/core';
import type { UiCheckpoint } from '@decryp7/paddock-ui/shell/startup';

/** The demo window's UI state: a note, kept across a recovery reload by the start-up contract's checkpoint (plan S7). */
@Injectable({ providedIn: 'root' })
export class DemoNote {
  readonly note = signal('');

  readonly checkpoint: UiCheckpoint = {
    capture: () => ({ note: this.note() }),
    restore: (ui) => {
      const note = (ui as { note?: unknown }).note;
      if (typeof note === 'string') this.note.set(note);
    },
  };
}
