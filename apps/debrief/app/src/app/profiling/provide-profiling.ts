import { EnvironmentProviders, inject, Injector, makeEnvironmentProviders, provideAppInitializer } from '@angular/core';
import { CaptureSummary } from '../core/api.types';
import { COMMAND_CONTEXT } from '../extensions/command-registry';
import { DROPPED_FILES_HANDLERS, DroppedFilesHandler } from '../core/dropped-files';
import { CommandContext, DebriefView } from '../extensions/tokens';
import { UI_STATE_CONTRIBUTIONS } from '../lifecycle/ui-state';
import { CapturesStore } from './captures.store';
import { ProfilingPersistence } from './profiling-persistence';
import { provideProfilingBackend } from './profiling-backend';
import { PROFILING_VIEW_IDS as V } from './profiling.ids';
import { isHeap, isProfile } from './profiling-format';
import { provideDotnetBackend } from './dotnet/dotnet-backend';
import { provideNativeBackend } from './native/native-backend';
import { PROFILING_COMMANDS_CONTRIBUTIONS } from './profiling.contributions';
import { debriefContributionProviders } from '../extensions/contributions';

/** The profiling views. Same extension point as plugin views; none opens until a capture is. */
export const PROFILING_VIEWS: DebriefView[] = [
  { id: V.captures, category: 'Profiling', title: $localize`:@@profiling.views.captures:Captures`, icon: 'files', defaultPosition: 'right', layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 372, height: 400 }, priority: 45 }, component: () => import('./captures/captures-view').then((m) => m.CapturesView) },
  { id: V.hotspots, category: 'Profiling', title: $localize`:@@profiling.views.hotspots:Hotspots`, icon: 'columns', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./hotspots/hotspots-view').then((m) => m.HotspotsView) },
  { id: V.tree, category: 'Profiling', title: $localize`:@@profiling.views.tree:Call tree`, icon: 'group', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./tree/call-tree-view').then((m) => m.CallTreeView) },
  { id: V.inverted, category: 'Profiling', menu: 'palette', title: $localize`:@@profiling.views.inverted:Inverted tree`, icon: 'group', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./tree/call-tree-view').then((m) => m.InvertedTreeView) },
  { id: V.butterfly, category: 'Profiling', menu: 'palette', title: $localize`:@@profiling.views.butterfly:Callers and callees`, icon: 'selector', defaultPosition: 'right', layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 372, height: 400 }, priority: 45 }, component: () => import('./butterfly/butterfly-view').then((m) => m.ButterflyView) },
  { id: V.flame, category: 'Profiling', title: $localize`:@@profiling.views.flame:Flame graph`, icon: 'chart', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./flame/flame-view').then((m) => m.FlameView) },
  { id: V.compare, category: 'Profiling', title: $localize`:@@profiling.views.compare:Compare profiles`, icon: 'columns', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./compare/compare-view').then((m) => m.CompareView) },
  { id: V.threads, category: 'Profiling', title: $localize`:@@profiling.views.threads:Thread timeline`, icon: 'timeline', defaultPosition: 'bottom', layout: { area: 'analysis', min: { width: 420, height: 180 }, pref: { width: 560, height: 280 }, priority: 50 }, component: () => import('./threads/thread-timeline-view').then((m) => m.ThreadTimelineView) },
  { id: V.findings, category: 'Profiling', title: $localize`:@@profiling.views.findings:Profile findings`, icon: 'alert', defaultPosition: 'right', layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 372, height: 400 }, priority: 45 }, component: () => import('./findings/findings-view').then((m) => m.FindingsView) },
  { id: V.heap, category: 'Profiling', title: $localize`:@@profiling.views.heap:Memory`, icon: 'metrics', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./heap/heap-view').then((m) => m.HeapView) },
];

/** Profiles open on the hotspots, heap snapshots on the memory view; the captures list follows. */
export function showOpened(ctx: CommandContext, opened: CaptureSummary[]): void {
  if (!opened.length) return;
  ctx.suggestPerspective?.('profiling');
  ctx.showView(V.captures);
  if (opened.some(isProfile)) ctx.showView(V.hotspots);
  else if (opened.some(isHeap)) ctx.showView(V.heap);
}

/** Loads the captures the engine already has open in this window. */
export function startProfiling(): void {
  void inject(CapturesStore).refresh();
}

/**
 * The drag-and-drop claim of profiling: files the engine sniffs as profiles or heap snapshots open
 * here (and show their view, or their error in the Profiles view); the rest go to the log import.
 */
export function profilingDropHandler(): DroppedFilesHandler {
  const captures = inject(CapturesStore);
  const injector = inject(Injector);
  return async (paths) => {
    const { claimed, opened } = await captures.openDropped(paths);
    const ctx = injector.get(COMMAND_CONTEXT);
    showOpened(ctx, opened);
    if (captures.error()) ctx.showView(V.captures);
    return claimed;
  };
}

/** One line in `app.config.ts`: profiling backend, views, commands, drag-and-drop of captures and their persistence. */
export function provideProfiling(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...provideProfilingBackend(),
    ...provideDotnetBackend(),
    ...provideNativeBackend(),
    ...debriefContributionProviders({ views: PROFILING_VIEWS }),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...PROFILING_COMMANDS_CONTRIBUTIONS,
    { provide: DROPPED_FILES_HANDLERS, multi: true, useFactory: profilingDropHandler },
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => [inject(ProfilingPersistence).contribution] },
    provideAppInitializer(() => startProfiling()),
  ]);
}
