import { TestBed } from '@angular/core/testing';
import type { LifeForecast, LifeOutcome, LifespanOverview } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { mockLifespan } from '../../core/mock/hardware';
import { provideIcons } from '../../shared/icons';
import { LifespanPage } from './lifespan-page';

const NOW = 1_700_000_000_000;

function render(overview?: LifespanOverview, fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  if (overview) vi.spyOn(mock, 'lifespan').mockResolvedValue(overview);
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(LifespanPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 5; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};
const one = (outcome: LifeOutcome, extra: Partial<LifeForecast> = {}): LifespanOverview => ({
  at: NOW,
  forecasts: [{ subject: 'ssd', name: 'Test SSD', outcome, confidence: 'medium', headline: 'The headline', explanation: 'Because.', basis: [], historyDays: 100, readings: 10, restartedAt: null, ...extra }],
});

describe('dy-lifespan-page', () => {
  it('lists the disk and the battery with the engine headline, a range bar and the confidence in words', async () => {
    const { f, el } = render(mockLifespan('macos', 'laptop', 'calm'));
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Disk and battery life');
    const items = [...el.querySelectorAll('.list > li')];
    expect(items).toHaveLength(2);
    expect(items[0]!.textContent).toContain('About 7.4 years of writes left (5.2 to 10 years)');
    expect(items[0]!.querySelector('.conf')?.textContent).toBe('Confidence: medium');
    expect(items[1]!.getAttribute('data-subject')).toBe('battery');
    const range = items[1]!.querySelector('.range')!;
    expect(range.getAttribute('aria-label')).toBe('Soonest 17 months, expected 23 months, latest 31 months');
    expect(range.querySelector('.mark')).not.toBeNull();
    expect(range.textContent).toContain('Expected 23 months');
    expect(items[1]!.querySelectorAll('.kv > div')).toHaveLength(4);
    expect(items[1]!.querySelector('.kv')?.getAttribute('aria-label')).toBe('What this stands on for Battery');
  });

  it('a desktop has only its disk', async () => {
    const { f, el } = render(mockLifespan('macos', 'desktop', 'calm'));
    await settle(f);
    expect(el.querySelectorAll('.list > li')).toHaveLength(1);
  });

  it('too little history shows the progress in days', async () => {
    const { f, el } = render(mockLifespan('macos', 'laptop', 'learning'));
    await settle(f);
    const bars = [...el.querySelectorAll('[role=progressbar]')];
    expect(bars).toHaveLength(2);
    expect(bars[0]!.getAttribute('aria-label')).toBe('History collected');
    expect(bars[0]!.getAttribute('aria-valuemax')).toBe('100');
    expect(bars[0]!.getAttribute('aria-valuenow')).toBe(String(Math.round((6 / 14) * 100)));
    expect(bars[0]!.getAttribute('aria-valuetext')).toBe('Have 6 of 14 days');
    expect(el.textContent).toContain('Have 6 of 14 days');
    expect(el.querySelector('.range')).toBeNull();
  });

  it('unavailable gives the reason and no bar', async () => {
    const { f, el } = render(mockLifespan('macos', 'laptop', 'blind'));
    await settle(f);
    expect(el.querySelector('.unavailable')?.textContent).toContain('smartmontools');
    expect(el.querySelector('.range, [role=progressbar]')).toBeNull();
    expect(el.querySelectorAll('.why')).toHaveLength(0);
  });

  it.each([
    ['beyondHorizon', 'More than 10 years', 'ok'],
    ['noWearDetected', 'No wear detected', 'ok'],
    ['alreadyPastLimit', 'Already past its rated limit', 'warn'],
  ] as const)('%s shows the headline with a dot and no range bar', async (outcome, headline, tone) => {
    const { f, el } = render(one(outcome, { headline }));
    await settle(f);
    expect(el.querySelector('.headline')?.textContent).toContain(headline);
    expect(el.querySelector('.headline .dot')?.getAttribute('data-tone')).toBe(tone);
    expect(el.querySelector('.range, [role=progressbar], .unavailable')).toBeNull();
    expect(el.querySelector('.why')?.textContent).toBe('Because.');
  });

  it('says when a different disk or battery was detected, and hints at low confidence', async () => {
    const at = new Date(2026, 8, 3, 12).getTime(); // noon local time: the same calendar day in every time zone
    const { f, el } = render(one('beyondHorizon', { restartedAt: at, subject: 'battery', confidence: 'low' }));
    await settle(f);
    expect(el.querySelector('.note')?.textContent).toMatch(/A different battery was detected on .*3.*; only readings since then are used\./);
    expect(el.querySelector('.conf')?.textContent).toBe('Confidence: low (a rough guide)');
    expect(el.querySelector('.note')).not.toBeNull();
  });

  it('shows no note when the series was not restarted', async () => {
    const { f, el } = render(one('beyondHorizon'));
    await settle(f);
    expect(el.querySelector('.note')).toBeNull();
  });

  it('has an empty state when there is nothing to forecast', async () => {
    const { f, el } = render({ at: NOW, forecasts: [] });
    await settle(f);
    expect(el.textContent).toContain('No disk or battery to forecast');
  });

  it('shows a failed request inline with Retry', async () => {
    const { f, el } = render(undefined, ['lifespan']);
    await settle(f);
    expect(el.querySelector('[role=alert]')).not.toBeNull();
    expect(el.textContent).toContain('Retry');
  });
});
