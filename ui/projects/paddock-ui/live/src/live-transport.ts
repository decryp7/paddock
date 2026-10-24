import { InjectionToken } from '@angular/core';
import type { LiveCommand } from '../generated/live-commands';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * How a window reaches a feed: the events Rust pushes and the snapshot command. Tauri in the desktop app
 * ({@link tauriLiveTransport}), {@link MemoryLiveTransport} in the browser build and in specs. Payloads are untrusted here;
 * the store parses them (`parseFrame`).
 */
export interface LiveTransport {
  /** Calls `onFrame` with every payload pushed on `channel` until the returned function is called. */
  listen(channel: string, onFrame: (payload: unknown) => void): Promise<Unlisten>;
  /** The whole state of `channel` as a snapshot frame (`live_snapshot`). */
  snapshot(channel: string): Promise<unknown>;
}

/** The transport of the window's live stores. */
export const LIVE_TRANSPORT = new InjectionToken<LiveTransport>('pd.live.transport');

/**
 * The app's IPC as the transport needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and an
 * adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface LiveIpc {
  invoke<T>(command: LiveCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/** The event frames of `channel` are pushed on (`paddock_live::Channel::event`). */
export function liveEvent(channel: string): string {
  return `paddock://live/${channel}`;
}

/** The transport over Tauri. */
export function tauriLiveTransport(ipc: LiveIpc): LiveTransport {
  return {
    listen: (channel, onFrame) => ipc.listen(liveEvent(channel), onFrame),
    snapshot: (channel) => ipc.invoke<unknown>('live_snapshot', { channel }),
  };
}

/**
 * A transport held in memory, for the browser build, demos and specs: `push` plays Rust's part (a payload reaches every
 * listener of the channel), `setSnapshot` is what `live_snapshot` answers (a function, so a spec can fail it or hold it).
 */
export class MemoryLiveTransport implements LiveTransport {
  private readonly listeners = new Map<string, Set<(payload: unknown) => void>>();
  private readonly snapshots = new Map<string, () => Promise<unknown>>();
  /** Every channel a snapshot was asked for, in order. */
  readonly snapshotCalls: string[] = [];

  /** Sends `payload` to the listeners of `channel`. */
  push(channel: string, payload: unknown): void {
    for (const l of [...(this.listeners.get(channel) ?? [])]) l(payload);
  }

  /** How many listeners `channel` has. */
  listenerCount(channel: string): number {
    return this.listeners.get(channel)?.size ?? 0;
  }

  /** What the snapshot command answers for `channel`. */
  setSnapshot(channel: string, answer: () => Promise<unknown>): void {
    this.snapshots.set(channel, answer);
  }

  listen(channel: string, onFrame: (payload: unknown) => void): Promise<Unlisten> {
    const set = this.listeners.get(channel) ?? new Set();
    set.add(onFrame);
    this.listeners.set(channel, set);
    return Promise.resolve(() => void set.delete(onFrame));
  }

  snapshot(channel: string): Promise<unknown> {
    this.snapshotCalls.push(channel);
    const answer = this.snapshots.get(channel);
    return answer ? answer() : Promise.reject(new Error('The live feed is not set up in this app'));
  }
}
