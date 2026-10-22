import type {
  Availability,
  Capability,
  DownloadState,
  EventKind,
  FitVerdict,
  HotState,
  JobState,
  LimitKind,
  LimitState,
  MemoryPressure,
  Preset,
  Priority,
  ProcessHealth,
  PresetStatus,
  ReleaseReason,
  SelfTest,
  BuildCapability,
  Severity,
} from '../core/api.types';

/** The visual tone of a status pill (always shown with its text, never colour alone). */
export type Tone = 'ok' | 'warn' | 'bad' | 'info' | 'muted';

export interface Look {
  label: string;
  tone: Tone;
}

export const PRESET_LABEL: Record<Preset, string> = {
  lowMemory: 'Low memory',
  balanced: 'Balanced',
  performance: 'Performance',
  custom: 'Custom',
};

export const PRESET_HINT: Record<Exclude<Preset, 'custom'>, string> = {
  lowMemory: 'Release fast, one model at a time',
  balanced: 'Sensible idle times, two hot models',
  performance: 'Keep models hot, use what fits',
};

export const PRIORITY_LABEL: Record<Priority, string> = {
  interactive: 'Interactive',
  background: 'Background',
  batch: 'Batch',
};

export const REASON_LABEL: Record<ReleaseReason, string> = {
  idle: 'idle',
  deepIdle: 'deep idle',
  pressure: 'memory pressure',
  yield: 'yield to foreground',
  manual: 'manual',
  shutdown: 'shutdown',
  failed: 'failure',
  evicted: 'made room',
};

export const CAPABILITY_LABEL: Record<Capability, string> = {
  chat: 'Chat',
  vision: 'Vision',
  tools: 'Tools',
  json: 'JSON',
  embeddings: 'Embeddings',
  rerank: 'Rerank',
  transcription: 'Transcription',
};

/**
 * The tone of a usage meter: neutral until it matters (calm by default), warning above 85 %, danger above 95 %.
 * Accent is never used for usage: it means action and selection.
 */
export function loadTone(percent: number): 'neutral' | 'warn' | 'bad' {
  return percent > 95 ? 'bad' : percent > 85 ? 'warn' : 'neutral';
}

export function pressureLook(p: MemoryPressure): Look {
  return { normal: { label: 'Normal', tone: 'ok' as Tone }, elevated: { label: 'Elevated', tone: 'warn' as Tone }, critical: { label: 'Critical', tone: 'bad' as Tone } }[p];
}

export function hotStateLook(s: HotState): Look {
  const map: Record<HotState, Look> = {
    loading: { label: 'Loading', tone: 'info' },
    ready: { label: 'Ready', tone: 'ok' },
    busy: { label: 'Busy', tone: 'info' },
    releasingContext: { label: 'Releasing context', tone: 'warn' },
    unloading: { label: 'Unloading', tone: 'warn' },
    failed: { label: 'Failed', tone: 'bad' },
  };
  return map[s];
}

export function availabilityLook(a: Availability): Look {
  const map: Record<Availability['state'], Look> = {
    available: { label: 'Available', tone: 'ok' },
    notInstalled: { label: 'Not installed', tone: 'muted' },
    notAvailable: { label: 'Not available', tone: 'muted' },
    failed: { label: 'Failed', tone: 'bad' },
  };
  return map[a.state];
}

export function healthLook(h: ProcessHealth): Look {
  const map: Record<ProcessHealth, Look> = {
    starting: { label: 'Starting', tone: 'info' },
    healthy: { label: 'Healthy', tone: 'ok' },
    unhealthy: { label: 'Unhealthy', tone: 'bad' },
    restarting: { label: 'Restarting', tone: 'warn' },
    stopped: { label: 'Stopped', tone: 'muted' },
  };
  return map[h];
}

export function selfTestLook(s: SelfTest): Look {
  const map: Record<SelfTest, Look> = {
    passed: { label: 'Self-test passed', tone: 'ok' },
    failed: { label: 'Self-test failed', tone: 'bad' },
    notRun: { label: 'Not tested', tone: 'muted' },
  };
  return map[s];
}

