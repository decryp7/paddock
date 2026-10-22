import type { AppEventKind, SourceGroup, WatchStatusKind, WatchSummary } from '../core/api.types';

/** The paddock status roles. */
export type Tone = 'success' | 'warning' | 'danger' | 'info' | 'neutral';

export interface StatusView {
  /** The word (status is never colour alone). */
  word: string;
  tone: Tone;
}

const STATUS_WORDS: Record<WatchStatusKind, StatusView> = {
  pending: { word: 'Waiting for first check', tone: 'neutral' },
  ok: { word: 'Working', tone: 'success' },
  noMatch: { word: 'Nothing matches', tone: 'warning' },
  unreachable: { word: 'Unreachable', tone: 'danger' },
  authNeeded: { word: 'Sign-in needed', tone: 'warning' },
  needsYou: { word: 'Needs you', tone: 'warning' },
  invalid: { word: 'Invalid settings', tone: 'danger' },
  failed: { word: 'Check failed', tone: 'danger' },
};

/** How a watch is shown: paused and checking come before the last check's outcome. */
export function statusView(w: Pick<WatchSummary, 'status' | 'paused' | 'checking'>): StatusView {
  if (w.checking) return { word: 'Checking now', tone: 'info' };
  if (w.paused) return { word: 'Paused', tone: 'neutral' };
  return STATUS_WORDS[w.status.kind];
}

/** Status kinds in the order the filter offers them. */
export const STATUS_FILTERS: { value: string; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'new', label: 'New' },
  { value: 'attention', label: 'Needs attention' },
  { value: 'paused', label: 'Paused' },
];

/** Does the watch need the user (a failed check or a page waiting on them)? */
export function needsAttention(w: Pick<WatchSummary, 'status' | 'paused'>): boolean {
  return !w.paused && w.status.kind !== 'ok' && w.status.kind !== 'pending';
}

/** Icon for a source kind id; unknown kinds get a generic one. */
export function kindIcon(kind: string): string {
  switch (kind) {
    case 'folder':
    case 'share':
      return 'folder';
    case 'http-dir':
    case 'web-page':
    case 'page':
      return 'globe';
    case 'feed':
    case 'rss':
      return 'feed';
    case 'json-api':
      return 'braces';
    case 'github':
    case 'gitea':
    case 'gitlab':
      return 'branch';
    case 'oci':
    case 'docker':
      return 'box';
    case 'npm':
    case 'crates':
    case 'pypi':
    case 'winget':
    case 'homebrew':
      return 'package';
    default:
      return 'sparkle';
  }
}

export const GROUP_LABEL: Record<SourceGroup, string> = {
  files: 'Files and shares',
  web: 'Web',
  forge: 'Code hosts',
  registry: 'Registries',
};

export const GROUP_ORDER: SourceGroup[] = ['files', 'web', 'forge', 'registry'];

export interface EventView {
  icon: string;
  tone: Tone;
  title: string;
}

/** How an event of the feed reads. */
export function eventView(kind: AppEventKind): EventView {
  switch (kind) {
    case 'new-release':
      return { icon: 'sparkle', tone: 'info', title: 'New release' };
    case 'check-failed':
      return { icon: 'alert', tone: 'danger', title: 'Check failed' };
    case 'recovered':
      return { icon: 'check', tone: 'success', title: 'Recovered' };
    case 'download-finished':
      return { icon: 'download', tone: 'success', title: 'Download finished' };
    case 'action-failed':
      return { icon: 'alert', tone: 'danger', title: 'Action failed' };
    case 'needs-you':
      return { icon: 'lock', tone: 'warning', title: 'Needs you' };
  }
}

export const INTERVAL_PRESETS: { secs: number; label: string }[] = [
  { secs: 900, label: '15 minutes' },
  { secs: 3600, label: '1 hour' },
  { secs: 6 * 3600, label: '6 hours' },
  { secs: 24 * 3600, label: 'Daily' },
  { secs: 7 * 24 * 3600, label: 'Weekly' },
];

export const SCHEME_OPTIONS: { value: string; label: string; help: string }[] = [
  { value: 'semver', label: 'Semantic version', help: '1.4.2, 2.0.0-rc.1' },
  { value: 'numeric', label: 'Dotted numbers', help: '1.04.00' },
  { value: 'build-number', label: 'Build number', help: 'R1.01.00T10, build 4521' },
  { value: 'date', label: 'Date', help: '2026-10-22, 20261022' },
  { value: 'custom', label: 'Custom pattern', help: 'Your own regular expression' },
];
