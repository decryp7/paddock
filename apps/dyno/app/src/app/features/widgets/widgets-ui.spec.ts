import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { newWidget } from './widget-model';
import { DashboardLayoutStore } from '../../state/dashboard-layout.store';
import { LiveStore } from '../../state/live.store';
import { SettingsStore } from '../../state/settings.store';
import { TrayItemsEditor } from '../settings/tray-items-editor';
import { AnimatedNumber } from './animated-number';
import { WidgetBoard } from './widget-board';
import { WidgetView } from './widget-view';

function env() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  TestBed.inject(LiveStore).receive(mock.liveUpdate('summary'));
  return mock;
}

describe('dy-widget', () => {
  it('draws every kind from live data without errors', () => {
    env();
    for (const [kind, metric] of [['bigNumber', 'cpuTotal'], ['graph', 'networkDown'], ['bars', 'memory'], ['ring', 'memory'], ['heatmap', 'cpuPerCore'], ['topProcesses', 'memory'], ['issues', 'cpuTotal'], ['health', 'cpuTotal'], ['bigNumber', 'fan'], ['graph', 'fan']] as const) {
      const f = TestBed.createComponent(WidgetView);
      f.componentRef.setInput('widget', newWidget(kind, metric, []));
      f.detectChanges();
      expect((f.nativeElement as HTMLElement).getAttribute('data-kind')).toBe(kind);
      expect((f.nativeElement as HTMLElement).textContent!.length).toBeGreaterThan(0);
    }
  });

  it('colours a big number only when a threshold is crossed, with the number still printed', () => {
    env();
    const f = TestBed.createComponent(WidgetView);
    f.componentRef.setInput('widget', { ...newWidget('bigNumber', 'cpuTotal', []), thresholds: { warning: 1, critical: 99 } });
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.hero')!.getAttribute('data-tone')).toBe('warn');
    expect(el.querySelector('.hero')!.textContent).toMatch(/\d/);
  });
});

describe('AnimatedNumber', () => {
  it('sets text at once for non-numbers and the first value, and glides to the next', async () => {
    const f = TestBed.createComponent(AnimatedNumber);
    f.componentRef.setInput('value', '10');
    f.detectChanges();
    expect(f.nativeElement.textContent).toBe('10');
    f.componentRef.setInput('value', 'Normal');
    f.detectChanges();
    expect(f.nativeElement.textContent).toBe('Normal');
    f.componentRef.setInput('value', '20');
    f.detectChanges();
    f.componentRef.setInput('value', '30.5');
    f.detectChanges();
    for (let i = 0; i < 60 && f.nativeElement.textContent !== '30.5'; i++) {
      await new Promise((r) => setTimeout(r, 50));
      f.detectChanges();
    }
    expect(f.nativeElement.textContent).toBe('30.5');
  });
});

describe('dy-widget-board', () => {
  it('shows the widgets, enters edit mode, adds from the gallery and removes', async () => {
    env();
    await TestBed.inject(SettingsStore).load();
    const f = TestBed.createComponent(WidgetBoard);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelectorAll('dy-widget')).toHaveLength(7);
    [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Edit dashboard'))!.click();
    f.detectChanges();
    expect(el.querySelectorAll('.edit').length).toBe(7);
    [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Add widget'))!.click();
    f.detectChanges();
    expect(el.querySelectorAll('.gtile')).toHaveLength(8);
    (el.querySelectorAll('.gtile')[2] as HTMLButtonElement).click();
    f.detectChanges();
    expect(el.querySelectorAll('dy-widget')).toHaveLength(8);
    (el.querySelector('button[aria-label="Remove widget"]') as HTMLButtonElement).click();
    f.detectChanges();
    expect(el.querySelectorAll('dy-widget')).toHaveLength(7);
    (el.querySelectorAll('.seg button')[2] as HTMLButtonElement).click();
    f.detectChanges();
    expect(TestBed.inject(DashboardLayoutStore).dirty()).toBe(true);
    [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Cancel')!.click();
    f.detectChanges();
    expect(el.querySelector('.edit')).toBeNull();
  });
});

describe('dy-tray-items-editor', () => {
  it('adds, changes style and metric, reorders and removes items, up to four', () => {
    env();
    const f = TestBed.createComponent(TrayItemsEditor);
    let items = [{ id: 'cpu', metric: 'cpuTotal' as const, style: 'verticalBars' as const, opens: 'ownPopover' as const }];
    f.componentRef.setInput('items', items);
    f.componentInstance.itemsChange.subscribe((v) => {
      items = v as typeof items;
      f.componentRef.setInput('items', items);
      f.detectChanges();
    });
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    const add = () => [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Add item'))!.click();
    add();
    add();
    expect(items).toHaveLength(3);
    const styleSel = el.querySelectorAll<HTMLSelectElement>('select[aria-label^="Style"]')[0]!;
    styleSel.value = 'number';
    styleSel.dispatchEvent(new Event('change'));
    f.detectChanges();
    expect(items[0]!.style).toEqual({ number: { unit: '', label: '' } });
    expect(el.querySelectorAll('input[aria-label="Unit"]')).toHaveLength(1);
    const metricSel = el.querySelectorAll<HTMLSelectElement>('select[aria-label^="Metric"]')[1]!;
    metricSel.value = 'memory';
    metricSel.dispatchEvent(new Event('change'));
    expect(items[1]!.metric).toBe('memory');
    (el.querySelectorAll('button[aria-label="Move down"]')[0] as HTMLButtonElement).click();
    expect(items[1]!.metric).toBe('cpuTotal');
    add();
    add();
    expect(items).toHaveLength(4);
    expect(el.textContent).toContain('up to 4 items');
    (el.querySelector('button[aria-label="Remove item"]') as HTMLButtonElement).click();
    expect(items).toHaveLength(3);
    for (const style of ['lineGraph', 'graphAndNumber', 'horizontalMeter', 'ring']) {
      const sel = el.querySelectorAll<HTMLSelectElement>('select[aria-label^="Style"]')[0]!;
      sel.value = style;
      sel.dispatchEvent(new Event('change'));
      f.detectChanges();
      expect(el.querySelector('.preview')!.textContent).toBeDefined();
    }
  });
});
