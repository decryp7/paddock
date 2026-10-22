import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { ReportsStore } from '../../state/reports.store';
import { ReportsPage } from './reports-page';

const NOW = 1_700_000_000_000;

function render(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(ReportsPage);
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

describe('dy-reports-page', () => {
  it('shows every section of the week', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Reports');
    const stats = el.querySelector('.stats')!.textContent!;
    expect(stats).toContain('Health score');
    expect(stats).toContain('+3 against the week before');
    expect(el.querySelectorAll('[aria-label="Highlights"] li').length).toBeGreaterThan(0);
    expect(el.querySelector('[aria-label="Health score trend"] dy-sparkline')).not.toBeNull();
    expect(el.querySelectorAll('[aria-label="Busiest apps"] tbody tr')).toHaveLength(5);
    expect(el.querySelector('[aria-label="Disks"]')!.textContent).toContain('Full in ~41 days');
    expect(el.querySelector('[aria-label="Issues"]')!.textContent).toContain('Drive is getting full');
    expect(el.querySelector('[aria-label="What changed"]')!.textContent).toContain('Turned off Updater at login');
    expect(el.querySelector('.range')?.textContent).toContain(' to ');
  });

  it('offers the week choices and shows an empty week as a valid report', async () => {
    const { f, el } = render();
    await settle(f);
    const select = el.querySelector<HTMLSelectElement>('select[aria-label="Week"]')!;
    expect(select.options[0]!.textContent).toBe('Last 7 days');
    expect(select.options).toHaveLength(12);
    select.value = '6';
    select.dispatchEvent(new Event('change'));
    await settle(f);
    expect(TestBed.inject(ReportsStore).weeksAgo()).toBe(6);
    expect(el.querySelector('.empty')?.textContent).toContain('Nothing to report');
    expect(el.querySelector('.stats')).toBeNull();
    expect(btn(el, 'Export Markdown').disabled).toBe(false);
  });

  it('exports Markdown and the web page where the person chooses and shows the path', async () => {
    const { f, el, mock } = render();
    await settle(f);
    btn(el, 'Export Markdown').click();
    await settle(f);
    btn(el, 'Export web page').click();
    await settle(f);
    expect(mock.savedReports.map((r) => r.format)).toEqual(['markdown', 'html']);
    expect(el.querySelector('.note .path')?.textContent).toMatch(/\.html$/);
  });

  it('shows a failed export inline', async () => {
    const { f, el, mock } = render();
    await settle(f);
    vi.spyOn(mock, 'saveReport').mockRejectedValue({ code: 'accessDenied', message: 'Dyno could not write the report' });
    btn(el, 'Export Markdown').click();
    await settle(f);
    expect(el.querySelector('.note.bad')?.textContent).toContain('could not write');
  });

  it('shows a failed load inline with Retry and cannot export what it does not have', async () => {
    const { f, el, mock } = render(['weeklyReport']);
    await settle(f);
    expect(el.querySelector('[role=alert]')).not.toBeNull();
    expect(btn(el, 'Export Markdown').disabled).toBe(true);
    vi.spyOn(mock, 'weeklyReport').mockRestore();
  });
});
