import { InjectionToken } from '@angular/core';
import type { Notice, NoticeAction, RaiseCap } from './wire.types';

/** How long a toast that times out stays, ms (the message stays in the centre). */
export const TOAST_MS = 8000;

/** The most toasts at once; older ones wait (hidden) until a newer one goes. */
export const MAX_VISIBLE = 3;

/** How long after the pointer or focus leaves a toast the host waits before it lets go of the stack (crossing the gap between two toasts). */
export const HOLD_RELEASE_MS = 150;

/**
 * The clock of the notification host and the centre: the time, and a repeating tick the host uses for its toasts' time.
 * A port, so tests and e2e drive it. Nothing here times out work: a toast going away leaves its message in the centre.
 */
export interface NotifyClock {
  now(): number;
  /** Calls `tick` every `ms` until the returned function is called. */
  every(ms: number, tick: () => void): () => void;
}

export const NOTIFY_CLOCK = new InjectionToken<NotifyClock>('PADDOCK_NOTIFY_CLOCK', {
  providedIn: 'root',
  factory: () => ({
    now: () => Date.now(),
    every: (ms, tick) => {
      const handle = setInterval(tick, ms);
      return () => clearInterval(handle);
    },
  }),
});

/**
 * Opens Settings at a section (the `openSettings` repair and notice action). The app provides it (plan S10); without one
 * no button for that repair is shown (never a button that does nothing).
 */
export const OPEN_SETTINGS = new InjectionToken<(section: string) => void | Promise<void>>('PADDOCK_OPEN_SETTINGS');

/** Raises the memory cap a process was stopped over. The app provides it; without one the chip offers no "Raise the cap…". */
export const RAISE_CAP = new InjectionToken<(cap: RaiseCap) => void | Promise<void>>('PADDOCK_RAISE_CAP');

/**
 * What a notice button of a given `type` does (`{ type: 'installUpdate', version }`). The shell handles `repairService`
 * and `openSettings`; an app adds its own with `provideNoticeAction`. A type nobody handles shows no button.
 */
export interface NoticeActionHandler {
  readonly type: string;
  /** The button's text. */
  label(action: NoticeAction, notice: Notice): string;
  /** Runs it; a rejection keeps the message and shows why. */
  run(action: NoticeAction, notice: Notice): void | Promise<void>;
}

export const NOTICE_ACTION_HANDLERS = new InjectionToken<readonly NoticeActionHandler[]>('PADDOCK_NOTICE_ACTION_HANDLERS');
