import { inject, Injectable, type EnvironmentProviders, type Provider } from '@angular/core';
import { MemoryPitwallBackend, providePitwall, type AiState } from '@decryp7/paddock-ui/shell/pitwall';

const NEEDS = { chat: true, vision: false, tools: false, json: false, embeddings: false, contextTokens: 8192, fast: false } as const;

/** The states the demo can play (`?pitwall=<name>`); Pitwall is not installed by default. */
export const DEMO_PITWALL_STATES: Readonly<Record<string, AiState>> = {
  notInstalled: { kind: 'notInstalled' },
  notRunning: { kind: 'notRunning' },
  needsPairing: { kind: 'needsPairing' },
  paused: { kind: 'paused', stopped: false },
  noModel: { kind: 'noSuitableModel', needs: NEEDS },
  incompatible: { kind: 'incompatible', message: 'Pitwall 2.0.0 speaks a newer connection version (2) than this Shell Demo (1). Update Shell Demo to use it.', updateApp: true },
  ready: { kind: 'ready', model: 'qwen3-8b-q4km', vision: false, contextTokens: 8192 },
};

/** The first state from the page's address (`?pitwall=needsPairing`); `?builtin=0` plays an app without an engine of its own. */
export function demoPitwallOptions(search: string = globalThis.location?.search ?? '') {
  const q = new URLSearchParams(search);
  return {
    app: 'Shell Demo',
    state: DEMO_PITWALL_STATES[q.get('pitwall') ?? ''] ?? DEMO_PITWALL_STATES['notInstalled'],
    builtInAvailable: q.get('builtin') !== '0',
  };
}

/** The demo's engine: one in-memory Pitwall side per visit of the page (the real one is the app's Rust `Link`). */
@Injectable()
export class DemoPitwall {
  readonly engine = new MemoryPitwallBackend(demoPitwallOptions());
}

/** The Local AI page's own providers (rule 32): the engine and the shared store, created when the page opens. */
export const PITWALL_PROVIDERS: readonly (Provider | EnvironmentProviders)[] = [DemoPitwall, providePitwall({ backend: () => inject(DemoPitwall).engine })];
