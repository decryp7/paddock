import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { AuditRow, McpSettings, McpView } from '../generated/mcp';
import { McpBackend, type Unlisten } from './backend';
import { mcpMessage } from './wire';

/** Where the last save, revoke or clear is: one union, never separate flags. */
export type McpSaveStatus = 'idle' | 'saving' | { readonly error: string };

/** Where the activity log is: it loads on its own, so a failed log never hides the settings. */
export type McpAuditStatus = 'loading' | 'loaded' | { readonly error: string };

interface McpState {
  /** What the engine last said; null until the first answer. */
  view: McpView | null;
  saveStatus: McpSaveStatus;
  audit: readonly AuditRow[];
  auditStatus: McpAuditStatus;
}

/**
 * The AI access (MCP) settings as one window sees them (rule 34: the engine is the single source of truth). The store holds the
 * view the engine reported, saves every change as a whole (the engine validates and applies it at once), revokes keys and keeps
 * the activity log, which it reloads when the engine says a call was recorded. A refusal is kept as the engine worded it. Keys
 * never pass through here: the key dialog's window asks the backend itself and shows the key once.
 */
export const McpStore = signalStore(
  withState<McpState>({ view: null, saveStatus: 'idle', audit: [], auditStatus: 'loading' }),
  withRequestStatus(),
  withComputed(({ view, saveStatus, auditStatus }) => ({
    settings: computed(() => view()?.settings ?? null),
    readOnlyTools: computed(() => (view()?.tools ?? []).filter((t) => !t.action)),
    actionTools: computed(() => (view()?.tools ?? []).filter((t) => t.action)),
    keys: computed(() => view()?.keys ?? []),
    maxKeys: computed(() => view()?.maxKeys ?? 0),
    atKeyLimit: computed(() => {
      const v = view();
      return v !== null && v.keys.filter((k) => !k.revoked).length >= v.maxKeys;
    }),
    saving: computed(() => saveStatus() === 'saving'),
    saveError: computed(() => {
      const s = saveStatus();
      return typeof s === 'object' ? s.error : null;
    }),
    auditError: computed(() => {
      const s = auditStatus();
      return typeof s === 'object' ? s.error : null;
    }),
    auditLoaded: computed(() => auditStatus() === 'loaded'),
  })),
  withMethods((store, backend = inject(McpBackend)) => {
    const change = async (next: (s: McpSettings) => McpSettings): Promise<boolean> => {
      const current = store.settings();
      if (!current) return false;
      patchState(store, { saveStatus: 'saving' });
      try {
        patchState(store, { view: await backend.save(next(current)), saveStatus: 'idle' }, setLoaded());
        return true;
      } catch (e) {
        patchState(store, { saveStatus: { error: mcpMessage(e, $localize`:@@pd.mcp.save-failed:The change could not be saved.`) } });
        return false;
      }
    };
    const loadAudit = async (): Promise<void> => {
      try {
        patchState(store, { audit: await backend.audit(), auditStatus: 'loaded' });
      } catch (e) {
        // A failed reload keeps the rows on screen but says so; the first failure replaces the skeleton with the error.
        patchState(store, { auditStatus: { error: mcpMessage(e, $localize`:@@pd.mcp.audit-failed:The activity log could not be read.`) } });
      }
    };
    return {
      /** Asks the engine for the settings and the activity log (also the Retry of a failed ask). */
      async load(): Promise<void> {
        // A refresh keeps the page on screen; only the first ask shows as loading.
        if (!store.view()) patchState(store, setLoading());
        try {
          patchState(store, { view: await backend.get() }, setLoaded());
        } catch (e) {
          patchState(store, setError(mcpMessage(e, $localize`:@@pd.mcp.load-failed:The AI access settings could not be read.`)));
        }
        await loadAudit();
      },
      /** Re-reads the view quietly (after the key dialog issued a key in its own window). */
      async refresh(): Promise<void> {
        try {
          patchState(store, { view: await backend.get() });
        } catch {
          // The view on screen stays; a failed refresh is not worth hiding it.
        }
      },
      /** Re-reads the activity log (also the Retry of a failed read). */
      loadAudit,
      /** The master switch. Resolves true when the engine stored it; otherwise `saveError` says why. */
      setEnabled: (enabled: boolean): Promise<boolean> => change((s) => ({ ...s, enabled })),
      /** Whether resources are exposed. */
      setResources: (resources: boolean): Promise<boolean> => change((s) => ({ ...s, resources })),
      /** Switches a read-only tool on or off. */
      setToolOn(name: string, on: boolean): Promise<boolean> {
        return change((s) => ({ ...s, disabledTools: on ? s.disabledTools.filter((n) => n !== name) : [...new Set([...s.disabledTools, name])] }));
      },
      /** Allows or forbids an action tool (off by default). */
      setActionAllowed(name: string, allowed: boolean): Promise<boolean> {
        return change((s) => ({ ...s, allowedActions: allowed ? [...new Set([...s.allowedActions, name])] : s.allowedActions.filter((n) => n !== name) }));
      },
      /** Revokes a client's key. */
      async revoke(id: number): Promise<boolean> {
        patchState(store, { saveStatus: 'saving' });
        try {
          patchState(store, { view: await backend.revokeKey(id), saveStatus: 'idle' });
          return true;
        } catch (e) {
          patchState(store, { saveStatus: { error: mcpMessage(e, $localize`:@@pd.mcp.revoke-failed:The key could not be revoked.`) } });
          return false;
        }
      },
      /** Empties the activity log. */
      async clearAudit(): Promise<boolean> {
        try {
          await backend.clearAudit();
        } catch (e) {
          patchState(store, { saveStatus: { error: mcpMessage(e, $localize`:@@pd.mcp.clear-failed:The activity log could not be cleared.`) } });
          return false;
        }
        await loadAudit();
        return true;
      },
      /** Hides the last failure. */
      dismissSaveError(): void {
        patchState(store, { saveStatus: 'idle' });
      },
      /** An event from the engine: the log changed, read it again. */
      _event(): void {
        void loadAudit();
      },
    };
  }),
  withHooks((store, backend = inject(McpBackend)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then ask: no call recorded in between is missed.
        void backend
          .listen(() => store._event())
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-mcp] cannot listen for activity: ${errorMessage(e, 'unknown error')}`));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type McpStoreType = InstanceType<typeof McpStore>;
