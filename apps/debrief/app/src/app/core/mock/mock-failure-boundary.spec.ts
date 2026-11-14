import { TestBed } from '@angular/core/testing';
import { MemoryLogger, query } from '../../../testing/helpers';
import { createAnalysisBackend } from '../../analysis/analysis.plugin';
import { createShareBackend } from '../../share/share.plugin';
import { AnalysisBackend } from '../analysis-backend';
import { DebriefBackend, provideBackend } from '../backend';
import { Logger } from '../logger';
import { HomeBackend } from '../../home/home-backend';
import { ProblemsBackend } from '../../problems/problems-backend';
import { SearchEverythingBackend } from '../../search-everything/search-everything-backend';
import { ShareBackend } from '../../share/share-backend';
import { StorePaneBackend } from '../../store-pane/store-pane-backend';
import type { RunsBackend } from '../../timeline/runs-backend';
import { FAILABLE, type Failable } from './mock-options';

/**
 * `?mockFail=<command>:N` simulates a failure of the UI's own call. The demo's other mock backends read their data through
 * the same commands, so they must read the raw mock, never the backend the UI receives (it would count their calls and use
 * up failures meant for the UI). One case per failable command.
 */
const UI_CALLS: Record<Failable, (b: DebriefBackend) => Promise<unknown>> = {
  workspaceSummary: (b) => b.workspaceSummary(),
  queryPage: (b) => b.queryPage({ query: query(), offset: 0, limit: 3, descending: false }),
  queryTimeline: (b) => b.queryTimeline({ query: query(), buckets: 10, alwaysShowLifecycle: false }),
  queryFacet: (b) => b.queryFacet({ query: query(), field: 'level', limit: 5 }),
  sourceContext: (b) => b.sourceContext({ recordId: 1, before: 1, after: 1 }),
  readFileHead: (b) => b.readFileHead('/incident-2026-10-14/web01/orders-20261014.clef', 100),
  previewParse: (b) => b.previewParse({ toml: '', sample: 's', fileName: 'a.log', limit: 5 }),
  annotations: (b) => b.annotations({ from: null, to: null }),
  queryRuns: (b) => (b as unknown as RunsBackend).queryRuns({}),
  previewSpans: (b) => (b as unknown as RunsBackend).previewSpans({ draft: { kind: 'r', label: 'R', start: { type: 'markers', kinds: ['start'] }, stop: { type: 'markers', kinds: ['stop'] }, per: 'instance' } }),
  searchPartial: (b) => b.searchPartial(query()),
};

/** Every call of the demo's mock backends that reads the workspace through the backend they were given; errors are not the point. */
async function exerciseMocks(): Promise<void> {
  const settle = async (...calls: (() => Promise<unknown>)[]): Promise<void> => {
    for (const call of calls) await call().catch(() => undefined);
  };
  const home = TestBed.inject(HomeBackend);
  const share = TestBed.inject(ShareBackend);
  const search = TestBed.inject(SearchEverythingBackend);
  const tile = (type: string, extra: object = {}) => ({ tile: { id: 't', title: 't', type, x: 0, y: 0, w: 1, h: 1, ...extra } as never, query: query() });
  await settle(
    () => home.querySummary({ query: query(), buckets: 40 }),
    () => TestBed.inject(AnalysisBackend).queryAnomalies(query()),
    () => TestBed.inject(ProblemsBackend).problems(),
    () => TestBed.inject(StorePaneBackend).querySql('select * from events limit 3', false, 'r1'),
    () => search.searchEverything({ text: 'error' }),
    () => share.dashboardTile(tile('top', { field: 'level' })),
    () => share.dashboardTile(tile('annotations')),
    () => share.dashboardTile(tile('table', { query: 'level:error' })),
    () => share.redactionSummary({ query: query() } as never),
    () => share.overview(query()),
  );
}

describe('the mock failure boundary', () => {
  afterEach(() => window.history.replaceState({}, '', '/'));

  for (const command of FAILABLE) {
    it(`${command}: no mock backend consumes the simulated failure; the UI-facing backend still does`, async () => {
      window.history.replaceState({}, '', `/?mockFail=${command}:1`);
      TestBed.configureTestingModule({ providers: [provideBackend(), { provide: AnalysisBackend, useFactory: createAnalysisBackend }, { provide: ShareBackend, useFactory: createShareBackend }, { provide: Logger, useFactory: () => new MemoryLogger() }] });
      const ui = TestBed.inject(DebriefBackend);
      await exerciseMocks();
      await expect(UI_CALLS[command](ui)).rejects.toBe(`Simulated ${command} failure`);
      await expect(UI_CALLS[command](ui)).resolves.not.toBeUndefined();
    });
  }
});
