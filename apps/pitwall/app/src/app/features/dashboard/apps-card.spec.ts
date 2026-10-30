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

import { AppsCard, limitsShort, limitsText } from './apps-card';

describe('limitsText', () => {
  const none: AppLimits = { requestsPerMin: null, tokensPerDay: null, maxConcurrent: null, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null };
  it('says None without limits', () => expect(limitsText(none)).toBe('None'));
  it('lists the limits that are set, in words', () => {
    expect(limitsText({ requestsPerMin: 600, tokensPerDay: 5_000_000, maxConcurrent: 2, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null })).toBe('600 requests/min · 5.0M tokens/day · 2 requests at once');
    expect(limitsText({ ...none, maxConcurrent: 1 })).toBe('1 request at once');
    expect(limitsText({ ...none, requestsPerMin: 0 })).toBe('0 requests/min');
    expect(limitsText({ ...none, requestsPerDay: 5000, tokensPerMonth: 3_000_000, maxMemoryPercent: 40 })).toBe('5.0k requests/day · 3.0M tokens/month · up to 40% of RAM');
  });
  it('shortens to the one limit or a count for a narrow cell', () => {
    expect(limitsShort(none)).toBe('None');
    expect(limitsShort({ ...none, maxConcurrent: 4 })).toBe('4 at once');
    expect(limitsShort({ ...none, requestsPerMin: 60 })).toBe('60 requests/min');
    expect(limitsShort({ ...none, requestsPerMin: 60, maxConcurrent: 2 })).toBe('2 limits');
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
    expect(el.querySelector('pd-empty-state')!.textContent).toContain('No apps yet');
    expect(el.querySelector('a[href="/apps"]')).not.toBeNull();
    expect(el.querySelector('table')).toBeNull();
  });

  it('lists apps by today requests, busiest first, with connection state and tokens', async () => {
    const { el, live } = await mount();
    const rows = Array.from(el.querySelectorAll('tbody tr'));
    expect(rows.length).toBe(live.apps().length);
    const counts = rows.map((r) => Number(r.querySelector('td.right')!.textContent!.replace(/,/g, '')));
    expect(counts).toEqual([...counts].sort((a, b) => b - a));
    expect(rows.map((r) => r.querySelector('.nm')!.textContent)).toEqual(['Spotter', 'Debrief', 'Nightly report script', 'Pitcrew', 'Dyno']);
    expect(rows[1]!.querySelector('.avatar')!.textContent).toBe('D');
    expect(rows[0]!.querySelector('td.right')!.textContent).toBe('3,460');
    expect(rows[0]!.querySelector('.limits')!.textContent).toBe('5 limits');
    expect(rows[3]!.querySelector('.limits')!.textContent).toBe('4 at once');
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
    const { el, live } = await mount([{ key: 'debrief', values: [1, 4, 2] }]);
    const rows = Array.from(el.querySelectorAll('tbody tr'));
    const imt = rows.find((r) => r.querySelector('.nm')!.textContent === 'Debrief')!;
    expect(imt.querySelector('pd-sparkline .pd-line')!.getAttribute('d')).toMatch(/^M3\.0 /);
    const other = rows.find((r) => r.querySelector('.nm')!.textContent !== 'Debrief')!;
    expect(other.querySelector('pd-sparkline .pd-line')!.getAttribute('d')).toBe('');
    expect(live.apps().length).toBeGreaterThan(1);
  });

  it('colours an app by its slot in the series, and apps without a series after them', async () => {
    const { el } = await mount([
      { key: 'nightly-report', values: [1] },
      { key: 'debrief', values: [1] },
    ]);
    const stroke = (name: string) => {
      const row = Array.from(el.querySelectorAll('tbody tr')).find((r) => r.querySelector('.nm')!.textContent === name)!;
      return (row.querySelector('pd-sparkline') as HTMLElement).style.color;
    };
    expect(stroke('Nightly report script')).not.toBe(stroke('Debrief'));
    expect(stroke('Spotter')).not.toBe(stroke('Debrief'));
    expect(stroke('Spotter')).not.toBe(stroke('Nightly report script'));
  });
  it('says limits every app shares once under the table instead of in a constant column', async () => {
    const backend = new MockBackend();
    const shared = { requestsPerMin: null, tokensPerDay: null, maxConcurrent: 4, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null };
    const real = backend.overview.bind(backend);
    backend.overview = async () => {
      const o = await real();
      return { ...o, apps: o.apps.map((a) => ({ ...a, permissions: { ...a.permissions, limits: shared } })) };
    };
    const { el } = await mount(undefined, backend);
    expect(Array.from(el.querySelectorAll('thead th')).map((h) => h.textContent!.trim())).not.toContain('Limits');
    expect(el.querySelector('.shared')!.textContent).toBe('Limits for every app: 4 requests at once');
  });

  it('says nothing under the table when the apps differ in their limits', async () => {
    const { el } = await mount();
    expect(el.querySelector('.shared')).toBeNull();
  });
});
