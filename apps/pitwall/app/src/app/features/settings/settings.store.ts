import { InjectionToken, computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { SettingsCloseVerdict } from '@decryp7/paddock-ui/shell/settings/window';
import type { CacheClearScope, CacheInfo, CommandError, Preset, Settings } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';
import { PRESET_LABEL } from '../../shared/labels';
import { bytes, seconds } from '../../shared/format';

type Section = keyof Settings;

/** How long a typed value waits after the last keystroke before it is saved (a switch or a choice saves at once). */
export const SETTINGS_SAVE_DELAY = new InjectionToken<number>('PitwallSettingsSaveDelay', { factory: () => 500 });

/**
 * Where edits not yet confirmed by the server are written down, so a window that goes without waiting (the app quits, the
 * webview is torn down) loses none: the next Settings page sends them, each only if the server still has the value it was
 * made over (compare-and-set; otherwise it is shown as unsent, see `UnsentEdit`). The app's own origin storage, shared by its
 * windows.
 */
export const SETTINGS_JOURNAL = new InjectionToken<() => Storage | null>('PitwallSettingsJournal', {
  factory: () => () => globalThis.localStorage ?? null,
});
export const SETTINGS_JOURNAL_KEY = 'pw.settings.unsent';
/** An unsent edit older than this is not sent any more (the user has long seen the window without it). */
export const SETTINGS_JOURNAL_TTL_MS = 10 * 60_000;

/** A setting as the user sees it: an id for the edit (one per control, or per pair of controls on one value) and its name. */
export interface SettingField {
  readonly id: string;
  /** The setting's name as a sentence starts it: "Port", "Largest request". */
  readonly label: string;
}

/** An edit of one setting: the values it changes in one section, over `base`, the server's values of those keys when it was made. */
interface Edit {
  readonly field: SettingField;
  readonly section: Section;
  readonly values: Readonly<Record<string, unknown>>;
  readonly base: Readonly<Record<string, unknown>>;
}

/**
 * An edit a window left unsent whose setting changed on the server since (another window, the tray, a preset): it is not sent
 * over the newer value, but shown with "Apply it anyway" and "Discard".
 */
export interface UnsentEdit {
  readonly field: SettingField;
  /** The value it would set, as the page shows it ("12000", "on"). */
  readonly value: string;
}

/** "Port was changed elsewhere before your edit was sent. Your value: 12000." */
export function unsentText(u: UnsentEdit): string {
  return `${u.field.label} was changed elsewhere before your edit was sent. Your value: ${u.value}.`;
}

const MIB = 1024 ** 2;

/** The words of the settings' choices, as the page's menus show them. */
const CHOICE_TEXT: Readonly<Record<string, string>> = {
  keepServing: 'Keep serving',
  stopServing: 'Stop serving',
  ask: 'Ask each time',
  disk: 'On disk',
  memory: 'Memory only',
};

/** One setting's value as the page shows it (`key` is the setting's): 50 MB, 5 min, 30 days, on, Keep serving. */
export function shownValue(key: string, v: unknown): string {
  if (typeof v === 'boolean') return v ? 'on' : 'off';
  if (v === null) return key === 'hotCapBytes' ? 'what fits' : 'automatic';
  if (typeof v === 'string') return CHOICE_TEXT[v] ?? v;
  if (typeof v !== 'number') return JSON.stringify(v) ?? '';
  if (key.endsWith('Bytes')) return bytes(v);
  if (key.endsWith('Mib')) return bytes(v * MIB);
  if (key.endsWith('Secs')) return seconds(v);
  if (key.endsWith('Days')) return `${v} ${v === 1 ? 'day' : 'days'}`;
  if (key.endsWith('Percent')) return `${v}%`;
  return String(v);
}

/** A setting the server refused: its edit stays in the form, with the server's reason, until it is edited, retried or undone. */
export interface Refusal {
  readonly field: SettingField;
  readonly error: CommandError;
}

/** "Port could not be saved: the port is in use." */
export function refusalText(r: Refusal): string {
  return `${r.field.label} could not be saved: ${r.error.message}`;
}

/** "Undo port change". */
export function undoText(r: Refusal): string {
  return `Undo ${lowerFirst(r.field.label)} change`;
}

/** The first letter in lower case, unless the word is an abbreviation ("LAN port" stays). */
function lowerFirst(text: string): string {
  return /^[A-Z][A-Z]/.test(text) ? text : text.charAt(0).toLowerCase() + text.slice(1);
}

interface SettingsState {
  /** What the server has. */
  saved: Settings | null;
  /** The form: `saved` with the edits not confirmed yet on top (waiting, on the way, or refused). */
  draft: Settings | null;
  loading: boolean;
  /** A save or a preset is queued or on the way. */
  saving: boolean;
  error: CommandError | null;
  /** The settings the server refused, in the order refused; each is shown with its reason, Undo and Retry. */
  refusals: readonly Refusal[];
  /** Edits a window left unsent whose setting changed on the server since; each is shown with "Apply it anyway" and "Discard". */
  unsent: readonly UnsentEdit[];
  /** The caches' sizes and hit counts (ADR 0019); null until loaded. */
  cacheInfo: CacheInfo | null;
  cacheError: CommandError | null;
  /** A cache clear is running. */
  clearing: boolean;
}

const initial: SettingsState = { saved: null, draft: null, loading: false, saving: false, error: null, refusals: [], unsent: [], cacheInfo: null, cacheError: null, clearing: false };

/** Applies an edit to one section; any memory edit other than the preset itself makes the preset `custom`. */
export function editSettings<S extends Section>(s: Settings, section: S, values: Partial<Settings[S]>): Settings {
  const next = { ...s, [section]: { ...s[section], ...values } } as Settings;
  if (section === 'memory' && !('preset' in values)) next.memory = { ...next.memory, preset: 'custom' };
  return next;
}

/**
 * Settings: the saved values and the form's draft. Each setting saves by itself: a switch or a choice at once, a typed value
 * after a short pause (or at once on Enter, leaving the field, or the window closing). Every save and preset runs through one
 * chain, in the order made, and each save sends one setting over what the server has, so a refusal names its setting and a
 * refused setting never blocks the others. The server validates and returns what it kept.
 */
export const SettingsStore = signalStore(
  withState(initial),
  withComputed(({ saved, draft }) => ({
    dirty: computed(() => {
      const s = saved();
      const d = draft();
      return !!s && !!d && JSON.stringify(s) !== JSON.stringify(d);
    }),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts), delay = inject(SETTINGS_SAVE_DELAY), journalStorage = inject(SETTINGS_JOURNAL)) => {
    /** Edits not sent yet, by field id, in the order made. */
    const pending = new Map<string, Edit>();
    /** Refused edits, kept in the form and never sent again unless edited or retried. */
    const refused = new Map<string, Edit>();
    /** Typed values waiting for their pause. */
    const timers = new Map<string, ReturnType<typeof setTimeout>>();
    /** Unsent edits whose setting changed on the server since, by field id (`unsent` shows them). */
    const conflicts = new Map<string, Edit>();
    /** The edit on its way to the server. */
    let sending: Edit | null = null;
    /** The one chain every save and preset runs through. */
    let chain: Promise<unknown> = Promise.resolve();
    let queued = 0;

    const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
    const apply = (base: Settings, e: Edit) => editSettings(base, e.section, e.values as Partial<Settings[Section]>);

    /** The draft again: what the server has, then the refused edits, the one on its way and the waiting ones. */
    function redraw(): void {
      const s = store.saved();
      if (!s) return;
      let d: Settings = structuredClone(s);
      for (const e of [...refused.values(), ...(sending ? [sending] : []), ...pending.values()]) d = apply(d, e);
      patchState(store, { draft: d });
    }

    /** The shown refusals follow `refused`. */
    function syncRefusals(): void {
      patchState(store, { refusals: store.refusals().filter((r) => refused.has(r.field.id)) });
    }

    /** The shown unsent edits follow `conflicts`. */
    function syncUnsent(): void {
      patchState(store, { unsent: [...conflicts.values()].map((e) => ({ field: e.field, value: Object.entries(e.values).map(([k, v]) => shownValue(k, v)).join(', ') })) });
    }

    /** Writes down every edit the server has not confirmed (waiting or on its way). */
    function journal(): void {
      const unsent = [...(sending ? [sending] : []), ...pending.values()];
      try {
        const storage = journalStorage();
        if (!storage) return;
        if (unsent.length === 0) storage.removeItem(SETTINGS_JOURNAL_KEY);
        else storage.setItem(SETTINGS_JOURNAL_KEY, JSON.stringify({ at: Date.now(), edits: unsent }));
      } catch {
        // Storage refused: the close hook and the page's flush still save.
      }
    }

    /** The values of `keys` in `section` as the server has them now. */
    function serverValues(saved: Settings, section: Section, keys: Iterable<string>): Record<string, unknown> {
      const current = saved[section] as unknown as Record<string, unknown>;
      return Object.fromEntries([...keys].map((k) => [k, current[k]]));
    }

    const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

    /** The edits a window left unsent, once (the record is removed): fresh, well-formed, for a known section, with a base. */
    function takeJournal(saved: Settings): Edit[] {
      try {
        const storage = journalStorage();
        const raw = storage?.getItem(SETTINGS_JOURNAL_KEY);
        if (!storage || !raw) return [];
        storage.removeItem(SETTINGS_JOURNAL_KEY);
        const parsed = JSON.parse(raw) as { at?: unknown; edits?: unknown } | null;
        if (typeof parsed?.at !== 'number' || Math.abs(Date.now() - parsed.at) > SETTINGS_JOURNAL_TTL_MS || !Array.isArray(parsed.edits)) return [];
        return (parsed.edits as Partial<Edit>[]).filter(
          (e): e is Edit =>
            typeof e?.field?.id === 'string' &&
            typeof e.field.label === 'string' &&
            typeof e.section === 'string' &&
            Object.hasOwn(saved, e.section) &&
            isRecord(e.values) &&
            isRecord(e.base),
        );
      } catch {
        return [];
      }
    }

    /** Runs `task` after everything queued before it; `saving` is true while anything is queued. */
    function enqueue<T>(task: () => Promise<T>): Promise<T> {
      queued++;
      patchState(store, { saving: true });
      const run = chain.then(task);
      chain = run.then(
        () => undefined,
        () => undefined,
      );
      return run.finally(() => {
        if (--queued === 0) patchState(store, { saving: false });
      });
    }

    /** Sends the waiting edit of `id` over what the server has; true when it was kept (or nothing waited). */
    function sendField(id: string): Promise<boolean> {
      return enqueue(async () => {
        const e = pending.get(id);
        const base = store.saved();
        if (!e || !base) return !refused.has(id);
        pending.delete(id);
        sending = e;
        const body = apply(base, e);
        try {
          if (same(body, base)) return true;
          const kept = await backend.saveSettings(body);
          patchState(store, { saved: kept });
          return true;
        } catch (err) {
          // A newer edit of the same setting waits: it is sent next, and this refusal is moot.
          if (!pending.has(id)) {
            refused.set(id, e);
            patchState(store, { refusals: [...store.refusals().filter((r) => r.field.id !== id), { field: e.field, error: toCommandError(err) }] });
          }
          return false;
        } finally {
          sending = null;
          journal();
          redraw();
        }
      });
    }

    function stopTimer(id: string): void {
      const t = timers.get(id);
      if (t !== undefined) clearTimeout(t);
      timers.delete(id);
    }

    /** Queues every waiting edit now, in the order made. */
    function sendAllWaiting(): Promise<boolean>[] {
      for (const id of [...timers.keys()]) stopTimer(id);
      return [...pending.keys()].map((id) => sendField(id));
    }

    async function flush(): Promise<boolean> {
      sendAllWaiting();
      await enqueue(async () => undefined);
      return refused.size === 0;
    }

    function edit<S extends Section>(field: SettingField, section: S, values: Partial<Settings[S]>, how: 'now' | 'typed' = 'now'): void {
      if (!store.saved()) return;
      if (refused.delete(field.id)) syncRefusals();
      // A new edit of a setting shown as unsent replaces it.
      if (conflicts.delete(field.id)) syncUnsent();
      const prev = pending.get(field.id);
      pending.delete(field.id);
      const before = prev?.section === section ? prev : null;
      // The base is the server's value when the edit was first made: a key the edit already held keeps its base.
      const base = { ...serverValues(store.saved()!, section, Object.keys(values)), ...before?.base };
      pending.set(field.id, { field, section, values: { ...before?.values, ...values }, base });
      journal();
      // A typed value is in its field already: drawing the form again mid-word would rewrite it ("2.5" shown as "3").
      if (how === 'now') redraw();
      stopTimer(field.id);
      if (how === 'now') {
        void sendField(field.id);
        return;
      }
      timers.set(
        field.id,
        setTimeout(() => {
          timers.delete(field.id);
          void sendField(field.id);
        }, delay),
      );
    }

    return {
      async load(): Promise<void> {
        patchState(store, { loading: true, error: null });
        try {
          const settings = await backend.settings();
          patchState(store, { saved: settings, loading: false });
          redraw();
          // Edits a window left unsent (it went without waiting) are sent now, in the order they were made, each only if the
          // server still has the value it was made over; one made over a value changed since is shown instead, never sent.
          for (const e of takeJournal(settings)) {
            const keys = Object.keys(e.values);
            if (same(serverValues(settings, e.section, keys), Object.fromEntries(keys.map((k) => [k, e.base[k]])))) {
              edit(e.field, e.section, e.values as Partial<Settings[Section]>, 'now');
            } else {
              conflicts.set(e.field.id, e);
            }
          }
          syncUnsent();
        } catch (e) {
          patchState(store, { loading: false, error: toCommandError(e) });
        }
      },

      /** Reads the caches' sizes and hit counts; a failure shows inline and keeps the last good numbers. */
      async loadCacheInfo(): Promise<void> {
        try {
          patchState(store, { cacheInfo: await backend.cacheInfo(), cacheError: null });
        } catch (e) {
          patchState(store, { cacheError: toCommandError(e) });
        }
      },

      /** Empties caches (a command, applied at once and not part of the draft) and refreshes the numbers. */
      async clearCache(scope: CacheClearScope): Promise<number | undefined> {
        if (store.clearing()) return undefined;
        patchState(store, { clearing: true });
        try {
          const removed = await backend.clearCache(scope);
          toasts.ok(removed > 0 ? `Removed ${removed} cached ${removed === 1 ? 'entry' : 'entries'}.` : 'Nothing to remove: the cache was already empty.');
          patchState(store, { clearing: false });
          patchState(store, { cacheInfo: await backend.cacheInfo(), cacheError: null });
          return removed;
        } catch (e) {
          patchState(store, { clearing: false });
          toasts.error(toCommandError(e));
          return undefined;
        }
      },

      /**
       * Changes one setting in the form and saves it: `now` for a switch or a choice (and a typed value committed with Enter or
       * by leaving the field), `typed` after the pause (a newer keystroke restarts it).
       */
      edit,

      /** Puts a refused or waiting setting back to what the server has (`id`), or every one. */
      revert(id?: string): void {
        const ids = id === undefined ? [...new Set([...refused.keys(), ...pending.keys()])] : [id];
        for (const i of ids) {
          stopTimer(i);
          pending.delete(i);
          refused.delete(i);
        }
        syncRefusals();
        journal();
        redraw();
      },

      /** Sends an unsent edit over the server's newer value (the user chose "Apply it anyway"). */
      applyUnsent(id: string): void {
        const e = conflicts.get(id);
        if (!e) return;
        conflicts.delete(id);
        syncUnsent();
        edit(e.field, e.section, e.values as Partial<Settings[Section]>, 'now');
      },

      /** Forgets an unsent edit: the server's value stays. */
      discardUnsent(id: string): void {
        if (conflicts.delete(id)) syncUnsent();
      },

      /** Sends a refused setting again (`id`), or every one; true when all were kept. */
      async retry(id?: string): Promise<boolean> {
        const ids = id === undefined ? [...refused.keys()] : refused.has(id) ? [id] : [];
        for (const i of ids) {
          pending.set(i, refused.get(i)!);
          refused.delete(i);
        }
        syncRefusals();
        journal();
        const results = await Promise.all(ids.map((i) => sendField(i)));
        return results.every(Boolean);
      },

      /** Saves every waiting edit now (Enter, the page or window going); true when nothing is left refused. */
      flush,

      /** What the Settings window's close hook hears: wait for a save, say what was refused and why, or close. */
      closeVerdict(): SettingsCloseVerdict {
        if (timers.size > 0 || pending.size > 0 || queued > 0) return { kind: 'wait', done: flush() };
        const refusals = store.refusals();
        if (refusals.length === 0) return { kind: 'close' };
        return { kind: 'warn', message: refusals.map((r) => `${r.field.label} was not saved: ${r.error.message}`).join(' ') };
      },

      /**
       * Applies a memory preset on the server after the edits made before it, in the same chain (a preset chosen while a save is
       * on its way is queued, never dropped), and shows its values. A refused memory edit gives way to the preset.
       */
      applyPreset(preset: Exclude<Preset, 'custom'>): Promise<void> {
        sendAllWaiting();
        return enqueue(async () => {
          try {
            const settings = await backend.applyPreset(preset);
            for (const [id, e] of [...refused]) if (e.section === 'memory') refused.delete(id);
            syncRefusals();
            patchState(store, { saved: settings });
            redraw();
            toasts.ok(`${PRESET_LABEL[preset]} preset applied.`);
          } catch (e) {
            toasts.error(toCommandError(e));
          }
        });
      },
    };
  }),
);

export type SettingsStore = InstanceType<typeof SettingsStore>;
