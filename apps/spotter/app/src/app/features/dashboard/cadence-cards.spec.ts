import { TestBed } from '@angular/core/testing';
import type { WatchNumbers, WatchSummary } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { appProviders, makeMock, NOW, settle, text } from '../../../testing/test-helpers';
import { CadenceCards } from './cadence-cards';

const watch = (id: string, name: string, latest: WatchSummary['latest'] = null): WatchSummary => ({ ...(({} as unknown) as WatchSummary), id, name, latest });
const nums = (watchId: string, releases: number[], averageIntervalMs: number | null = null): WatchNumbers => ({
  watchId, averageIntervalMs, sinceLastMs: null, releases, failedChecks: [], checks: [], sparkline: [],
});

describe('CadenceCards', () => {
  async function make(watches: WatchSummary[], numbers: WatchNumbers[], range?: string) {
    TestBed.configureTestingModule({ providers: appProviders(makeMock()) });
    TestBed.inject(ClockService).set(NOW);
    const f = TestBed.createComponent(CadenceCards);
    f.componentRef.setInput('watches', watches);
    f.componentRef.setInput('numbers', numbers);
    if (range) f.componentRef.setInput('rangeLabel', range);
    await settle(f);
    return f;
  }
  const cards = (f: { nativeElement: HTMLElement }) => Array.from(f.nativeElement.querySelectorAll<HTMLAnchorElement>('a.card'));

  it('shows nothing without releases or for unknown watches', async () => {
    const f = await make([watch('a', 'Alpha')], [nums('a', [0, 0]), nums('ghost', [1, 1])]);
    expect(cards(f)).toHaveLength(0);
  });

  it('draws a card per released watch, busiest first then by name, with a tick per bucket that has releases', async () => {
    const latest = { version: '2.0.0', firstSeenMs: NOW - 3_600_000 } as WatchSummary['latest'];
    const f = await make(
      [watch('a', 'Beta', latest), watch('b', 'Alpha'), watch('c', 'Gamma')],
      [nums('a', [1, 0, 1]), nums('b', [1, 0, 1]), nums('c', [0, 5, 0], 86_400_000)],
      '30 days',
    );
    const list = cards(f);
    expect(list.map((c) => text(c.querySelector('.name')))).toEqual(['Gamma', 'Alpha', 'Beta']);
    expect(list[0]!.getAttribute('href')).toBe('/watches/c');
    expect(text(list[0]!.querySelector('.sub'))).toBe('5 releases · about every 1 day');
    expect(text(list[1]!.querySelector('.sub'))).toBe('2 releases · in 30 days');
    expect(list[0]!.getAttribute('aria-label')).toBe('Gamma, 5 releases in 30 days');
    expect(list[0]!.querySelectorAll('line.tick')).toHaveLength(1);
    expect(list[0]!.querySelector('line.tick.multi')).not.toBeNull(); // five releases in one bucket
    expect(list[1]!.querySelector('line.tick.multi')).toBeNull();
    expect(list[1]!.querySelectorAll('line.tick')).toHaveLength(2);
    expect(list[2]!.querySelector('sp-ver')).not.toBeNull();
    expect(text(list[2]!.querySelector('.axis-labels'))).toContain('30 days ago');
    expect(text(list[2]!.querySelector('.axis-labels'))).toContain('last 1 h ago');
    expect(text(list[1]!.querySelector('.axis-labels'))).toContain('last —');
    expect(list[1]!.querySelector('sp-ver')).toBeNull();
  });

  it('uses the singular for one release', async () => {
    const f = await make([watch('a', 'Alpha')], [nums('a', [0, 1])]);
    expect(text(cards(f)[0]!.querySelector('.sub'))).toBe('1 release · in 90 days');
  });
});
