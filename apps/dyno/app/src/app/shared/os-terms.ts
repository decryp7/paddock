import type { WindowPlatform } from '../core/platform';

/** The words each OS uses for the things Dyno names, so a Mac reads "File descriptors", not Windows' word for them. */
export interface OsTerms {
  /** What a process holds open: handles on Windows, file descriptors elsewhere. */
  readonly handles: string;
  /** An example of "a change you are about to make", for the benchmark label. */
  readonly powerExample: string;
}

const TERMS: Record<WindowPlatform, OsTerms> = {
  windows: { handles: 'Handles', powerExample: 'Power mode: High performance' },
  macos: { handles: 'File descriptors', powerExample: 'Low Power Mode: On' },
  linux: { handles: 'File descriptors', powerExample: 'Power profile: Performance' },
  browser: { handles: 'Handles', powerExample: 'Power mode: High performance' },
};

/** The terms of a platform. */
export function osTerms(platform: WindowPlatform): OsTerms {
  return TERMS[platform];
}
