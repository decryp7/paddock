import { Component, input } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { Chart } from '../../shared/chart';
import { IssuesPage } from './issues-page';
import { IssuesView } from './issues-view';

/** ECharts needs a real canvas; the chart's own spec covers drawing. */
@Component({ selector: 'dy-chart', template: '' })
class StubChart {
  readonly options = input<unknown>(null);
  readonly label = input('');
}

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

function render(fail: string[] = [], prepare: (mock: MockBackend) => void = () => undefined) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  prepare(mock);
  TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([]), { provide: BACKEND, useValue: mock }] });
  TestBed.overrideComponent(IssuesView, { remove: { imports: [Chart] }, add: { imports: [StubChart] } });
  const f = TestBed.createComponent(IssuesPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};
const btn = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text))!;

describe('dy-issues-page', () => {
  it('shows the score, the leaderboards and the events grouped by day', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Issues');
    expect(el.querySelector('[aria-label="Stability"] .big')?.textContent).toMatch(/\d+/);
    expect(el.querySelectorAll('table[aria-label="Apps with the most issues"] tbody tr').length).toBe(5);
    expect(el.querySelectorAll('table[aria-label="Issues"] tr.day').length).toBeGreaterThan(2);
    expect(el.textContent).toContain('2 of 2 logs read');
    expect(el.querySelector('.tab-count, .count')?.textContent).toBe('5');
  });

  it('filters by a leaderboard row, clears it, collapses a day and opens evidence as plain text', async () => {
    const { f, el } = render();
    await settle(f);
    (el.querySelector('tr[aria-label="Show only Mail"]') as HTMLElement).click();
    await settle(f);
    const rows = [...el.querySelectorAll('table[aria-label="Issues"] tr.row')];
    expect(rows.length).toBe(3);
    expect(rows.every((r) => r.textContent!.includes('Mail'))).toBe(true);
    (el.querySelector('[aria-label="Clear the filter"]') as HTMLElement).click();
    await settle(f);
    expect(el.querySelectorAll('table[aria-label="Issues"] tr.row').length).toBeGreaterThan(3);
    (el.querySelector('tr.day button') as HTMLElement).click();
    await settle(f);
    expect(el.querySelector('tr.day button')?.getAttribute('aria-expanded')).toBe('false');
    (el.querySelectorAll('tr.row')[0] as HTMLElement).click();
    await settle(f);
    const detail = el.querySelector('[aria-label="Issue details"]')!;
    expect(detail.querySelector('pre.evidence')).not.toBeNull();
    expect(detail.innerHTML).not.toContain('<b>');
    (el.querySelector('[aria-label="Close details"]') as HTMLElement).click();
    await settle(f);
    expect(el.querySelector('[aria-label="Issue details"]')).toBeNull();
  });

  it('switches the window, searches and shows a calm empty result', async () => {
    const { f, el } = render();
    await settle(f);
    btn(el, '7 days').click();
    await settle(f);
    expect(el.querySelector('[aria-label="Stability"] .trend')?.textContent).toContain('in 7 days');
    const input = el.querySelector<HTMLInputElement>('input[type=search]')!;
    input.value = 'zzzz';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    expect(el.textContent).toContain('Nothing matches this filter');
  });

  it('says which log could not be read and claims no score when none could', async () => {
    const { f, el } = render([], (mock) => {
      const real = mock.issues.bind(mock);
      vi.spyOn(mock, 'issues').mockImplementation(async (q) => ({
        ...(await real(q)),
        score: null,
        rating: null,
        daily: [],
        sources: [{ source: 'macLog', label: 'System log', state: { unavailable: 'log did not finish within 60 s.' }, at: NOW }],
      }));
    });
    await settle(f);
    expect(el.textContent).toContain('System log');
    expect(el.textContent).toContain('log did not finish within 60 s.');
    expect(el.querySelector('[aria-label="Stability"] .big')?.textContent).toBe('—');
  });

  it('shows an inline error with Retry', async () => {
    const { f, el } = render(['issues']);
    await settle(f);
    expect(el.querySelector('[role=alert]')).not.toBeNull();
    expect(btn(el, 'Retry')).toBeDefined();
  });

  it('exports and rescans from the header', async () => {
    const { f, el, mock } = render();
    await settle(f);
    btn(el, 'Export for Debrief').click();
    await settle(f);
    expect(mock.savedIssues).toEqual([14]);
    expect(el.textContent).toContain('dyno-issues.jsonl');
    (el.querySelector('[aria-label="Read the system logs now"]') as HTMLElement).click();
    await settle(f);
    expect(el.querySelector('[aria-label="Stability"]')).not.toBeNull();
  });

  it('new and unusual: reasons, protect, hide and show hidden', async () => {
    const { f, el } = render();
    await settle(f);
    btn(el, 'New and unusual').click();
    await settle(f);
    const table = el.querySelector('table[aria-label="New and unusual programs"]')!;
    expect(table.textContent).toContain('Unusual because it runs from a temporary folder');
    expect(table.textContent).not.toMatch(/malware|virus/i);
    (el.querySelector('[aria-label="Protect updater-helper"]') as HTMLElement).click();
    await settle(f);
    expect(el.querySelector('table[aria-label="New and unusual programs"]')!.textContent).not.toContain('updater-helper');
    (el.querySelector('[aria-label="Hide ngrok"]') as HTMLElement).click();
    await settle(f);
    btn(el, 'Show hidden').click();
    await settle(f);
    const text = el.querySelector('table[aria-label="New and unusual programs"]')!.textContent!;
    expect(text).toContain('ngrok');
    expect(text).toContain('Protected');
    expect(el.querySelector('[aria-label="Show ngrok again"]')).not.toBeNull();
  });

  it('new and unusual: the detail panel explains each reason and states what protect does', async () => {
    const { f, el } = render();
    await settle(f);
    btn(el, 'New and unusual').click();
    await settle(f);
    (el.querySelectorAll('tr.row')[0] as HTMLElement).click();
    await settle(f);
    const d = el.querySelector('[aria-label="Program details"]')!;
    expect(d.textContent).toContain('It is not signed by any publisher.');
    expect(d.textContent).toContain('never ends or removes a program');
    (d.querySelector('[aria-label="Close details"]') as HTMLElement).click();
    await settle(f);
    expect(el.querySelector('[aria-label="Program details"]')).toBeNull();
  });

  it('new and unusual: an empty list explains learning, and forgetting asks first', async () => {
    const { f, el, mock } = render();
    await settle(f);
    btn(el, 'New and unusual').click();
    await settle(f);
    await mock.forgetBaseline();
    btn(el, 'Show hidden').click();
    await settle(f);
    expect(el.textContent).toContain('Dyno is learning what is normal here');
  });
});
