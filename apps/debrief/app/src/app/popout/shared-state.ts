import { Injector, runInInjectionContext } from '@angular/core';
import { contributedItems, DEBRIEF_KIND } from '../extensions/contributions';
import { Query, RecordView } from '../core/api.types';
import { WorkspaceStore } from '../core/workspace.store';
import { TimeCursor } from '../analysis/time-cursor';
import { sanitizeQuery } from '../core/sanitize-query';
import { LiveFollow, sanitizeFollow } from '../live/live-follow.store';
import { LiveTail } from '../workspace/live-tail';
import { SyncedValue } from './ui-sync';

/** The time cursor follows the pointer: other windows get it at most this often. */
export const CURSOR_SYNC_MS = 50;


function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/** A query from another window: known keys only, plus the query-language filter and the live window when set. */
export function sharedQuery(v: unknown): Query {
  const q: Query = sanitizeQuery(v);
  if (!isRecord(v)) return q;
  if (typeof v['ql'] === 'string') q.ql = v['ql'];
  if (isRecord(v['window'])) q.window = v['window'] as unknown as Query['window'];
  return q;
}

/** A selected record from another window, or null. */
export function sharedRecord(v: unknown): RecordView | null {
  return isRecord(v) && typeof v['id'] === 'number' && typeof v['message'] === 'string' ? (v as unknown as RecordView) : null;
}

export function sharedRange(v: unknown): { from: number; to: number } | null {
  return isRecord(v) && typeof v['from'] === 'number' && typeof v['to'] === 'number' ? { from: v['from'], to: v['to'] } : null;
}

/** How much of the host's state a guest window shares (Paddock rules 32/34: a window instantiates only the stores it uses). */
export type SharedScope =
  /** Everything: a pop-out, the host, and a dialog that shows the workspace's events. */
  | 'full'
  /** Display settings and the data version (a dialog that reads the workspace summary). */
  | 'summary'
  /** Display settings only (theme, formats, density): a dialog that never reads the workspace. */
  | 'display';

/** The workspace interaction state: query, selection, jumps, sort and columns, time cursor, live follow and tail. */
function workspaceState(injector: Injector): SyncedValue[] {
  const store = injector.get(WorkspaceStore);
  const cursor = injector.get(TimeCursor);
  const follow = injector.get(LiveFollow);
  const live = injector.get(LiveTail);
  return [
    { key: 'query', read: () => store.query(), apply: (v) => store.restoreQuery(sharedQuery(v)) },
    { key: 'selected', read: () => store.selected(), apply: (v) => store.select(sharedRecord(v)) },
    {
      key: 'jump',
      read: () => store.anchor(),
      apply: (v) => {
        if (isRecord(v) && typeof v['id'] === 'number') store.jumpTo(v['id']);
      },
    },
    {
      key: 'sort',
      read: () => store.descending(),
      apply: (v) => {
        if (typeof v === 'boolean' && v !== store.descending()) store.toggleSort();
      },
    },
    {
      key: 'columns',
      read: () => store.extraColumns(),
      apply: (v) => {
        if (!Array.isArray(v)) return;
        const want = v.filter((c): c is string => typeof c === 'string');
        for (const c of store.extraColumns()) if (!want.includes(c)) store.removeColumn(c);
        for (const c of want) store.addColumn(c);
      },
    },
    { key: 'cursor', read: () => cursor.hovered(), apply: (v) => cursor.hover(typeof v === 'number' ? v : null), throttleMs: CURSOR_SYNC_MS },
    { key: 'range', read: () => cursor.selection(), apply: (v) => cursor.select(sharedRange(v)) },
    {
      key: 'follow',
      read: () => ({ follow: follow.follow(), preset: follow.preset(), customMinutes: follow.customMinutes(), paused: follow.paused() }),
      apply: (v) => {
        follow.restore(sanitizeFollow(v));
        if (isRecord(v) && v['paused'] === true) follow.pause();
      },
    },
    {
      key: 'live',
      direction: 'fromHost',
      read: () => live.enabled(),
      apply: (v) => {
        if (v === true && !live.enabled()) void live.restore();
        else if (v === false) live.enabled.set(false);
      },
    },
  ];
}

/**
 * A group of shared state as a contribution: Debrief's `debriefSharedState` kind until M3a moves it to the shell's
 * `defineSharedState`. It differs from that design in two ways until then: a group binds several values, and it is
 * placed in one of three scopes (display, summary, full) where the shell's has two (every window, or the window group);
 * each value parses what another window sends in its own `apply`. A key already bound by an earlier group is left out.
 * Registered in `shared-state.contributions.ts` (main window, pop-outs) and `display-state.contributions.ts` (dialog
 * windows).
 */
export interface SharedStateDef {
  /** Namespaced id: `db.shared.display`. */
  readonly id: string;
  /** `display`: every window; `summary`: windows that read the workspace summary and pop-outs; `full`: pop-outs and the host. */
  readonly scope: SharedScope;
  values(injector: Injector): SyncedValue[];
}

/** The scopes each window scope includes. */
const SCOPE_INCLUDES: Readonly<Record<SharedScope, readonly SharedScope[]>> = {
  display: ['display'],
  summary: ['display', 'summary'],
  full: ['display', 'summary', 'full'],
};

/** The workspace group: query, selection and jumps, sort and columns, time cursor, live follow and tail (pop-outs and the host). */
export const WORKSPACE_SHARED_STATE: SharedStateDef = Object.freeze({ id: 'db.shared.workspace', scope: 'full', values: workspaceState });

/**
 * The interaction state a host and its guests share: the query, selection and jumps, time cursor and brushed
 * range, live follow, theme and display settings, plus what only the host decides (data versions, live tail on/off),
 * as the window's registered groups bind them (a window that registers none shares nothing).
 * A guest that shows less asks for a narrower `scope`, so it never creates the stores of what it does not show.
 * A window ignores the keys it was not given.
 */
export function sharedUiState(injector: Injector, scope: SharedScope = 'full'): SyncedValue[] {
  const registered = runInInjectionContext(injector, () => contributedItems<SharedStateDef>(DEBRIEF_KIND.sharedState));
  const groups = registered.filter((g) => SCOPE_INCLUDES[scope].includes(g.scope));
  const seen = new Set<string>();
  return groups.flatMap((g) => g.values(injector)).filter((v) => !seen.has(v.key) && !!seen.add(v.key));
}
