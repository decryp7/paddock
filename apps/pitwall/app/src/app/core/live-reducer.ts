import type { Download, HistoryPoint, LiveUpdate, MemorySample, Overview, PitwallEvent, Throughput } from './api.types';

/** Most recent events kept for the feed. */
export const MAX_EVENTS = 100;
/** Live tail for the charts: at most one point per {@link TAIL_STEP_MS}, at most {@link MAX_TAIL} points. */
export const TAIL_STEP_MS = 1000;
export const MAX_TAIL = 3600;

export interface LiveState {
  overview: Overview | null;
  events: PitwallEvent[];
  downloads: Download[];
  /** Points built from pushed samples, appended to the 1 h chart without asking the server again. */
  tail: HistoryPoint[];
  updates: number;
}

export const INITIAL_LIVE: LiveState = { overview: null, events: [], downloads: [], tail: [], updates: 0 };

/** Folds one pushed update into the state (pure; returns the same object when nothing changed). */
export function applyUpdate(state: LiveState, u: LiveUpdate): LiveState {
  const o = state.overview;
  const next = { ...state, updates: state.updates + 1 };
  switch (u.kind) {
    case 'memory':
      return { ...next, overview: o && { ...o, memory: u.sample }, tail: appendTail(state.tail, u.sample, o?.throughput ?? null) };
    case 'models':
      return { ...next, overview: o && { ...o, models: u.models } };
    case 'queue':
      return { ...next, overview: o && { ...o, queue: u.queue } };
    case 'throughput':
      return { ...next, overview: o && { ...o, throughput: u.throughput } };
    case 'pairing':
      return { ...next, overview: o && { ...o, pairings: u.pairings } };
    case 'server':
      return { ...next, overview: o && { ...o, server: u.server } };
    case 'event':
      if (state.events.some((e) => e.id === u.event.id)) return state;
      return { ...next, events: [u.event, ...state.events].slice(0, MAX_EVENTS) };
    case 'download':
      return { ...next, downloads: upsertDownload(state.downloads, u.download) };
  }
}

export function upsertDownload(list: Download[], d: Download): Download[] {
  const i = list.findIndex((x) => x.id === d.id);
  if (i < 0) return [d, ...list];
  const copy = list.slice();
  copy[i] = d;
  return copy;
}

/** Appends a chart point for a memory sample, throttled to one per second and bounded. */
export function appendTail(tail: HistoryPoint[], m: MemorySample, t: Throughput | null): HistoryPoint[] {
  const last = tail[tail.length - 1];
  if (last && m.at - last.at < TAIL_STEP_MS) return tail;
  const gpu = m.gpus[0];
  const point: HistoryPoint = {
    at: m.at,
    ramUsed: m.ramTotal - m.ramAvailable,
    ramTotal: m.ramTotal,
    vramUsed: gpu?.vramUsed ?? null,
    vramTotal: gpu?.vramTotal ?? null,
    pitwallRss: m.pitwallRss,
    cpuPercent: m.cpuPercent,
    promptTps: t?.promptTps ?? 0,
    genTps: t?.genTps ?? 0,
    p50Ms: t ? t.ttftP50Ms : null,
    p95Ms: null,
    requests: 0,
  };
  const out = tail.length >= MAX_TAIL ? tail.slice(tail.length - MAX_TAIL + 1) : tail.slice();
  out.push(point);
  return out;
}
