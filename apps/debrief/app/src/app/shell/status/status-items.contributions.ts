import type { Provider } from '@angular/core';
import { contributionProviders, type StatusItemDef } from '@decryp7/paddock-ui/shell/contributions';
import type { DebriefContext } from '../../extensions/context';
import { localMessage } from '../../extensions/contributions';

/*
 * The status bar's items as the shell's `statusItems` contributions (workbench.md §4.3 M1). The registration holds data
 * and loaders only: each item's component (and the stores it reads) loads after the first paint. Debrief's bar
 * (`shell/status-bar.ts`) keeps its look until M3 moves the frame; it renders the items of each side by `order`.
 */

/** A Debrief status item: the shell's `StatusItemDef` and its place on its side (ascending; built-in items use multiples of 100). */
export interface DebriefStatusItemDef extends StatusItemDef<DebriefContext> {
  readonly order: number;
}

/** Declares a status item (frozen; identity for the types, as the shell's `defineStatusItem`). */
export function debriefStatusItem(def: DebriefStatusItemDef): DebriefStatusItemDef {
  return Object.freeze({ ...def });
}

/** Debrief's built-in status items, start side then end side, in bar order. */
export const DEBRIEF_STATUS_ITEMS: readonly DebriefStatusItemDef[] = [
  debriefStatusItem({
    id: 'db.status.counts',
    title: localMessage('db.status.counts', () => $localize`:@@shell.status.item.counts:Event counts`),
    side: 'start',
    priority: 1000,
    order: 100,
    load: () => import('./workspace-counts').then((m) => m.StatusCounts),
  }),
  debriefStatusItem({
    id: 'db.status.service',
    title: localMessage('db.status.service', () => $localize`:@@shell.status.item.service:Service health`),
    side: 'end',
    priority: 900,
    pinned: true,
    order: 100,
    load: () => import('./service-chip-item').then((m) => m.StatusService),
  }),
  debriefStatusItem({
    id: 'db.status.busy',
    title: localMessage('db.status.busy', () => $localize`:@@shell.status.item.busy:Work in progress`),
    side: 'end',
    priority: 800,
    order: 200,
    load: () => import('./status-busy').then((m) => m.StatusBusy),
  }),
  debriefStatusItem({
    id: 'db.status.import',
    title: localMessage('db.status.import', () => $localize`:@@shell.status.item.import:Import progress`),
    side: 'end',
    priority: 800,
    order: 300,
    load: () => import('../../import/import-ui').then((m) => m.ImportStatus),
  }),
  debriefStatusItem({
    id: 'db.status.ai-browsing',
    title: localMessage('db.status.ai-browsing', () => $localize`:@@shell.status.item.aiBrowsing:AI browsing`),
    side: 'end',
    priority: 600,
    order: 400,
    load: () => import('../../browser/ai-browsing-chip').then((m) => m.AiBrowsingChip),
  }),
  debriefStatusItem({
    id: 'db.status.live',
    title: localMessage('db.status.live', () => $localize`:@@shell.status.item.live:Live tail`),
    side: 'end',
    priority: 700,
    order: 500,
    load: () => import('./live-toggle').then((m) => m.StatusLive),
  }),
  debriefStatusItem({
    id: 'db.status.background',
    title: localMessage('db.status.background', () => $localize`:@@shell.status.item.background:Background work`),
    side: 'end',
    priority: 500,
    order: 600,
    load: () => import('../../windowing/background-work').then((m) => m.BackgroundWork),
  }),
  debriefStatusItem({
    id: 'db.status.problems',
    title: localMessage('db.status.problems', () => $localize`:@@shell.status.item.problems:Problems`),
    side: 'end',
    priority: 850,
    pinned: true,
    order: 700,
    load: () => import('../../problems/problems-badge').then((m) => m.ProblemsBadge),
  }),
  debriefStatusItem({
    id: 'db.status.jobs',
    title: localMessage('db.status.jobs', () => $localize`:@@shell.status.item.jobs:Jobs`),
    side: 'end',
    priority: 750,
    order: 800,
    load: () => import('../../jobs/status-jobs').then((m) => m.StatusJobs),
  }),
  debriefStatusItem({
    id: 'db.status.time',
    title: localMessage('db.status.time', () => $localize`:@@shell.status.item.time:Time display`),
    side: 'end',
    priority: 650,
    order: 900,
    load: () => import('./status-time').then((m) => m.StatusTime),
  }),
  debriefStatusItem({
    id: 'db.status.details',
    title: localMessage('db.status.details', () => $localize`:@@shell.status.item.details:Details`),
    side: 'end',
    priority: 400,
    order: 1000,
    load: () => import('./status-time').then((m) => m.StatusDetails),
  }),
];

/** Registers status items in the shell's registry (a plugin's, or a test's). */
export function provideStatusItems(items: readonly DebriefStatusItemDef[]): Provider[] {
  return contributionProviders('statusItems', items);
}

/** One line in `app.config.ts`: Debrief's built-in status items. */
export function provideDebriefStatusItems(): Provider[] {
  return provideStatusItems(DEBRIEF_STATUS_ITEMS);
}
