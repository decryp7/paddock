import type { AbstractType } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MemoryLogger, query } from '../../../testing/helpers';
import { createAnalysisBackend } from '../../analysis/analysis.plugin';
import { HomeBackend } from '../../home/home-backend';
import { IncidentBackend, KnownIssuesBackend, PlaybookBackend } from '../../investigate/investigate-backend';
import { ProblemsBackend } from '../../problems/problems-backend';
import { SearchEverythingBackend } from '../../search-everything/search-everything-backend';
import { ParsersBackend } from '../../settings/parsers/parsers-backend';
import { createShareBackend } from '../../share/share.plugin';
import { ShareBackend } from '../../share/share-backend';
import { EncodingBackend } from '../../sources/encoding/encoding-backend';
import { PasteBackend } from '../../sources/paste/paste-backend';
import { StorePaneBackend } from '../../store-pane/store-pane-backend';
import { RunsBackend } from '../../timeline/runs-backend';
import { AnalysisBackend } from '../analysis-backend';
import type { PlaybookStep, RunsRequest, SpanPreviewRequest, TileCfg, TileRequest } from '../api.types';
import { DebriefBackend, provideBackend } from '../backend';
import { Logger } from '../logger';
import { MOCK_WORKSPACE } from '../mock-workspace';
import { ProjectBackend, provideToolsBackend } from '../tools-backend';
import { UxBackend } from '../ux-backend';
import { UI_FACING_BACKEND } from './mock-failures';
import { FAILABLE, type Failable } from './mock-options';

/**
 * `?mockFail=<command>:N` simulates a failure of the UI's own call. The demo's mock backends read the workspace too, so they
 * must read the raw mock (`MOCK_WORKSPACE`), never the backend the UI receives: it would count their calls and use up
 * failures meant for the UI. Every consumer is exercised; each declares the commands it reads and the spec checks, on the raw
 * mock, that it called them (so a consumer that stops reading cannot leave this guard vacuous).
 */
interface Consumer {
  name: string;
  /** The commands the consumer reads from the workspace, checked on the raw mock. */
  reads: Failable[];
  call: () => Promise<unknown>;
}

const step = (ql: string): PlaybookStep => ({ id: 's', title: 's', instruction: '', action: { type: 'filter', ql } });
const tile = (type: TileCfg['type'], extra: Partial<TileCfg> = {}): TileRequest => ({ tile: { id: 't', title: 't', type, x: 0, y: 0, w: 1, h: 1, ...extra }, query: query() });
const draft: SpanPreviewRequest['draft'] = { kind: 'r', label: 'R', start: { type: 'markers', kinds: ['start'] }, stop: { type: 'markers', kinds: ['stop'] }, per: 'instance' };

function consumers(): Consumer[] {
  const get = TestBed.inject.bind(TestBed);
  return [
    { name: 'home', reads: ['workspaceSummary', 'queryTimeline', 'queryPage'], call: () => get(HomeBackend).querySummary({ query: query(), buckets: 40 }) },
    { name: 'analysis', reads: ['queryPage'], call: () => get(AnalysisBackend).queryAnomalies(query()) },
    { name: 'problems', reads: ['workspaceSummary'], call: () => get(ProblemsBackend).problems() },
    { name: 'store pane', reads: ['workspaceSummary', 'queryPage'], call: () => get(StorePaneBackend).querySql('select * from events limit 3', false, 'r1') },
    { name: 'search everything', reads: ['workspaceSummary', 'queryPage', 'queryFacet'], call: () => get(SearchEverythingBackend).searchEverything({ text: 'error' }) },
    { name: 'share: top tile', reads: ['queryFacet'], call: () => get(ShareBackend).dashboardTile(tile('top', { field: 'level' })) },
    { name: 'share: annotations tile', reads: ['annotations'], call: () => get(ShareBackend).dashboardTile(tile('annotations')) },
    { name: 'share: redaction summary', reads: ['queryPage'], call: () => get(ShareBackend).redactionSummary({ query: query() }) },
    { name: 'share: redacted view', reads: ['workspaceSummary'], call: () => get(ShareBackend).setRedactedView(false) },
    { name: 'tools: project', reads: ['workspaceSummary'], call: () => get(ProjectBackend).openProject('/x/last-session.debriefproj') },
    { name: 'paste', reads: ['workspaceSummary'], call: () => get(PasteBackend).pasteLog('one line', 'a.log') },
    { name: 'known issues', reads: ['queryPage'], call: () => get(KnownIssuesBackend).match() },
    { name: 'incident', reads: ['workspaceSummary', 'queryPage'], call: () => get(IncidentBackend).build({}) },
    { name: 'playbook', reads: ['queryPage'], call: () => get(PlaybookBackend).runStep(step('level=error')) },
    // These hold the raw mock for what it computes itself (parser library, encodings, clock, demo paths), not a failable command.
    { name: 'parsers', reads: [], call: () => get(ParsersBackend).list() },
    { name: 'encoding', reads: [], call: () => get(EncodingBackend).options() },
    { name: 'ux', reads: [], call: () => get(UxBackend).demoPaths() },
  ];
}

