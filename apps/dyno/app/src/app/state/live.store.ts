import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import type { AgentStatus, LiveUpdate } from '../core/api.types';
import { BACKEND, type Unsubscribe } from '../core/backend';

/** How many live samples the sparklines keep (one a second: the last minute). */
export const SPARK_CAPACITY = 60;

/** The recent values of each live metric, oldest first. */
export interface Series {
  cpu: number[];
  memory: number[];
  diskRead: number[];
  diskWrite: number[];
  netRx: number[];
  netTx: number[];
  gpu: number[];
}

interface LiveState {
  update: LiveUpdate | null;
  series: Series;
  agent: AgentStatus;
}

const emptySeries = (): Series => ({ cpu: [], memory: [], diskRead: [], diskWrite: [], netRx: [], netTx: [], gpu: [] });

/** Appends `value` to a copy of `list`, keeping the last {@link SPARK_CAPACITY}. */
export function push(list: readonly number[], value: number): number[] {
  const next = list.length >= SPARK_CAPACITY ? list.slice(list.length - SPARK_CAPACITY + 1) : list.slice();
  next.push(value);
  return next;
}

/** Folds one update into the sparkline series (pure: the store and the tests share it). */
export function appendSeries(s: Series, u: LiveUpdate): Series {
  const sys = u.system;
  const gpu = 'available' in sys.gpu ? sys.gpu.available.utilization : 0;
  return {
    cpu: push(s.cpu, sys.cpu.total),
    memory: push(s.memory, sys.memory.total > 0 ? (sys.memory.used / sys.memory.total) * 100 : 0),
    diskRead: push(s.diskRead, sys.diskIo.readBps),
    diskWrite: push(s.diskWrite, sys.diskIo.writeBps),
    netRx: push(s.netRx, sys.network.reduce((a, n) => a + n.rxBps, 0)),
    netTx: push(s.netTx, sys.network.reduce((a, n) => a + n.txBps, 0)),
    gpu: push(s.gpu, gpu),
  };
}

/**
 * The live summary every screen shares (dashboard tiles, the shell's overhead and agent indicator, the tray):
 * subscribed once for the app's lifetime, with a minute of history for the sparklines.
 */
export const LiveStore = signalStore(
  { providedIn: 'root' },
  withState<LiveState>({ update: null, series: emptySeries(), agent: { state: 'starting', message: null } }),
  withComputed(({ update }) => ({
    system: computed(() => update()?.system ?? null),
    connected: computed(() => update() !== null),
    overhead: computed(() => update()?.overhead ?? null),
  })),
  withMethods((store) => ({
    /** Applies one pushed update. */
    receive(u: LiveUpdate): void {
      patchState(store, (s) => ({ update: u, series: appendSeries(s.series, u) }));
    },
    setAgent(agent: AgentStatus): void {
      patchState(store, { agent });
    },
  })),
  withHooks((store) => {
    const backend = inject(BACKEND);
    const stops: Unsubscribe[] = [];
    return {
      onInit() {
        stops.push(backend.onLive('summary', (u) => store.receive(u)));
        stops.push(backend.onAgentStatus((a) => store.setAgent(a)));
      },
      onDestroy() {
        for (const stop of stops) stop();
      },
    };
  }),
);
