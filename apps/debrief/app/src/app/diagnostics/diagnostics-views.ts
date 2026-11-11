import { DebriefView } from '../extensions/tokens';

/** The view that lists the saved error snapshots. */
export const DIAGNOSTICS_VIEWS: DebriefView[] = [
  {
    id: 'error-snapshots',
    title: $localize`:@@diagnostics.commands.view.title:Error snapshots`,
    category: 'Diagnostics',
    icon: 'file',
    defaultPosition: 'center', layout: { area: 'status', min: { width: 320, height: 180 }, pref: { width: 480, height: 360 }, priority: 20 },
    component: () => import('./error-snapshots-view').then((m) => m.ErrorSnapshotsView),
  },
  {
    id: 'diagnostics-report',
    title: $localize`:@@diagnostics.commands.reportView.title:Diagnostics report`,
    category: 'Diagnostics',
    icon: 'file',
    defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 },
    component: () => import('./diagnostics-report-view').then((m) => m.DiagnosticsReportPanel),
  },
];
