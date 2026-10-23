import { TestBed } from '@angular/core/testing';
import type { BaselineOverview } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { mockBaselines } from '../../core/mock/hardware';
import { provideIcons } from '../../shared/icons';
import { BASELINES_REFRESH_MS } from '../../state/baselines.store';
import { NormalCard } from './normal-card';

const NOW = 1_700_000_000_000;

function render(overview: BaselineOverview | null, fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  if (overview) vi.spyOn(mock, 'baselines').mockResolvedValue(overview);
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(NormalCard);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 5; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};

describe('dy-normal-card', () => {
  afterEach(() => vi.useRealTimers());

  it('while learning: one quiet line and no highlights', async () => {
    const { f, el } = render(mockBaselines(NOW, 'learning'));
    await settle(f);
    expect(el.querySelector('.line')?.textContent).toBe('Learning what is normal for this PC: day 3 of 7');
    expect(el.querySelector('li')).toBeNull();
  });

  it('lists only the unusual items with current and usual values and the explanation', async () => {
    const { f, el } = render(mockBaselines(NOW, 'calm'));
    await settle(f);
    const rows = [...el.querySelectorAll('li')];
    expect(rows).toHaveLength(3);
    expect(rows[0]!.querySelector('.what')?.textContent).toContain('CPU: Higher than usual for Tuesday mornings');
    expect(rows[0]!.querySelector('.vals')?.textContent).toBe('71% now · usually 14%');
    expect(rows[0]!.getAttribute('title')).toContain('usually about 14%');
    expect(rows[0]!.querySelector('.visually-hidden')?.textContent).toContain('usually about 14%');
    expect(rows[0]!.querySelector('.dot')?.getAttribute('data-tone')).toBe('warn');
    expect(rows[1]!.querySelector('.dot')?.getAttribute('data-tone')).toBe('info');
    expect(rows.map((r) => r.textContent).join()).not.toContain('Usual for');
    expect(el.querySelector('.line')).toBeNull();
  });

  it('says everything looks usual when nothing is unusual', async () => {
    const calm = mockBaselines(NOW, 'calm');
    const { f, el } = render({ ...calm, items: calm.items.filter((i) => i.judgement.state === 'usual') });
    await settle(f);
    expect(el.querySelector('.line')?.textContent).toBe('Everything looks usual for this time of the week');
    expect(el.querySelector('li')).toBeNull();
  });

  it('shows nothing when baselines are off, while loading, or when the request failed', async () => {
    const off = render({ at: NOW, learning: 'off', items: [] });
    await settle(off.f);
    expect(off.el.querySelector('section')).toBeNull();
    TestBed.resetTestingModule();
    const failing = render(null, ['baselines']);
    await settle(failing.f);
    expect(failing.el.querySelector('section')).toBeNull();
  });

  it('refreshes every minute while the dashboard is open and stops when it closes', async () => {
    vi.useFakeTimers();
    const { f, mock } = render(mockBaselines(NOW, 'calm'));
    const spy = vi.spyOn(mock, 'baselines');
    await vi.advanceTimersByTimeAsync(1);
    const first = spy.mock.calls.length;
    await vi.advanceTimersByTimeAsync(BASELINES_REFRESH_MS + 10);
    expect(spy.mock.calls.length).toBe(first + 1);
    f.destroy();
    await vi.advanceTimersByTimeAsync(BASELINES_REFRESH_MS * 3);
    expect(spy.mock.calls.length).toBe(first + 1);
  });
});
