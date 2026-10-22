import type { AppLimits, KeyedSeries } from '../../core/api.types';
import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { settle } from '../../../testing/render';

import { AppsCard, limitsText } from './apps-card';

describe('limitsText', () => {
  const none: AppLimits = { requestsPerMin: null, tokensPerDay: null, maxConcurrent: null };
  it('says None without limits', () => expect(limitsText(none)).toBe('None'));
  it('lists the limits that are set', () => {
    expect(limitsText({ requestsPerMin: 600, tokensPerDay: 5_000_000, maxConcurrent: 2 })).toBe('600/min · 5.0M/day · ×2');
    expect(limitsText({ ...none, maxConcurrent: 1 })).toBe('×1');
    expect(limitsText({ ...none, requestsPerMin: 0 })).toBe('0/min');
  });
});

async function mount(series?: KeyedSeries[], backend = new MockBackend()) {
  TestBed.configureTestingModule({ imports: [AppsCard], providers: [{ provide: BACKEND, useValue: backend }, provideRouter([])] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
  const fixture = TestBed.createComponent(AppsCard);
  if (series) fixture.componentRef.setInput('series', series);
  await settle(fixture);
  return { fixture, el: fixture.nativeElement as HTMLElement, live };
}

describe('AppsCard', () => {
  it('invites the user to pair an app when there are none', async () => {
    const { el } = await mount(undefined, new MockBackend({ empty: true }));
    expect(el.querySelector('pw-empty-state')!.textContent).toContain('No apps yet');
    expect(el.querySelector('a[href="/apps"]')).not.toBeNull();
    expect(el.querySelector('table')).toBeNull();
  });

  it('lists apps by today requests, busiest first, with connection state and tokens', async () => {
    const { el, live } = await mount();
    const rows = Array.from(el.querySelectorAll('tbody tr'));
    expect(rows.length).toBe(live.apps().length);
    const counts = rows.map((r) => Number(r.querySelector('td.right')!.textContent!.replace(/,/g, '')));
    expect(counts).toEqual([...counts].sort((a, b) => b - a));
    expect(rows.map((r) => r.querySelector('.nm')!.textContent)).toEqual(['Spotter', 'IMT', 'Continue (VS Code)', 'Pitcrew', 'Dyno']);
    expect(rows[1]!.querySelector('.avatar')!.textContent).toBe('I');
    expect(rows[0]!.querySelector('td.right')!.textContent).toBe('3,460');
    expect(rows[0]!.querySelector('.limits')!.textContent).toBe('600/min · 5.0M/day · ×2');
    expect(rows[3]!.querySelector('.limits')!.textContent).toBe('×4');
    expect(rows[1]!.querySelectorAll('td.right')[1]!.textContent).toBe('2.2M');
    const states = rows.map((r) => r.querySelector('.conn')!.getAttribute('aria-label'));
    expect(states).toContain('Connected');
    expect(states).toContain('Not connected');
  });

  it('marks apps with errors', async () => {
    const { el, live } = await mount();
    const errors = Array.from(el.querySelectorAll('tbody tr')).map((r) => r.querySelectorAll('td.right')[3] as HTMLElement);
    expect(errors.some((c) => c.classList.contains('bad'))).toBe(live.apps().some((a) => a.today.errors > 0));
    expect(errors.some((c) => !c.classList.contains('bad'))).toBe(true);
  });

  it('draws a sparkline from the series of each app and an empty one without data', async () => {
    const { el, live } = await mount([{ key: 'imt', values: [1, 4, 2] }]);
    const rows = Array.from(el.querySelectorAll('tbody tr'));
    const imt = rows.find((r) => r.querySelector('.nm')!.textContent === 'IMT')!;
    expect(imt.querySelectorAll('pw-sparkline path')[1]!.getAttribute('d')).toMatch(/^M0\.0,/);
    const other = rows.find((r) => r.querySelector('.nm')!.textContent !== 'IMT')!;
    expect(other.querySelectorAll('pw-sparkline path')[1]!.getAttribute('d')).toBe('');
    expect(live.apps().length).toBeGreaterThan(1);
  });

  it('colours an app by its slot in the series, and apps without a series after them', async () => {
    const { el } = await mount([
      { key: 'continue', values: [1] },
      { key: 'imt', values: [1] },
    ]);
    const stroke = (name: string) => {
      const row = Array.from(el.querySelectorAll('tbody tr')).find((r) => r.querySelector('.nm')!.textContent === name)!;
      return row.querySelectorAll('pw-sparkline path')[1]!.getAttribute('stroke');
    };
    expect(stroke('Continue (VS Code)')).not.toBe(stroke('IMT'));
    expect(stroke('Spotter')).not.toBe(stroke('IMT'));
    expect(stroke('Spotter')).not.toBe(stroke('Continue (VS Code)'));
  });
});
