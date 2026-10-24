import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import type { ShellNotice } from '@decryp7/paddock-ui/shell';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import { NotificationsBackend, type Unlisten } from './backends';
import { NOTIFY_CLOCK } from './notify-config';
import { EMPTY_NOTICES, type Notice, type NoticesEvent, type NoticesSnapshot } from './wire.types';

/** Which messages the centre lists. */
export type ReadFilter = 'all' | 'unread';

/** The cause prefix of the UI's own notices (`SHELL_PORTS.notify`). */
export const LOCAL_CAUSE = 'ui:';

/** The source of a local notice without a kind. */
export const LOCAL_SOURCE = 'app';

interface NotificationsState {
  /** The engine's list for this window, newest first. */
  snapshot: NoticesSnapshot;
  /** The UI's own notices of this session (negative ids), newest first. */
  local: readonly Notice[];
  /** Messages previewed as toasts (the latest occurrence of each cause), oldest first. */
  toasts: readonly Notice[];
  read: ReadFilter;
  /** The message to reveal in the centre (an OS notification was clicked). */
  focus: number | null;
}

const FALLBACK = (): string => $localize`:@@pd.notifications.no-answer:The notification centre did not answer.`;

/** The cause of a local notice: the same kind and text is the same cause, so a repeat merges with a count. */
export function localCause(n: ShellNotice): string {
  return `${LOCAL_CAUSE}${n.kind ?? n.level}:${n.message}`;
}

/** Puts `n` in place of the entry with its cause (newest last). */
function upsertLast(list: readonly Notice[], n: Notice): Notice[] {
  return [...list.filter((t) => t.cause !== n.cause), n];
}

/**
 * The window's notifications (ADR 0010): the engine's centre as it reports it (dedupe by cause with counts, caps and read
 * state are the engine's), the UI's own notices posted through `SHELL_PORTS.notify` (merged by cause the same way, for
 * this session), the toast previews of what was just posted, and the centre's read filter. Provided per window by
 * `provideShellNotify`.
 */
export const NotificationsStore = signalStore(
  { protectedState: true },
  withState<NotificationsState>({ snapshot: EMPTY_NOTICES, local: [], toasts: [], read: 'all', focus: null }),
  withRequestStatus(),
  withComputed((store) => {
    const all = computed(() => [...store.local(), ...store.snapshot().notices].sort((a, b) => b.at - a.at || b.id - a.id));
    const filtered = computed(() => (store.read() === 'all' ? all() : all().filter((n) => !n.read)));
    return {
      /** Every message of the window (engine and local), newest first. */
      all,
      notices: filtered,
      unread: computed(() => store.snapshot().unread + store.local().filter((n) => !n.read).length),
      /** The listed messages grouped by source, in the order sources first appear (newest first). */
      groups: computed(() => {
        const groups = new Map<string, Notice[]>();
        for (const n of filtered()) groups.set(n.source, [...(groups.get(n.source) ?? []), n]);
        return [...groups.entries()].map(([source, notices]) => ({ source, notices }));
      }),
    };
  }),
  withMethods((store, backend = inject(NotificationsBackend), clock = inject(NOTIFY_CLOCK)) => {
    let localIds = 0;
    const call = async <T>(f: () => Promise<T>): Promise<T | null> => {
      try {
        return await f();
      } catch (e) {
        patchState(store, setError(errorMessage(e, FALLBACK())));
        return null;
      }
    };
    // Answers and events can cross (a list read before a post answers after its event): a window keeps the newest revision.
    const take = (next: NoticesSnapshot): boolean => {
      if (next.revision < store.snapshot().revision) return false;
      patchState(store, { snapshot: { revision: next.revision, notices: next.notices, unread: next.unread } });
      return true;
    };
    const dismissToast = (cause: string): void => patchState(store, (s) => ({ toasts: s.toasts.filter((t) => t.cause !== cause) }));
    const isLocal = (id: number): boolean => id < 0;
    return {
      /** A change from the engine (an older one than the window has is ignored): the new list, and a toast for the message just posted (unless already read). */
      apply(e: NoticesEvent): void {
        if (!take(e)) return;
        const p = e.posted;
        if (p && !p.read) patchState(store, (s) => ({ toasts: upsertLast(s.toasts, p) }));
        // A message the engine resolved or dismissed takes its toast with it.
        const listed = new Set(e.notices.map((n) => n.cause));
        patchState(store, (s) => ({ toasts: s.toasts.filter((t) => isLocal(t.id) || listed.has(t.cause)) }));
      },

      /** Reads the list (also the Retry of a failed read). Never toasts: these messages are not new. */
      async load(): Promise<void> {
        patchState(store, setLoading());
        try {
          take(await backend.list());
          patchState(store, setLoaded());
        } catch (e) {
          patchState(store, setError(errorMessage(e, FALLBACK())));
        }
      },

      /** `SHELL_PORTS.notify`: a notice of the UI's own; the same kind and text again merges into it with a count. */
      post(n: ShellNotice): Notice {
        const cause = localCause(n);
        const at = clock.now();
        const known = store.local().find((l) => l.cause === cause);
        const notice: Notice = known
          ? { ...known, count: known.count + 1, at: Math.max(at, known.at), read: false }
          : {
              id: -++localIds,
              source: n.kind ?? LOCAL_SOURCE,
              severity: n.level,
              title: n.message,
              body: '',
              code: null,
              at,
              firstAt: at,
              count: 1,
              read: false,
              primary: null,
              secondary: null,
              tertiary: null,
              link: { window: null, jobId: null, path: null },
              cause,
              scope: 'project',
            };
        patchState(store, (s) => ({ local: [notice, ...s.local.filter((l) => l.cause !== cause)], toasts: upsertLast(s.toasts, notice) }));
        return notice;
      },

      async markRead(id: number): Promise<void> {
        if (isLocal(id)) patchState(store, (s) => ({ local: s.local.map((l) => (l.id === id ? { ...l, read: true } : l)) }));
        else await call(() => backend.markRead(id));
      },

      async markAllRead(): Promise<void> {
        patchState(store, (s) => ({ local: s.local.map((l) => ({ ...l, read: true })) }));
        if (store.snapshot().unread > 0) await call(() => backend.markAllRead());
      },

      /** Removes a message from the centre (and its toast). */
      async dismiss(id: number): Promise<void> {
        const n = store.all().find((x) => x.id === id);
        if (n) dismissToast(n.cause);
        if (isLocal(id)) patchState(store, (s) => ({ local: s.local.filter((l) => l.id !== id) }));
        else await call(() => backend.dismiss(id));
      },

      /** The toast of `cause` goes (dismissed, or its time ran out); the message stays in the centre. */
      dismissToast,

      setRead(read: ReadFilter): void {
        patchState(store, { read });
      },

      setFocus(id: number | null): void {
        patchState(store, { focus: id });
      },
    };
  }),
  withHooks((store, backend = inject(NotificationsBackend)) => {
    const offs: Unlisten[] = [];
    let destroyed = false;
    const keep = (u: Unlisten): void => {
      if (destroyed) u();
      else offs.push(u);
    };
    const warn = (e: unknown): void => console.warn(`[paddock-notify] cannot listen for notifications: ${errorMessage(e, FALLBACK())}`);
    return {
      onInit() {
        void backend.listen((e) => store.apply(e)).then(keep).catch(warn);
        void backend.onOpen((id) => store.setFocus(id)).then(keep).catch(warn);
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        for (const off of offs) off();
      },
    };
  }),
);

export type NotificationsStoreType = InstanceType<typeof NotificationsStore>;
