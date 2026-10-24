import { computed, inject, Injectable } from '@angular/core';
import type { HostAction, HostNotification, NotificationSource } from './notification-host.model';
import { NoticeActions } from './notice-actions';
import { NotificationsStore } from './notifications.store';
import type { Notice, NoticeAction } from './wire.types';

/**
 * The centre's new messages and the UI's own notices as toasts: the cause key is the toast's cause, so a repeat updates
 * the toast in place (with its count) instead of adding another. Registered with the host by `provideShellNotify`.
 */
@Injectable()
export class NotificationsSource implements NotificationSource {
  private readonly store = inject(NotificationsStore);
  private readonly actions = inject(NoticeActions);

  readonly items = computed<HostNotification[]>(() => this.store.toasts().map((t) => this.adapt(t)));

  private adapt(t: Notice): HostNotification {
    const run = async (a: NoticeAction): Promise<void> => {
      const err = await this.actions.run(a, t);
      if (err) throw new Error(err);
      this.store.dismissToast(t.cause);
    };
    const act = (a: NoticeAction | null, kind: HostAction['kind'], id: string): HostAction[] =>
      a && this.actions.available(a) ? [{ label: this.actions.label(a, t), kind, id, run: () => run(a) }] : [];
    const actions: HostAction[] = [...act(t.primary, 'primary', 'primary'), ...act(t.secondary, 'default', 'secondary'), ...act(t.tertiary, 'default', 'tertiary')];
    actions.push({
      label: $localize`:@@pd.notifications.show:Show in Notifications`,
      kind: 'ghost',
      id: 'show',
      run: () => {
        this.store.dismissToast(t.cause);
        this.store.setFocus(t.id);
      },
    });
    return {
      cause: t.cause,
      severity: t.severity,
      title: t.title,
      body: t.body || null,
      count: t.count,
      actions,
      stamp: `${t.at}:${t.count}`,
      dismiss: () => this.store.dismissToast(t.cause),
    };
  }
}
