import { EnvironmentProviders, inject, Injector, makeEnvironmentProviders, provideAppInitializer } from '@angular/core';
import { DumpDocSummary } from '../core/api.types';
import { DROPPED_FILES_HANDLERS, DroppedFilesHandler } from '../core/dropped-files';
import { TOOL_IDS } from '../core/tools.types';
import { COMMAND_CONTEXT } from '../extensions/command-registry';
import { CommandContext, DebriefView } from '../extensions/tokens';
import { UI_STATE_CONTRIBUTIONS } from '../lifecycle/ui-state';
import { ToolCatalog } from '../tools/tool-catalog.store';
import { provideDumpsBackend } from './dumps-backend';
import { DumpsPersistence } from './dumps-persistence';
import { DUMP_VIEW_IDS } from './dumps.ids';
import { DUMPS_COMMANDS_CONTRIBUTIONS } from './dumps.contributions';
import { DumpsStore } from './dumps.store';
import { contributeFrom } from '../extensions/contributions';

export { DUMP_VIEW_IDS };
const V = DUMP_VIEW_IDS;

/** The dump views (same extension point as plugin views). */
export function dumpsViews(tools: { listed(id: string): boolean }): DebriefView[] {
  return [
    { id: V.dump, category: 'Diagnostics', title: $localize`:@@dumps.views.dump:Dump`, icon: 'alert', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./dump-view').then((m) => m.DumpView) },
    { id: V.umdh, category: 'Diagnostics', title: $localize`:@@dumps.views.umdh:UMDH heap growth`, icon: 'metrics', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./umdh-view').then((m) => m.UmdhView) },
    { id: V.capture, category: 'Tools', title: $localize`:@@dumps.views.capture:Create dump`, icon: 'file', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./create-dump-view').then((m) => m.CreateDumpView), enabled: () => tools.listed(TOOL_IDS.dumpCapture) },
  ];
}

/** Shows the view for what was opened: the Dump view for dumps, the UMDH view for heap logs. */
export function showOpened(ctx: CommandContext, opened: readonly DumpDocSummary[]): void {
  if (opened.some((d) => d.kind !== 'dump')) ctx.showView(V.umdh);
  if (opened.some((d) => d.kind === 'dump')) ctx.showView(V.dump);
}

/** Loads the documents the engine already has open in this window. */
export function startDumps(): void {
  void inject(DumpsStore).refresh();
}

/** The drag-and-drop claim of dumps: files the engine sniffs as dumps or UMDH logs open here; the rest go on. */
export function dumpsDropHandler(): DroppedFilesHandler {
  const store = inject(DumpsStore);
  const injector = inject(Injector);
  return async (paths) => {
    const { claimed, opened } = await store.openDropped(paths);
    const ctx = injector.get(COMMAND_CONTEXT);
    showOpened(ctx, opened);
    if (claimed.length && !opened.length && store.error()) ctx.showView(V.dump);
    return claimed;
  };
}

/** One line in `app.config.ts`: backend, views, commands, drag-and-drop of dumps and their persistence. */
export function provideDumps(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...provideDumpsBackend(),
    ...contributeFrom('views', () => dumpsViews(inject(ToolCatalog))),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...DUMPS_COMMANDS_CONTRIBUTIONS,
    { provide: DROPPED_FILES_HANDLERS, multi: true, useFactory: dumpsDropHandler },
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => [inject(DumpsPersistence).contribution] },
    provideAppInitializer(() => startDumps()),
  ]);
}
