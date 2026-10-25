import type { Tracking, WatchDraft } from './api.types';

/** The defaults for how the latest release is decided and which products are tracked. */
export function blankTracking(): Tracking {
  return { latestBy: 'version', missingOlder: false, products: { only: [], announceNew: false, settings: {} } };
}

/** A new watch with the defaults every watch starts from (the user's default interval is applied by the editor). */
export function blankDraft(over: Partial<WatchDraft> = {}): WatchDraft {
  return {
    id: null,
    name: '',
    source: { kind: 'github', values: {} },
    versions: 'semver',
    versionPattern: null,
    includePrereleases: false,
    nameFilter: '',
    schedule: { mode: 'every', secs: 6 * 3600 },
    notify: { mode: 'every-new', nag: false, quietHours: null },
    actions: [{ kind: 'open-location' }],
    channels: [],
    filter: { prefix: '', soakHours: 0 },
    paused: false,
    tracking: blankTracking(),
    ...over,
  };
}
