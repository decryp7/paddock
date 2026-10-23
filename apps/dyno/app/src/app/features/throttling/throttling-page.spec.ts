import { TestBed } from '@angular/core/testing';
import type { ThrottleOverview } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { mockThrottling } from '../../core/mock/hardware';
import { provideIcons } from '../../shared/icons';
import { THROTTLE_REFRESH_MS, ThrottleStore } from '../../state/throttle.store';
import { ThrottlingPage } from './throttling-page';

const NOW = 1_700_000_000_000;

function render(overview?: ThrottleOverview, fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  if (overview) vi.spyOn(mock, 'throttling').mockResolvedValue(overview);
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(ThrottlingPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 5; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};
const scenario = (s: 'calm' | 'hot' | 'blind', os: 'macos' | 'windows' = 'macos') => mockThrottling(os, NOW, s);

describe('dy-throttling-page', () => {
  afterEach(() => vi.useRealTimers());

  it('calm: says it runs at full speed with a quiet dot, the confidence in words and the evidence', async () => {
    const { f, el } = render(scenario('calm'));
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Throttling');
    expect(el.querySelector('.status h2')?.textContent).toBe('Running at full speed');
    expect(el.querySelector('.status .dot')?.getAttribute('data-tone')).toBe('ok');
    expect(el.querySelector('.conf')?.textContent).toBe('Confidence: high');
    expect(el.querySelectorAll('.kv > div')).toHaveLength(3);
    expect(el.querySelector('.current')).toBeNull();
  });

  it('hot: warns with the cause and shows the running episode', async () => {
    const { f, el } = render(scenario('hot'));
    await settle(f);
    expect(el.querySelector('.status h2')?.textContent).toBe('Slowed down by heat: 44% slower');
    expect(el.querySelector('.status .dot')?.getAttribute('data-tone')).toBe('warn');
    expect(el.querySelector('.current')?.textContent).toContain('Slowed for 7 min, since');
    expect(el.querySelector('.card-head .summary')?.textContent).toContain('throttled in the last 30 days');
  });

  it('lists the last 30 days in one table, newest first, with the cause as a word, and sorts by Started', async () => {
    const { f, el } = render(scenario('calm'));
    await settle(f);
    const rows = () => [...el.querySelectorAll('tbody tr')];
    expect(rows()).toHaveLength(5);
    const heads = [...el.querySelectorAll('thead th')].map((t) => t.textContent!.replace(/\s+/g, ' ').trim());
    expect(heads).toEqual(['Started', 'Duration', 'Cause', 'Speed lost, average (%)', 'Peak temperature (°C)']);
    const cause = (i: number) => rows()[i]!.querySelectorAll('td')[2]!.textContent!.trim();
    expect(cause(0)).toBe('Heat');
    expect(cause(2)).toBe('Power limit');
    expect(cause(3)).toBe('Battery saver');
    expect(rows()[2]!.querySelectorAll('td')[4]!.textContent).toBe('—');
    expect(rows()[0]!.querySelectorAll('td')[1]!.textContent).toBe('14 min');
    const first = rows()[0]!.textContent;
    expect(el.querySelector('th[aria-sort]')?.getAttribute('aria-sort')).toBe('descending');
    el.querySelector<HTMLButtonElement>('th button.sort')!.click();
    f.detectChanges();
    expect(el.querySelector('th[aria-sort]')?.getAttribute('aria-sort')).toBe('ascending');
    expect(rows()[4]!.textContent).toBe(first);
    el.querySelector<HTMLButtonElement>('th button.sort')!.click();
    f.detectChanges();
    expect(rows()[0]!.textContent).toBe(first);
  });

  it('blind: cannot tell, explains what the machine does not report and has an empty table state', async () => {
    const { f, el } = render(scenario('blind'));
    await settle(f);
    expect(el.querySelector('.status h2')?.textContent).toContain('Cannot tell');
    expect(el.querySelector('.status .dot')?.getAttribute('data-tone')).toBe('muted');
    expect(el.querySelector('table')).toBeNull();
    expect(el.querySelector('.empty strong')?.textContent).toBe('No throttling in the last 30 days');
    // The list opens on its own when Dyno cannot tell.
    const items = [...el.querySelectorAll('.inputs li')];
    expect(items).toHaveLength(4);
    expect(items[0]!.textContent).toContain('Not available');
    expect(items[0]!.textContent).toContain('does not report');
    el.querySelector<HTMLButtonElement>('.inputs button')!.click();
    f.detectChanges();
    expect(el.querySelectorAll('.inputs li')).toHaveLength(0);
    expect(el.querySelector('.inputs button')?.getAttribute('aria-expanded')).toBe('false');
  });

  it('keeps what the machine reports collapsed while it can tell, and opens on a click', async () => {
    const { f, el } = render(scenario('calm', 'windows'));
    await settle(f);
    expect(el.querySelectorAll('.inputs li')).toHaveLength(0);
    el.querySelector<HTMLButtonElement>('.inputs button')!.click();
    f.detectChanges();
    const items = [...el.querySelectorAll('.inputs li')];
    expect(items.some((i) => i.textContent!.includes('Available'))).toBe(true);
    expect(items.some((i) => i.textContent!.includes('Not available'))).toBe(true);
  });

  it('shows a failed request inline with Retry, which asks again', async () => {
    const { f, el, mock } = render(undefined, ['throttling']);
    await settle(f);
    expect(el.querySelector('[role=alert]')).not.toBeNull();
    expect(el.querySelector('table')).toBeNull();
    vi.spyOn(mock, 'throttling').mockResolvedValue(scenario('calm'));
    [...el.querySelectorAll<HTMLButtonElement>('[role=alert] button')].find((b) => b.textContent?.includes('Retry'))!.click();
    await settle(f);
    expect(el.querySelector('.status h2')?.textContent).toBe('Running at full speed');
  });

  it('asks again every 15 seconds while open and stops when destroyed', async () => {
    vi.useFakeTimers();
    const { f, mock } = render(scenario('calm'));
    const spy = vi.spyOn(mock, 'throttling');
    await vi.advanceTimersByTimeAsync(1);
    f.detectChanges();
    const first = spy.mock.calls.length;
    await vi.advanceTimersByTimeAsync(THROTTLE_REFRESH_MS * 2 + 10);
    expect(spy.mock.calls.length).toBe(first + 2);
    f.destroy();
    await vi.advanceTimersByTimeAsync(THROTTLE_REFRESH_MS * 3);
    expect(spy.mock.calls.length).toBe(first + 2);
    expect(TestBed.inject(ThrottleStore).overview()).not.toBeNull();
  });
});
