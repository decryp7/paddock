import { Injectable, inject } from '@angular/core';
import { Notifier } from '../state/notifier';

/** Copies text to the clipboard and says so; a clipboard that is not available gets a message instead of an exception. */
@Injectable({ providedIn: 'root' })
export class Clipboard {
  private readonly toast = inject(Notifier);

  async copy(text: string, what: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.toast.success(`${what} copied`);
    } catch {
      this.toast.error('Could not copy', 'The clipboard is not available. Select the text and copy it with the keyboard.');
    }
  }
}
