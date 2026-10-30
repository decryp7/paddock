import { By } from '@angular/platform-browser';
import { MatTooltip } from '@angular/material/tooltip';
import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { Chart } from '../../shared/chart';
import { MockBackend } from '../../core/mock-backend';
import { CountersStore } from '../../state/counters.store';
import { SettingsStore } from '../../state/settings.store';
import { nextFrame } from '../../testing/frame';
import { CountersPage } from './counters-page';

const NOW = 1_700_000_000_000;

/** jsdom has no canvas, so ECharts cannot draw; the page's own logic is what is tested here (the chart option has its own spec). */
@Component({ selector: 'dy-chart', template: '', changeDetection: ChangeDetectionStrategy.OnPush })
class StubChart {
  readonly options = input<unknown>(null);
  readonly label = input('');
}

async function open(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, os: 'windows', fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
  TestBed.overrideComponent(CountersPage, { remove: { imports: [Chart] }, add: { imports: [StubChart] } });
  await TestBed.inject(SettingsStore).load();
  const f = TestBed.createComponent(CountersPage);
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      await new Promise((r) => setTimeout(r));
      await nextFrame();
      f.detectChanges();
    }
  };
  f.detectChanges();
  await settle();
  const el = f.nativeElement as HTMLElement;
  const click = async (sel: string, i = 0) => {
    (el.querySelectorAll(sel)[i] as HTMLElement).click();
    await settle();
  };
  return { f, el, click, settle, store: TestBed.inject(CountersStore), mock };
}

describe('CountersPage', () => {
  afterEach(() => vi.useRealTimers());

  it('lists the objects, opens one and charts a counter from it', async () => {
    const { el, click, store } = await open();
    expect(el.querySelector('h1')!.textContent).toBe('Counters');
    expect(el.querySelector('[aria-label="Objects"]')!.textContent).toContain('Memory');
    expect(el.textContent).toContain('Nothing charted yet');
    const memory = [...el.querySelectorAll('[aria-label="Objects"] button')].findIndex((b) => b.textContent!.includes('Memory'));
    await click('[aria-label="Objects"] button', memory);
    expect(el.querySelector('h2')!.textContent).toBe('Memory');
    await click('button[aria-label="Chart Pages/sec"]');
    expect(store.watching()).toEqual(['\\Memory\\Pages/sec']);
    const table = el.querySelector('[aria-label="Charted counters"][role="grid"]')!;
    expect(table.textContent).toContain('Pages/sec');
    expect(table.querySelector('b')!.textContent).toMatch(/\d/);
    expect(el.textContent).toContain('Network');
    await click('[aria-label="Charted counters"] [role="row"][data-key] button[aria-label="Stop charting"]');
    expect(el.textContent).toContain('Nothing charted yet');
  });

  it('opens an object with instances and charts the chosen instance', async () => {
    const { el, f, click, store } = await open();
    const idx = [...el.querySelectorAll('[aria-label="Objects"] button')].findIndex((b) => b.textContent!.includes('Network Interface'));
    await click('[aria-label="Objects"] button', idx);
    (el.querySelector('mat-select[aria-label="Instance"] .mat-mdc-select-trigger') as HTMLElement).click();
    f.detectChanges();
    ([...document.querySelectorAll('mat-option')].find((o) => o.textContent?.trim() === 'Wi-Fi') as HTMLElement).click();
    f.detectChanges();
    await click('button[aria-label="Chart Bytes Received/sec"]');
    expect(store.watching()).toEqual(['\\Network Interface(Wi-Fi)\\Bytes Received/sec']);
    await click('button[aria-label="Back to all objects"]');
    expect(el.querySelector('[aria-label="Objects"]')).not.toBeNull();
  });

  it('searches (after a short pause) and charts a hit', async () => {
    const { el, click, store, settle } = await open();
    vi.useFakeTimers();
    const input = el.querySelector('input[type="search"]') as HTMLInputElement;
    input.value = 'disk queue';
    input.dispatchEvent(new Event('input'));
    await vi.advanceTimersByTimeAsync(250);
    vi.useRealTimers();
    await settle();
    expect(el.querySelector('[aria-label="Search results"]')!.textContent).toContain('Avg. Disk Queue Length');
    await click('[aria-label="Search results"] button', 0);
    expect(store.watching()).toHaveLength(1);
    input.value = '';
    input.dispatchEvent(new Event('input'));
    await settle();
    expect(el.querySelector('[aria-label="Objects"]')).not.toBeNull();
  });

  it('says when nothing matches', async () => {
    const { el, f } = await open();
    await TestBed.inject(CountersStore).search('zzzzzz');
    f.detectChanges();
    expect(el.textContent).toContain('No counter matches');
  });

  it('pins from the row, shows pinned counters that are not charted and saves a set by name', async () => {
    const { el, f, click, store, settle } = await open();
    expect(el.querySelector('[aria-label="Pinned counters"][role="list"]')!.textContent).toContain('% Processor Time');
    await store.setWatching(['\\Memory\\Pages/sec']);
    f.detectChanges();
    await settle();
    await click('[aria-label="Charted counters"] [role="row"][data-key] button[aria-label="Pin"]');
    expect(store.isPinned('\\Memory\\Pages/sec')).toBe(true);
    await click('button[matButton="text"]', [...el.querySelectorAll('button[matButton="text"]')].findIndex((b) => b.textContent?.includes('Save as set')));
    const name = el.querySelector('input[aria-label="Set name"]') as HTMLInputElement;
    name.value = 'Memory only';
    name.dispatchEvent(new Event('input'));
    f.detectChanges();
    await click('.namebar button[matButton="filled"]');
    expect(store.sets().map((s) => s.name)).toContain('Memory only');
    expect(el.querySelector('.namebar')).toBeNull();
  });

  it('explains a counter that is gone instead of showing a number', async () => {
    const { el, f, store, settle } = await open();
    await store.setWatching(['\\Network Interface(USB Ethernet)\\Bytes Sent/sec']);
    f.detectChanges();
    await settle();
    const cell = el.querySelector('.unavail')!;
    expect(cell.textContent).toContain('Unavailable');
    const tip = f.debugElement.queryAll(By.css('.unavail'))[0]!.injector.get(MatTooltip);
    expect(tip.message).toMatch(/not available/);
  });

  it('shows an inline error with Retry when the objects cannot be listed', async () => {
    const { el } = await open(['counterObjects']);
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
    expect(el.textContent).toContain('Retry');
  });

  it('stops reading when the page closes', async () => {
    const { f, mock } = await open();
    const spy = vi.spyOn(mock, 'counterValues');
    f.destroy();
    vi.useFakeTimers();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(spy).not.toHaveBeenCalled();
  });
});