/** The UI's own calls of each failable command, as the panels make them. */
const UI_CALLS: Record<Failable, () => Promise<unknown>> = {
  workspaceSummary: () => TestBed.inject(DebriefBackend).workspaceSummary(),
  queryPage: () => TestBed.inject(DebriefBackend).queryPage({ query: query(), offset: 0, limit: 3, descending: false }),
  queryTimeline: () => TestBed.inject(DebriefBackend).queryTimeline({ query: query(), buckets: 10, alwaysShowLifecycle: false }),
  queryFacet: () => TestBed.inject(DebriefBackend).queryFacet({ query: query(), field: 'level', limit: 5 }),
  sourceContext: () => TestBed.inject(DebriefBackend).sourceContext({ recordId: 1, before: 1, after: 1 }),
  readFileHead: () => TestBed.inject(DebriefBackend).readFileHead('/incident-2026-10-14/web01/orders-20261014.clef', 100),
  previewParse: () => TestBed.inject(DebriefBackend).previewParse({ toml: '', sample: 's', fileName: 'a.log', limit: 5 }),
  annotations: () => TestBed.inject(DebriefBackend).annotations({ from: null, to: null }),
  queryRuns: () => TestBed.inject(RunsBackend).queryRuns({} satisfies RunsRequest),
  previewSpans: () => TestBed.inject(RunsBackend).previewSpans({ draft }),
  searchPartial: () => TestBed.inject(DebriefBackend).searchPartial(query()),
};

/** Commands no mock backend reads today: their cases guard the wrapper and any mock that starts reading one (add it to a consumer above). */
const FUTURE_GUARD: Failable[] = ['sourceContext', 'readFileHead', 'previewParse', 'queryRuns', 'previewSpans', 'searchPartial'];

/** True for the backend `provideBackend` hands the UI (the failure-injecting wrapper). */
function isUiFacing(backend: object): boolean {
  return Reflect.get(backend, UI_FACING_BACKEND) === true;
}

function setup(search: string) {
  window.history.replaceState({}, '', `/${search}`);
  TestBed.configureTestingModule({
    providers: [
      provideBackend(),
      provideToolsBackend(),
      { provide: AnalysisBackend, useFactory: createAnalysisBackend },
      { provide: ShareBackend, useFactory: createShareBackend },
      { provide: Logger, useFactory: () => new MemoryLogger() },
    ],
  });
}

describe('the mock failure boundary', () => {
  afterEach(() => window.history.replaceState({}, '', '/'));

  for (const command of FAILABLE) {
    it(`${command}: no mock backend consumes the simulated failure; the UI-facing backend still does${FUTURE_GUARD.includes(command) ? ' (future guard: no mock reads it yet)' : ''}`, async () => {
      setup(`?mockFail=${command}:1&mockLatency=0`);
      const raw = TestBed.inject(MOCK_WORKSPACE);
      const spies = new Map(FAILABLE.map((c) => [c, vi.spyOn(raw, c)]));
      for (const consumer of consumers()) {
        const before = new Map([...spies].map(([c, spy]) => [c, spy.mock.calls.length]));
        await consumer.call();
        for (const read of consumer.reads) expect(spies.get(read)?.mock.calls.length, `${consumer.name} reads ${read}`).toBeGreaterThan(before.get(read) ?? 0);
      }
      if (FUTURE_GUARD.includes(command)) {
        // Nothing reads these yet: when a consumer starts to, this fails and says to declare it above.
        expect(spies.get(command)?.mock.calls.length, `a mock now reads ${command}: declare it in a consumer`).toBe(0);
      }
      await expect(UI_CALLS[command]()).rejects.toBe(`Simulated ${command} failure`);
      await expect(UI_CALLS[command]()).resolves.not.toBeUndefined();
    });
  }

  it('no mock backend holds the backend the UI receives', () => {
    setup('?mockLatency=0');
    expect(isUiFacing(TestBed.inject(DebriefBackend))).toBe(true);
    expect(isUiFacing(TestBed.inject(MOCK_WORKSPACE))).toBe(false);
    const tokens: AbstractType<object>[] = [HomeBackend, ProblemsBackend, StorePaneBackend, SearchEverythingBackend, PasteBackend, KnownIssuesBackend, IncidentBackend, PlaybookBackend, ParsersBackend, EncodingBackend, UxBackend, ShareBackend];
    const held = tokens.map((token) => TestBed.inject(token));
    for (const service of held) {
      for (const [field, value] of Object.entries(service)) {
        if (value && typeof value === 'object') expect(isUiFacing(value), `${service.constructor.name}.${field}`).toBe(false);
      }
    }
  });
});
