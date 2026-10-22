import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, McpServerInfo, PluginInfo } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';

interface PluginsState {
  plugins: PluginInfo[];
  mcp: McpServerInfo[];
  loading: boolean;
  loaded: boolean;
  pluginsError: CommandError | null;
  mcpError: CommandError | null;
  /** Plugin ids with a toggle in flight. */
  busy: Record<string, true>;
}

const initial: PluginsState = { plugins: [], mcp: [], loading: false, loaded: false, pluginsError: null, mcpError: null, busy: {} };

/** Plugins & tools: out-of-process plugins and MCP servers. Each list fails on its own (one error doesn't hide the other). */
export const PluginsStore = signalStore(
  withState(initial),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => ({
    isBusy(id: string): boolean {
      return id in store.busy();
    },

    async load(): Promise<void> {
      patchState(store, { loading: true, pluginsError: null, mcpError: null });
      const [plugins, mcp] = await Promise.allSettled([backend.plugins(), backend.mcpServers()]);
      patchState(store, {
        loading: false,
        loaded: true,
        ...(plugins.status === 'fulfilled' ? { plugins: plugins.value } : { pluginsError: toCommandError(plugins.reason) }),
        ...(mcp.status === 'fulfilled' ? { mcp: mcp.value } : { mcpError: toCommandError(mcp.reason) }),
      });
    },

    /** Enables or disables a plugin; the list shows the new state at once and rolls back if the server refuses. */
    async setEnabled(id: string, enabled: boolean): Promise<void> {
      if (id in store.busy()) return;
      const before = store.plugins();
      patchState(store, { busy: { ...store.busy(), [id]: true }, plugins: before.map((p) => (p.id === id ? { ...p, enabled } : p)) });
      try {
        await backend.setPluginEnabled(id, enabled);
        const name = before.find((p) => p.id === id)?.name ?? id;
        toasts.ok(`${name} ${enabled ? 'enabled' : 'disabled'}.`);
        try {
          patchState(store, { plugins: await backend.plugins() });
        } catch {
          // The toggle succeeded; the refreshed state arrives with the next load.
        }
      } catch (e) {
        patchState(store, { plugins: before });
        toasts.error(toCommandError(e));
      } finally {
        const busy = { ...store.busy() };
        delete busy[id];
        patchState(store, { busy });
      }
    },
  })),
);

export type PluginsStore = InstanceType<typeof PluginsStore>;
