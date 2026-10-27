import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { ActivityPage, LOW_UPTIME_PERMILLE, withJobsAxis } from './activity-page';
import { trendBars } from './trend-bars';

describe('ActivityPage', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  async function mount() {
    localStorage.clear();
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(ActivityPage);
    await settle(f, 8);
    return { f, el: f.nativeElement as HTMLElement };
  }

  it('lists the recent jobs with the server\'s result when a token is known', async () => {
    const { el } = await mount();
    expect(text(el)).toContain('Recent jobs');
    expect(text(el.querySelector('#recent-title')?.closest('section')?.querySelector('thead') ?? null)).toContain('Result');
    expect(el.querySelectorAll('tbody tr').length).toBeGreaterThan(0);
    expect(el.querySelectorAll('.badge').length).toBe(0);
  });

  it('turns a conclusion into a badge, and no conclusion into nothing', async () => {
    const { f } = await mount();
    const page = f.componentInstance as unknown as { outcome(c: string | undefined): { label: string; tone: string } | null };
    expect(page.outcome('success')).toEqual({ label: 'Succeeded', tone: 'success' });
    expect(page.outcome('failure')?.tone).toBe('danger');
    expect(page.outcome('cancelled')?.label).toBe('Cancelled');
    expect(page.outcome('skipped')?.label).toBe('Skipped');
    expect(page.outcome('unknown')).toBeNull();
    expect(page.outcome(undefined)).toBeNull();
  });

  it('shows one runners table with labels once and no summary cards', async () => {
    const { el } = await mount();
    const table = el.querySelector('table.runners') as HTMLElement;
    expect(table).not.toBeNull();
    expect(text(table.querySelector('thead'))).toContain('Avg job');
    expect(text(table.querySelector('thead'))).toContain('Jobs trend');
    expect(el.querySelectorAll('.stat').length).toBe(0);
    expect(el.querySelectorAll('table.runners tbody tr').length).toBeGreaterThan(0);
    expect(el.querySelectorAll('table.runners pc-trend-bars').length).toBeGreaterThan(0);
  });

  it('sorts by a clicked header and reports aria-sort', async () => {
    const { f, el } = await mount();
    const th = (label: string) => Array.from(el.querySelectorAll('table.runners th')).find((h) => text(h).includes(label)) as HTMLElement;
    expect(th('Runner').getAttribute('aria-sort')).toBe('ascending');
    const names = () => Array.from(el.querySelectorAll('table.runners tbody td.name')).map((c) => text(c).trim());
    const asc = names();
    expect(asc).toEqual([...asc].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase())));
    (th('Runner').querySelector('button') as HTMLButtonElement).click();
    f.detectChanges();
    expect(th('Runner').getAttribute('aria-sort')).toBe('descending');
    expect(names()).toEqual([...asc].reverse());
    (th('Jobs').querySelector('button') as HTMLButtonElement).click();
    f.detectChanges();
    expect(th('Jobs').getAttribute('aria-sort')).toBe('descending');
    expect(th('Runner').getAttribute('aria-sort')).toBe('none');
    (th('Jobs').querySelector('button') as HTMLButtonElement).click();
    f.detectChanges();
    expect(th('Jobs').getAttribute('aria-sort')).toBe('ascending');
  });

  it('tints uptime only below 99%', async () => {
    const { f, el } = await mount();
    const page = f.componentInstance as unknown as { runnerRows(): { lowUptime: boolean; uptimePermille: number }[] };
    for (const r of page.runnerRows()) expect(r.lowUptime).toBe(r.uptimePermille < LOW_UPTIME_PERMILLE);
    expect(LOW_UPTIME_PERMILLE).toBe(990);
    const lows = el.querySelectorAll('table.runners td.low').length;
    expect(lows).toBe(page.runnerRows().filter((r) => r.lowUptime).length);
  });

  it('shows results as a dot and a word, with no row pills', async () => {
    const { el } = await mount();
    const results = el.querySelectorAll('tbody .result');
    expect(results.length).toBeGreaterThan(0);
    for (const r of Array.from(results)) {
      expect(r.querySelector('.dot')).not.toBeNull();
      expect(text(r).trim().length).toBeGreaterThan(0);
    }
    expect(el.querySelectorAll('.badge').length).toBe(0);
  });

  it('lists update results as a dot and "Updated from -> to" text', async () => {
    const { f, el } = await mount();
    const page = f.componentInstance as unknown as { updates(): { text: string; tone: string }[] };
    const rows = el.querySelectorAll('[aria-labelledby="upd-title"] .update');
    expect(rows.length).toBe(page.updates().length);
    for (const u of page.updates()) expect(u.text).toMatch(/^(Updated|Failed|Rolled back|Postponed|Cancelled)/);
    for (const r of Array.from(rows)) expect(r.querySelector('.dot')).not.toBeNull();
  });

  it('keeps the Debrief action icon-only, labelled, and reachable by keyboard', async () => {
    const { el } = await mount();
    const cell = el.querySelector('tbody td.row-action') as HTMLElement;
    const btn = cell.querySelector('button') as HTMLButtonElement;
    expect(btn.getAttribute('aria-label')).toMatch(/in Debrief$/);
    expect(text(btn).trim()).toBe('');
    expect(btn.tabIndex).toBe(0);
    const css = Array.from(document.querySelectorAll('style')).map((x) => x.textContent).join('');
    expect(css).toMatch(/focus-within/);
  });

  it('draws bars on a zero baseline and gives the jobs chart a legend only for 2+ series', () => {
    const bars = trendBars([0, 5, 10], 64, 20);
    expect(bars.map((b) => b.h)).toEqual([1, 10, 20]);
    expect(bars.every((b) => b.y + b.h === 20)).toBe(true);
    expect(trendBars([], 64, 20)).toEqual([]);
    expect(trendBars([0, 0], 64, 20).every((b) => b.h === 1)).toBe(true);
    const one = withJobsAxis({ series: [{}], yAxis: { type: 'value' } }) as { legend: { show: boolean }; yAxis: { name: string; min: number } };
    expect(one.legend.show).toBe(false);
    expect(one.yAxis.min).toBe(0);
    expect(one.yAxis.min).toBe(0);
    expect((withJobsAxis({ series: [{}, {}] }) as { legend: { show: boolean } }).legend.show).toBe(true);
    expect((withJobsAxis({}) as { legend: { show: boolean } }).legend.show).toBe(false);
  });
});