export function fitLook(v: FitVerdict): Look {
  const map: Record<FitVerdict, Look> = {
    fits: { label: 'Fits', tone: 'ok' },
    tight: { label: 'Tight', tone: 'warn' },
    wontFit: { label: "Won't fit", tone: 'bad' },
    unknown: { label: 'Unknown', tone: 'muted' },
  };
  return map[v];
}

export function downloadLook(s: DownloadState): Look {
  const map: Record<DownloadState, Look> = {
    queued: { label: 'Queued', tone: 'muted' },
    downloading: { label: 'Downloading', tone: 'info' },
    verifying: { label: 'Verifying', tone: 'info' },
    unpacking: { label: 'Unpacking', tone: 'info' },
    selfTest: { label: 'Self-test', tone: 'info' },
    done: { label: 'Done', tone: 'ok' },
    failed: { label: 'Failed', tone: 'bad' },
    paused: { label: 'Paused · resumable', tone: 'warn' },
    cancelled: { label: 'Cancelled', tone: 'muted' },
  };
  return map[s];
}

/** Whether a download is still moving (cancel makes sense). */
export function downloadActive(s: DownloadState): boolean {
  return s === 'queued' || s === 'downloading' || s === 'verifying' || s === 'unpacking' || s === 'selfTest';
}

export function jobStateLook(s: JobState): Look {
  const map: Record<JobState, Look> = {
    queued: { label: 'Waiting', tone: 'muted' },
    running: { label: 'Running', tone: 'info' },
    done: { label: 'Done', tone: 'ok' },
    failed: { label: 'Failed', tone: 'bad' },
    cancelled: { label: 'Cancelled', tone: 'muted' },
  };
  return map[s];
}

/** What each limit counts, and when it resets. */
export const LIMIT_LABEL: Record<LimitKind, { label: string; resets: string }> = {
  requestsPerMin: { label: 'Requests per minute', resets: 'counts the last minute' },
  requestsPerDay: { label: 'Requests per day', resets: 'resets at midnight UTC' },
  tokensPerDay: { label: 'Tokens per day', resets: 'resets at midnight UTC' },
  tokensPerMonth: { label: 'Tokens per month', resets: 'resets on the 1st, midnight UTC' },
  memoryShare: { label: 'Memory share', resets: 'checked against the model each request uses' },
};

/** The word and tone of a limit's state (the bar's colour is never the only cue). */
export function limitStateLook(s: LimitState): Look {
  const map: Record<LimitState, Look> = {
    ok: { label: 'Within limit', tone: 'ok' },
    warning: { label: 'Close to the limit', tone: 'warn' },
    exceeded: { label: 'Limit reached', tone: 'bad' },
  };
  return map[s];
}

export function severityTone(s: Severity): Tone {
  return s === 'error' ? 'bad' : s === 'warn' ? 'warn' : 'info';
}

/** The icon of an event in the feed. */
export function eventIcon(kind: EventKind): string {
  const map: Record<EventKind, string> = {
    modelLoaded: 'models',
    modelUnloaded: 'release',
    contextReleased: 'broom',
    engineStarted: 'engines',
    engineRestarted: 'refresh',
    engineFailed: 'error',
    download: 'download',
    pairing: 'link',
    servingChanged: 'pause',
    presetChanged: 'sliders',
    requestFailed: 'alert',
    limitWarning: 'alert',
  };
  return map[kind];
}

/** Where a preset stands: always words, never colour alone. */
export function presetStatusLook(s: PresetStatus): Look {
  switch (s.state) {
    case 'ready':
      return { label: s.loaded ? 'Ready, loaded' : 'Ready', tone: 'ok' };
    case 'stale':
      return { label: 'Model removed', tone: 'bad' };
    case 'noMatch':
      return { label: 'No model matches', tone: 'warn' };
    case 'shadowed':
      return { label: 'Shadowed by a model', tone: 'warn' };
  }
}

/** Status-code tone for the request inspector. */
export function statusTone(status: number): Tone {
  if (status >= 500) return 'bad';
  if (status >= 400) return 'warn';
  return 'ok';
}
