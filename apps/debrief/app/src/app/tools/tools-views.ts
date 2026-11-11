import { TOOL_IDS } from '../core/tools.types';
import { DebriefView } from '../extensions/tokens';

/** Answers whether a tool is listed by the engine (see `ToolCatalog.listed`). */
export interface ToolListing {
  listed(id: string): boolean;
}

/**
 * The tool panels. Same extension point as plugin views. Panels of plugin tools are offered only while their tool is
 * listed (not in safe mode, not with the tools plugin disabled).
 */
export function toolsViews(tools: ToolListing): DebriefView[] {
  return [
    { id: 'perfmon', category: 'Tools', title: $localize`:@@tools.view.perfmon:Performance Monitor`, icon: 'chart', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./perfmon/perfmon-view').then((m) => m.PerfmonView), enabled: () => tools.listed(TOOL_IDS.perfmonTemplates) },
    { id: 'process-monitor', category: 'Tools', title: $localize`:@@tools.view.processMonitor:Process Monitor`, icon: 'metrics', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./monitor/monitor-view').then((m) => m.MonitorView), enabled: () => tools.listed(TOOL_IDS.processMonitor) },
    { id: 'event-log-follower', category: 'Tools', title: $localize`:@@tools.view.eventLog:Event Log Follower`, icon: 'file', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./eventlog/eventlog-view').then((m) => m.EventLogView), enabled: () => tools.listed(TOOL_IDS.eventLog) },
    { id: 'saved-queries', category: 'Analysis', title: $localize`:@@tools.view.savedQueries:Saved queries`, icon: 'file', defaultPosition: 'right', layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 372, height: 400 }, priority: 45 }, component: () => import('../projects/bookmarks/saved-queries-view').then((m) => m.SavedQueriesView) },
  ];
}
