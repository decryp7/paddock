import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the analysis commands: stubs only, so `AnalysisCommands` and the query session, targets and
// time cursor it injects load when the window is idle or a command runs (debrief-1-2-api.md §14.3).
// `analysis.contributions.spec.ts` keeps the stubs equal to the class.

const CATEGORY = $localize`:@@analysis.commands.category:Analyze`;

export const ANALYSIS_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'query.focus', gated: true, title: $localize`:@@analysis.commands.queryFocus:Query: focus the search field`, category: CATEGORY, keybinding: 'Mod+/' },
  { id: 'query.run', gated: true, title: $localize`:@@analysis.commands.queryRun:Query: run`, category: CATEGORY },
  { id: 'query.clear', gated: true, title: $localize`:@@analysis.commands.queryClear:Query: clear applied query`, category: CATEGORY },
  { id: 'query.results', gated: true, title: $localize`:@@analysis.commands.queryResults:Query: show results`, category: CATEGORY, view: 'query-results' },
  { id: 'moment.analyse', gated: true, title: $localize`:@@analysis.commands.momentAnalyse:Analyse this moment`, category: CATEGORY, keybinding: 'Mod+Shift+M' },
  { id: 'moment.selected', gated: true, title: $localize`:@@analysis.commands.momentSelected:Moment: selected range`, category: CATEGORY },
  { id: 'anomalies.show', gated: true, title: $localize`:@@analysis.commands.anomalies:Show anomalies`, category: CATEGORY, view: 'anomalies' },
  { id: 'patterns.show', gated: true, title: $localize`:@@analysis.commands.patterns:Show patterns`, category: CATEGORY, view: 'patterns' },
  { id: 'exceptions.show', gated: true, title: $localize`:@@analysis.commands.exceptions:Show exceptions`, category: CATEGORY, view: 'exceptions' },
  { id: 'outliers.show', gated: true, title: $localize`:@@analysis.commands.outliers:Show error outliers`, category: CATEGORY, view: 'outliers' },
  { id: 'compare.show', gated: true, title: $localize`:@@analysis.commands.compare:Compare time ranges`, category: CATEGORY, view: 'compare' },
  { id: 'compare.previous', gated: true, title: $localize`:@@analysis.commands.comparePrevious:Compare: current range vs previous`, category: CATEGORY },
  { id: 'compare.beforeAfter', gated: true, title: $localize`:@@analysis.commands.compareBeforeAfter:Compare: before / after selected time`, category: CATEGORY },
  { id: 'fieldstats.show', gated: true, title: $localize`:@@analysis.commands.fieldStats:Field statistics…`, category: CATEGORY },
  { id: 'traces.show', gated: true, title: $localize`:@@analysis.commands.traces:Show traces and requests`, category: CATEGORY, view: 'traces' },
];

export const ANALYSIS_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./analysis-commands').then((m) => m.AnalysisCommands), ANALYSIS_COMMAND_STUBS);
