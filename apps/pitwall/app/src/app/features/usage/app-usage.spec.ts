import { Component, signal } from '@angular/core';
import { FakeChart, render as renderWith, settle } from '../../../testing/render';
import { MockBackend } from '../../core/mock-backend';
import { AppUsage } from './app-usage';

@Component({ selector: 'pw-host', imports: [AppUsage], template: `<pw-app-usage [appId]="id()" appName="Spotter" />` })
class Host {
  id = signal('spotter');
}

async function mount(backend = new MockBackend()) {
  FakeChart.all = [];
  const r = await renderWith(Host, backend);
  await settle(r.fixture);
  return r;
}

const text = (el: Element) => el.textContent!.replace(/\s+/g, ' ');

describe('AppUsage', () => {
  it('shows the range totals, a chart and the limits with progress and words', async () => {
    const { el } = await mount();
    expect(text(el.querySelector('.figures')!)).toContain('Requests');
    expect(el.querySelectorAll('pw-echart').length).toBe(1);
    expect(el.querySelector('pw-echart')!.getAttribute('aria-label')).toContain('Spotter over the last 24 hours');
    const limits = Array.from(el.querySelectorAll('.limits li'));
    expect(limits.map((l) => l.querySelector('.name')!.textContent)).toEqual(['Requests per minute', 'Requests per day', 'Tokens per day', 'Tokens per month']);
    const bar = limits[2]!.querySelector('[role="progressbar"]')!;
    expect(bar.getAttribute('aria-label')).toBe('Tokens per day');
    expect(Number(bar.getAttribute('aria-valuenow'))).toBeGreaterThanOrEqual(0);
    // A limit within its budget says nothing in words; only the ones close to or over it carry a status.
    const flagged = limits.filter((l) => l.querySelector('.status'));
    expect(flagged.length).toBeLessThan(limits.length);
    for (const l of flagged) expect(l.querySelector('.status')!.textContent!.length).toBeGreaterThan(3);
  });

  it('lists the latest requests as metadata with a status in words', async () => {
    const { el } = await mount();
    const rows = el.querySelectorAll('table.recent tbody tr');
    expect(rows.length).toBeGreaterThan(0);
    // Successes carry no status; a failure says so in words.
    expect(text(rows[0]!)).not.toMatch(/ ok/);
    const failed = Array.from(rows).filter((r) => r.querySelector('.status'));
    for (const r of failed) expect(text(r)).toMatch(/\d+ failed/);
  });

  it('switches the range and the metric', async () => {
    const { fixture, el, backend } = await mount();
    const spy = vi.spyOn(backend, 'usageSeries');
    (Array.from(el.querySelectorAll('[aria-label="Usage range"] button')).find((b) => b.textContent!.trim() === '30d') as HTMLButtonElement).click();
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('30d', 'spotter');
    expect(el.querySelector('pw-echart')!.getAttribute('aria-label')).toContain('the last 30 days');
    const metrics = Array.from(el.querySelectorAll('[aria-label="Metric"] button')) as HTMLButtonElement[];
    expect(metrics.map((m) => m.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    metrics[2]!.click();
    await settle(fixture);
    await new Promise((r) => requestAnimationFrame(() => r(null)));
    expect(metrics[2]!.getAttribute('aria-checked')).toBe('true');
    expect(el.querySelector('pw-echart')!.getAttribute('aria-label')).toContain('Peak memory');
    const last = FakeChart.all[0]!.options.at(-1)!.option as { series: { name: string }[] };
    expect(last.series.map((s) => s.name)).toEqual(['Peak memory']);
  });

  it('says so when no limits are set', async () => {
    const r = await mount();
    r.fixture.componentInstance.id.set('pitcrew');
    await settle(r.fixture);
    expect(r.el.textContent).toContain('No limits set');
  });

  it('says so when an app has no requests yet', async () => {
    const r = await mount();
    const app = await r.backend.createApp('Fresh script');
    r.fixture.componentInstance.id.set(app.id);
    await settle(r.fixture);
    expect(r.el.textContent).toContain('No requests yet');
    expect(r.el.querySelector('.figures')!.textContent).toContain('Peak memory—');
  });

  it('shows an inline error with Retry when the series cannot load', async () => {
    const backend = new MockBackend({ failing: ['usageSeries'] });
    const { fixture, el } = await mount(backend);
    expect(el.querySelector('pw-panel [role="alert"]')).not.toBeNull();
    (backend as unknown as { failing: Set<string> }).failing.delete('usageSeries');
    (Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.includes('Retry')) as HTMLButtonElement).click();
    await settle(fixture);
    expect(el.querySelector('.figures')).not.toBeNull();
  });

  it('shows the cache tiles for the app over the range', async () => {
    const { el } = await mount();
    expect(el.querySelector('pw-cache-tiles')!.textContent).toContain('Cache hit rate');
    expect(el.querySelector('pw-cache-tiles .value')!.textContent).toMatch(/^\d+%$/);
  });
});
