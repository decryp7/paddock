import type {
  Availability,
  Capability,
  DownloadState,
  EventKind,
  FitVerdict,
  HotState,
  JobState,
  MemoryPressure,
  Preset,
  Priority,
  ProcessHealth,
  ReleaseReason,
  SelfTest,
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
  };
  return map[kind];
}

/** Status-code tone for the request inspector. */
export function statusTone(status: number): Tone {
  if (status >= 500) return 'bad';
  if (status >= 400) return 'warn';
  return 'ok';
}
