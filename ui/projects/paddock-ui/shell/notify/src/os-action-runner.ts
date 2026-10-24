import { DestroyRef, inject, Injectable } from '@angular/core';
import { NotificationsBackend, type Unlisten } from './backends';
import { NoticeActions } from './notice-actions';
import { NotificationsStore } from './notifications.store';
import type { NoticeActionEvent } from './wire.types';

/**
 * Runs the button of an OS notification that the engine left to this window (`openSettings`, an app action): the engine
 * sends the message's action here (`paddock://notifications-action`, only to the window that shows the message) and it
 * runs as the centre's own button would. A failure, or a message that is gone, is a notice of the window and not a silent
 * click. Created once per window by `provideShellNotify`.
 */
@Injectable()
export class OsActionRunner {
  private readonly store = inject(NotificationsStore);
  private readonly actions = inject(NoticeActions);
  private readonly offs: Unlisten[] = [];
  private destroyed = false;

  constructor() {
    const warn = (e: unknown): void => console.warn(`[paddock-notify] cannot listen for notification actions: ${e instanceof Error ? e.message : String(e)}`);
    void inject(NotificationsBackend)
      .onAction((e) => void this.run(e))
      .then((off) => (this.destroyed ? off() : void this.offs.push(off)))
      .catch(warn);
    inject(DestroyRef).onDestroy(() => {
      this.destroyed = true;
      for (const off of this.offs) off();
    });
  }

  /** Runs the action of `e` on the message it belongs to. */
  async run(e: NoticeActionEvent): Promise<void> {
    const notice = this.store.all().find((n) => n.id === e.id);
    const why = notice ? await this.actions.run(e.action, notice) : $localize`:@@pd.notifications.action-gone:That notification is no longer listed.`;
    if (why !== null) await this.store.post({ level: 'error', message: why, kind: 'notification-action' });
  }
}
