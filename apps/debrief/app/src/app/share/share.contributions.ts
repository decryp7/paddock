import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { provideDebriefPlugin } from '../extensions/provide';
import { REDACTED_VIEW_KEY } from './redaction/redaction-key';
import { SHARE_VIEWS } from './share.views';

// The registration file of the share features (redaction, reports, dashboards, bundles, visuals, redacted copies): static
// data and loaders only, so their command classes and stores load when the window is idle or a command runs, not before
// the first paint (debrief-1-2-api.md §14.3). `share.contributions.spec.ts` keeps the stubs equal to the classes.

const VIEW = $localize`:@@share.commands.categoryView:View`;
const ANALYZE = $localize`:@@share.commands.categoryAnalyze:Analyze`;
const FILE = $localize`:@@share.commands.categoryFile:File`;

export const REDACTION_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'share.redactedView.toggle', title: $localize`:@@share.redactionCommands.toggle:Toggle redacted view`, category: VIEW, keybinding: REDACTED_VIEW_KEY },
  { id: 'share.redaction.settings', title: $localize`:@@share.redactionCommands.settings:Redaction settings…`, category: VIEW },
];

export const REPORT_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'report.show', title: $localize`:@@share.reportCommands.show:Show report builder`, category: ANALYZE, view: 'report' },
  { id: 'report.new', title: $localize`:@@share.reportCommands.new:New incident report`, category: ANALYZE },
  { id: 'report.addSelected', gated: true, title: $localize`:@@share.reportCommands.addSelected:Add selected event to report`, category: ANALYZE },
  { id: 'report.addNotes', gated: true, title: $localize`:@@share.reportCommands.addNotes:Add notes to report`, category: ANALYZE },
  { id: 'report.addTimeline', gated: true, title: $localize`:@@share.reportCommands.addTimeline:Add timeline to report`, category: ANALYZE },
  { id: 'report.addFindings', gated: true, title: $localize`:@@share.reportCommands.addFindings:Add health-check findings to report`, category: ANALYZE },
  { id: 'report.addAnomalies', gated: true, title: $localize`:@@share.reportCommands.addAnomalies:Add anomalies to report as findings`, category: ANALYZE },
  { id: 'report.addQueryTable', gated: true, title: $localize`:@@share.reportCommands.addQueryTable:Add query result to report`, category: ANALYZE },
  { id: 'report.addAnnotations', gated: true, title: $localize`:@@share.reportCommands.addAnnotations:Add annotations to report`, category: ANALYZE },
  { id: 'report.export', gated: true, title: $localize`:@@share.reportCommands.export:Export report…`, category: FILE },
];

export const DASHBOARD_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'dashboards.show', title: $localize`:@@share.dashboardCommands.show:Show dashboards`, category: VIEW, view: 'dashboards' },
  { id: 'dashboards.edit', gated: true, title: $localize`:@@share.dashboardCommands.edit:Dashboard: toggle edit mode`, category: VIEW },
  { id: 'dashboards.addTile', gated: true, title: $localize`:@@share.dashboardCommands.addTile:Dashboard: add tile…`, category: VIEW },
  { id: 'dashboards.wall', gated: true, title: $localize`:@@share.dashboardCommands.wall:Dashboard: wall mode`, category: VIEW },
];

export const BUNDLE_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'share.bundle.create', gated: true, title: $localize`:@@share.bundleCommands.shareProject:Share as project file…`, category: FILE },
  { id: 'share.bundle.open', title: $localize`:@@share.bundleCommands.open:Open bundle…`, category: FILE },
];

export const VISUAL_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'visuals.timelinePng', gated: true, title: $localize`:@@share.visualCommands.timelinePng:Export timeline as PNG…`, category: FILE },
  { id: 'visuals.timelineSvg', gated: true, title: $localize`:@@share.visualCommands.timelineSvg:Export timeline as SVG…`, category: FILE },
];

export const REDACTED_COPY_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'share.redactedCopy', gated: true, title: $localize`:@@share.redactedCopy.command:Share redacted copy…`, category: FILE },
];

/** The share features' views and commands, in the order `provideShare` registered them before M1. */
export const SHARE_CONTRIBUTIONS = [
  ...lazyCommands(() => import('./redaction/redaction.commands').then((m) => m.RedactionCommands), REDACTION_COMMAND_STUBS),
  ...lazyCommands(() => import('./report/report.commands').then((m) => m.ReportCommands), REPORT_COMMAND_STUBS),
  ...lazyCommands(() => import('./dashboard/dashboard.commands').then((m) => m.DashboardCommands), DASHBOARD_COMMAND_STUBS),
  ...lazyCommands(() => import('./bundle/bundle.commands').then((m) => m.BundleCommands), BUNDLE_COMMAND_STUBS),
  ...lazyCommands(() => import('./visuals/visual.commands').then((m) => m.VisualCommands), VISUAL_COMMAND_STUBS),
  ...lazyCommands(() => import('./redacted-copy/redacted-copy-commands').then((m) => m.RedactedCopyCommands), REDACTED_COPY_COMMAND_STUBS),
  provideDebriefPlugin({ views: SHARE_VIEWS }),
];
