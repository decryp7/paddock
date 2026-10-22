import type { WatchDraft } from './api.types';

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
    intervalSecs: 6 * 3600,
    notify: { mode: 'every-new', nag: false, quietHours: null },
    actions: [{ kind: 'open-location' }],
    channels: [],
    filter: { prefix: '', soakHours: 0 },
    paused: false,
    ...over,
  };
}
