import { Injectable, signal } from '@angular/core';
import type { NotificationKind } from './api.types';
import { readLocalJson, writeLocal } from './storage';

const KEY = 'pitcrew.notifications';

type Muted = NotificationKind[];

function isMuted(v: unknown): v is Muted {
  return Array.isArray(v) && v.every((x) => typeof x === 'string');
}

/** Which notification kinds show as toasts in this window (a per-viewer convenience kept in localStorage). */
@Injectable({ providedIn: 'root' })
export class PreferencesService {
  private readonly muted = signal<ReadonlySet<NotificationKind>>(new Set(readLocalJson<Muted>(KEY, [], isMuted)));

  /** Whether toasts of this kind are shown. */
  isEnabled(kind: NotificationKind): boolean {
    return !this.muted().has(kind);
  }

  setEnabled(kind: NotificationKind, enabled: boolean): void {
    const next = new Set(this.muted());
    if (enabled) next.delete(kind);
    else next.add(kind);
    this.muted.set(next);
    writeLocal(KEY, JSON.stringify([...next]));
  }
}
