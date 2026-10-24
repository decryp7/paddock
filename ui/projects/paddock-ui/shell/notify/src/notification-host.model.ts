import { InjectionToken, type Signal } from '@angular/core';
import { MAX_VISIBLE } from './notify-config';
import { isHealthCause, type NoticeSeverity } from './wire.types';

/** A button on a toast. */
export interface HostAction {
  readonly label: string;
  readonly run: () => void | Promise<void>;
  readonly kind?: 'primary' | 'default' | 'ghost';
  /** Names the action (`notification-action-<id>` is its test id). */
  readonly id?: string;
}

/**
 * One notification as the host shows it. Every notification has a `cause`: the same cause is one toast, updated in place.
 * Features adapt their own state to this shape (a `NotificationSource`); the host knows nothing of them.
 */
export interface HostNotification {
  readonly cause: string;
  readonly severity: NoticeSeverity;
  readonly title: string;
  /** A short sentence under the title. */
  readonly body?: string | null;
  /** Technical text, folded behind "Details". */
  readonly details?: string | null;
  /** Tells one occurrence of the cause from the next (Details fold again for each). Defaults to the cause. */
  readonly instance?: string | number;
  /** How many times the cause happened. */
  readonly count?: number;
  readonly actions?: readonly HostAction[];
  /** Marks one occurrence (its latest time): a new stamp starts the toast's time over. */
  readonly stamp?: string | number;
  /** How long a toast that times out stays, ms (default `TOAST_MS`). */
  readonly ttl?: number;
  /** Stays until dismissed or resolved, whatever its severity. */
  readonly sticky?: boolean;
  /** The user dismissed it, or its time ran out (the source forgets the toast). Sources only post, update and resolve. */
  readonly dismiss: () => void;
}

/** A feature's toasts, as a signal the host reads. The source owns lifetimes; the host owns the time. */
export interface NotificationSource {
  readonly items: Signal<readonly HostNotification[]>;
}

export const NOTIFICATION_SOURCES = new InjectionToken<readonly NotificationSource[]>('PADDOCK_NOTIFICATION_SOURCES');

/**
 * The one display rule as a pure function: one toast per cause (a later entry with the same cause replaces an earlier
 * one and takes the later position), a cause the status chip states (`health:`) is never toasted, and at most `max` (the
 * newest) are shown.
 */
export function visibleNotifications(items: readonly HostNotification[], max = MAX_VISIBLE): HostNotification[] {
  const byCause = new Map<string, HostNotification>();
  for (const n of items) {
    if (isHealthCause(n.cause)) continue;
    byCause.delete(n.cause);
    byCause.set(n.cause, n);
  }
  return [...byCause.values()].slice(-max);
}

/** Errors, critical failures and sticky toasts stay until resolved or dismissed; the rest time out. */
export function persists(n: Pick<HostNotification, 'severity' | 'sticky'>): boolean {
  return !!n.sticky || n.severity === 'error' || n.severity === 'critical';
}
