import { Component, input, output } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { RecorderStore } from '../../state/recorder.store';
import { SettingsStore } from '../../state/settings.store';
import { Chart } from '../../shared/chart';
import { RecorderPage } from './recorder-page';
import { reasonText } from './snapshot-list';
import { TimelinePanel } from './timeline-panel';

/** jsdom has no canvas: the chart is replaced by an element with the same selector, inputs and outputs. */
@Component({ selector: 'dy-chart', template: '', host: { '[attr.aria-label]': 'label()' } })
class StubChart {
  readonly options = input<unknown>(null);
  readonly label = input('');
  readonly itemClick = output<unknown>();
}
const stubChart = () => TestBed.overrideComponent(TimelinePanel, { remove: { imports: [Chart] }, add: { imports: [StubChart] } });


function render(setup: (m: MockBackend) => void = () => undefined) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  setup(mock);
  stubChart();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
  const f = TestBed.createComponent(RecorderPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
    f.detectChanges();
  }
};
/** A key press the way a browser makes it (the CDK listbox reads `keyCode`). */
const press = (target: Element, key: string): void => {
  const keyCode = { ArrowDown: 40, ArrowUp: 38, Home: 36, End: 35 }[key] ?? 0;
  target.dispatchEvent(new KeyboardEvent('keydown', { key, keyCode, bubbles: true }));
};
const rows = (el: HTMLElement) => [...el.querySelectorAll<HTMLElement>('[role="option"]')];

describe('reasonText', () => {
  it('names manual captures and alarms', () => {
    expect(reasonText('manual')).toBe('Manual');
    expect(reasonText({ alarm: { alarmId: 1, title: 'CPU hot' } })).toBe('CPU hot');
  });
});

describe('dy-recorder-page', () => {
  it('lists snapshots newest first and shows the selected timeline with who was busiest', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Flight recorder');
    expect(rows(el)).toHaveLength(3);
    expect(rows(el)[0]!.getAttribute('aria-selected')).toBe('true');
    expect(rows(el)[1]!.textContent).toContain('Manual');
    expect(el.querySelector('dy-chart')?.getAttribute('aria-label')).toContain('one-second samples');
    expect(el.querySelectorAll('[aria-label="Who was busiest"] pd-table [role="row"][data-key]').length).toBe(3);
    expect(el.querySelectorAll('button[matButton="filled"]')).toHaveLength(1);
  });

  it('selects a row by click and by arrow keys', async () => {
    const { f, el } = render();
    await settle(f);
    const store = TestBed.inject(RecorderStore);
    rows(el)[1]!.click();
    await settle(f);
    expect(store.selectedId()).toBe(2);
    const list = el.querySelector('[role="listbox"]')!;
    press(list, 'ArrowDown');
    await settle(f);
    expect(store.selectedId()).toBe(1);
    press(list, 'End');
    press(list, 'ArrowUp');
    await settle(f);
    expect(store.selectedId()).toBe(2);
    press(list, 'Home');
    await settle(f);
    expect(store.selectedId()).toBe(3);
    press(list, 'x');
    expect(store.selectedId()).toBe(3);
  });

  it('moves the picked moment with the arrow keys (10 s with shift) and jumps to the peak', async () => {
    const { f, el } = render();
    await settle(f);
    const store = TestBed.inject(RecorderStore);
    const peak = store.moment();
    const plot = el.querySelector('.plot')!;
    plot.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    expect(store.moment()).toBe(peak - 1);
    plot.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', shiftKey: true, bubbles: true }));
    expect(store.moment()).toBe(peak + 9);
    plot.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', bubbles: true }));
    expect(store.moment()).toBe(peak + 9);
    [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('Jump to peak'))!.click();
    expect(store.moment()).toBe(peak);
  });

  it('captures on the one primary button', async () => {
    const { f, el } = render();
    await settle(f);
    (el.querySelector('button[matButton="filled"]') as HTMLButtonElement).click();
    await settle(f);
    expect(rows(el)).toHaveLength(4);
    expect(rows(el)[0]!.getAttribute('aria-selected')).toBe('true');
    expect(rows(el)[0]!.textContent).toContain('Manual');
  });

  it('shows the empty state', async () => {
    const { f, el } = render((m) => vi.spyOn(m, 'recorderSnapshots').mockResolvedValue([]));
    await settle(f);
    expect(el.textContent).toContain('Nothing saved yet. Capture now, or Dyno saves the last ten minutes when an alarm fires.');
    expect(rows(el)).toHaveLength(0);
  });

  it('shows a list error with Retry, then the list', async () => {
    const { f, el } = render((m) => vi.spyOn(m, 'recorderSnapshots').mockRejectedValueOnce({ code: 'internal', message: 'The recorder is not answering.' }));
    await settle(f);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('The recorder is not answering.');
    [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes('Retry'))!.click();
    await settle(f);
    expect(rows(el)).toHaveLength(3);
  });

  it('shows a timeline error inline with Retry', async () => {
    const { f, el } = render((m) => vi.spyOn(m, 'recorderSnapshot').mockRejectedValueOnce({ code: 'notFound', message: 'That snapshot was deleted.' }));
    await settle(f);
    expect(rows(el)).toHaveLength(3);
    expect(el.querySelector('.main [role="alert"]')?.textContent).toContain('That snapshot was deleted.');
    [...el.querySelectorAll<HTMLButtonElement>('.main button')].find((b) => b.textContent?.includes('Retry'))!.click();
    await settle(f);
    expect(el.querySelector('dy-timeline-panel')).not.toBeNull();
  });

  it('toggles continuous recording through the settings, and explains when it is off', async () => {
    const { f, el, mock } = render();
    await settle(f);
    expect(el.querySelector('.note')).toBeNull();
    const save = vi.spyOn(mock, 'saveSettings');
    (el.querySelector('[role="switch"]') as HTMLButtonElement).click();
    await settle(f);
    expect(save.mock.calls[0]![0].flightRecorder).toBe(false);
    expect(TestBed.inject(SettingsStore).settings()?.flightRecorder).toBe(false);
    expect(el.querySelector('.note')?.textContent).toContain('Recording is off');
    (el.querySelector('[role="switch"]') as HTMLButtonElement).click();
    await settle(f);
    expect(el.querySelector('.note')).toBeNull();
  });

  it('ignores a toggle before the settings have loaded', async () => {
    const { f, el, mock } = render();
    const save = vi.spyOn(mock, 'saveSettings');
    await (f.componentInstance as unknown as { setRecording(on: boolean): Promise<void> }).setRecording(false);
    expect(save).not.toHaveBeenCalled();
    await settle(f);
    expect(el).toBeTruthy();
  });
});
