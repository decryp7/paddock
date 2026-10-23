import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { DashboardPage } from './dashboard-page';
import type { Dashboard } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { CHART_LOADER } from '../../shared/charts/chart';
import { WatchesStore } from '../../state/watches.store';
import { appProviders, fakeChart, makeMock, NOW, settle, text } from '../../../testing/test-helpers';

@Component({ template: '' })
class Stub {}

async function make(patch: (d: Dashboard) => Dashboard) {
  const backend = makeMock();
  const real = backend.dashboard.bind(backend);
  vi.spyOn(backend, 'dashboard').mockImplementation(async (r) => patch(await real(r)));
  const charts: ReturnType<typeof fakeChart>[] = [];
  TestBed.configureTestingModule({
    providers: [...appProviders(backend, [{ path: 'watches/:id', component: Stub }]), { provide: CHART_LOADER, useValue: () => Promise.resolve(() => charts[charts.push(fakeChart()) - 1]) }],
  });
  TestBed.inject(ClockService).set(NOW);
  TestBed.inject(WatchesStore).start();
  const f = TestBed.createComponent(DashboardPage);
  await settle(f, 6);
  return { f, charts };
}
const q = (f: { nativeElement: HTMLElement }, sel: string) => f.nativeElement.querySelector<HTMLElement>(sel);

describe('dashboard reliability and downloads', () => {
  afterEach(() => vi.restoreAllMocks());

  it('draws the reliability chart when some checks failed', async () => {
    const { f, charts } = await make((d) => ({ ...d, watches: d.watches.map((w, i) => ({ ...w, checks: w.checks.map(() => 3), failedChecks: w.failedChecks.map((_, j) => (i === 0 && j === 1 ? 2 : 0)) })) }));
    expect(text(q(f, '#rel-h'))).toBe('Check reliability');
    expect(q(f, '[data-testid=reliability-chart] sp-chart')).not.toBeNull();
    expect(q(f, '[data-testid=reliability-quiet]')).toBeNull();
    const drawn = charts.flatMap((c) => c.options as { series: { name: string }[] }[]);
    expect(drawn.some((o) => o.series?.some((s) => s.name === 'Failed'))).toBe(true);
  });

  it('says so instead of drawing bars that are all the same when every check succeeded', async () => {
    const { f } = await make((d) => ({ ...d, watches: d.watches.map((w) => ({ ...w, checks: w.checks.map(() => 2), failedChecks: w.failedChecks.map(() => 0) })) }));
    expect(q(f, '[data-testid=reliability-chart]')).toBeNull();
    expect(text(q(f, '[data-testid=reliability-quiet]'))).toMatch(/^All \d+ checks in this range succeeded\.$/);
  });

  it('leaves the section out when nothing was checked', async () => {
    const { f } = await make((d) => ({ ...d, watches: d.watches.map((w) => ({ ...w, checks: w.checks.map(() => 0), failedChecks: w.failedChecks.map(() => 0) })) }));
    expect(q(f, '#rel-h')).toBeNull();
  });

  it('draws the downloads chart with its two panels when something was downloaded, and not otherwise', async () => {
    const { f, charts } = await make((d) => ({ ...d, downloads: d.downloads.map((b, i) => (i === 2 ? { count: 2, failed: 1, bytes: 5_000_000 } : b)) }));
    expect(text(q(f, '#dl-h'))).toBe('Downloads');
    expect(q(f, '[data-testid=downloads-chart] sp-chart')).not.toBeNull();
    const drawn = charts.flatMap((c) => c.options as { grid?: unknown[] }[]);
    expect(drawn.some((o) => Array.isArray(o.grid) && o.grid.length === 2)).toBe(true);
    TestBed.resetTestingModule();
    const none = await make((d) => ({ ...d, downloads: d.downloads.map(() => ({ count: 0, failed: 0, bytes: 0 })) }));
    expect(q(none.f, '#dl-h')).toBeNull();
  });
});
