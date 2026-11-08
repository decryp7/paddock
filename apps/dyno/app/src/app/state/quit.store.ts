import { computed, inject, Injectable, signal } from '@angular/core';
import { BACKEND } from '../core/backend';

/** The words of Quit when the agent keeps measuring after it, and when it does not. The engine decides which (the login service is installed and supervising). */
export const QUIT_KEEPS = 'Quit Dyno (keeps measuring)';
export const QUIT_PLAIN = 'Quit Dyno';
/** The entry that stops the agent too. */
export const QUIT_STOP = 'Quit and stop measuring';

/** What Quit does, from the engine: shown on every Quit (the menu, the tray menu and the tray popup). */
@Injectable({ providedIn: 'root' })
export class QuitStore {
  private readonly backend = inject(BACKEND);
  /** The agent keeps measuring after a plain Quit. */
  readonly keeps = signal(false);
  /** The title of Quit for that state. */
  readonly title = computed(() => (this.keeps() ? QUIT_KEEPS : QUIT_PLAIN));

  /** Asks the engine again; a failed look keeps the last answer (the menu never blocks on it). */
  async refresh(): Promise<void> {
    try {
      this.keeps.set(await this.backend.quitKeepsMeasuring());
    } catch {
      /* the last answer stays */
    }
  }
}
