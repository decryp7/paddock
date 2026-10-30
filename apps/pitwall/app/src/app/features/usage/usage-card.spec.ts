import { TestBed } from '@angular/core/testing';
import { FakeChart, render, settle } from '../../../testing/render';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { UsageCard } from './usage-card';

async function mount(backend = new MockBackend()) {
  FakeChart.all = [];
  const r = await render(UsageCard, backend);
  await TestBed.inject(LiveStore).load();
  await settle(r.fixture);
  await new Promise((res) => requestAnimationFrame(() => res(null)));
  return r;
}

describe('UsageCard', () => {
  it('draws tokens per app over 24 hours with a legend of totals and peak memory', async () => {
    const { el } = await mount();
    expect(el.querySelectorAll('pw-echart').length).toBe(1);
    expect(el.querySelector('pw-echart')!.getAttribute('aria-label')).toContain('Tokens per app over the last 24 hours');
    const items = Array.from(el.querySelectorAll('.totals li'));
    expect(items.length).toBe(5);
    expect(items[0]!.textContent).toContain('Debrief');
    expect(items[0]!.textContent).toContain('tokens');
    expect(items[0]!.textContent).toContain('GB peak');
    expect(items[0]!.querySelector('a')!.getAttribute('href')).toBe('/apps?app=debrief');
    const option = FakeChart.all[0]!.options.at(-1)!.option as { series: { name: string }[] };
    expect(option.series.map((s) => s.name)).toContain('Debrief');
  });

  it('switches range and metric', async () => {
    const { fixture, el, backend } = await mount();
    const spy = vi.spyOn(backend, 'usageSeries');
    (Array.from(el.querySelectorAll('[aria-label="Usage range"] button')).find((b) => b.textContent!.trim() === '7d') as HTMLButtonElement).click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('7d', null);
    (Array.from(el.querySelectorAll('[aria-label="Usage metric"] button')).find((b) => b.textContent!.trim() === 'Requests') as HTMLButtonElement).click();
    await settle(fixture);
    expect(el.querySelector('pw-echart')!.getAttribute('aria-label')).toContain('Requests per app over the last 7 days');
  });

  it('invites the user to connect an app when nothing was used', async () => {
    const { el } = await mount(new MockBackend({ empty: true }));
    expect(el.querySelector('pd-empty-state')!.textContent).toContain('No usage yet');
    expect(el.querySelector('pw-echart')).toBeNull();
  });

  it('shows an inline error with Retry', async () => {
    const backend = new MockBackend({ failing: ['usageSeries'] });
    const { fixture, el } = await mount(backend);
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
    (backend as unknown as { failing: Set<string> }).failing.delete('usageSeries');
    (Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.includes('Retry')) as HTMLButtonElement).click();
    await settle(fixture);
    expect(el.querySelector('pw-echart')).not.toBeNull();
  });

  it('adds the cache tiles over the range and a hit rate to the apps that have one', async () => {
    const { el } = await mount();
    const tiles = el.querySelector('pw-cache-tiles')!;
    expect(tiles.textContent).toContain('Cache hit rate');
    expect(tiles.textContent).toContain('Tokens saved');
    expect(tiles.querySelector('.value')!.textContent).toMatch(/^\d+ %$/);
    const items = Array.from(el.querySelectorAll('.totals li'));
    expect(items[0]!.textContent).toContain('% cached');
    expect(items.some((li) => !li.textContent!.includes('cached'))).toBe(true);
  });
});
