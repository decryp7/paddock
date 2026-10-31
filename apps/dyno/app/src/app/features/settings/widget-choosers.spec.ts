import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { WidgetChoicesStore } from '../../state/widget-choices';
import { MenuBarItems } from './menu-bar-items';
import { PopupWidgets, newPopoverWidget } from './popup-widgets';

const NOW = 1_700_000_000_000;

function setup<T>(component: new () => T) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(component);
  f.detectChanges();
  const el = f.nativeElement as HTMLElement;
  return { f, mock, el };
}
const rows = (el: HTMLElement) => [...el.querySelectorAll('ul.pd-wc-list')[0]!.querySelectorAll('li.pd-wc-item')].map((li) => li.querySelector('.pd-wc-txt b')?.textContent);
const button = (el: HTMLElement, text: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text)!;

describe('dy-popup-widgets', () => {
  it('lists the popup in the engine order, with the volume choice and a preview of the real popup', async () => {
    const { f, el } = setup(PopupWidgets);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toEqual(['Active alarms', 'Disk space', 'CPU and memory']);
    });
    expect(el.querySelector('mat-select[aria-label="Volume of disk-space"]')).not.toBeNull();
    expect(el.querySelector('[aria-label="Preview of the popup"] pd-tray-popover pd-widget-area')).not.toBeNull();
  });

  it('adds a widget through the engine, and the preview follows', async () => {
    const { f, mock, el } = setup(PopupWidgets);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(button(el, 'Add')).toBeDefined();
    });
    el.querySelector<HTMLButtonElement>('button[aria-label="Add Network"]')!.click();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toContain('Network');
    });
    expect(el.querySelectorAll('[aria-label="Preview of the popup"] pd-widget-frame')).toHaveLength(4);
    expect(mock.widgets.view('popover')).toBeDefined();
  });

  it('removes a widget and offers Undo while the engine can undo', async () => {
    const { f, el } = setup(PopupWidgets);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(3);
    });
    el.querySelector<HTMLButtonElement>('button[aria-label="More actions for Active alarms"]')!.click();
    f.detectChanges();
    [...document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-item')].find((b) => b.textContent?.includes('Remove'))!.click();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toEqual(['Disk space', 'CPU and memory']);
    });
    await vi.waitFor(() => expect(document.querySelector('.pd-undo-snack')).not.toBeNull());
  });

  it('builds a default widget for a kind', () => {
    expect(newPopoverWidget('diskSpace')).toEqual({ diskSpace: { volume: 'all' } });
    expect(newPopoverWidget('network')).toBe('network');
  });
});

describe('dy-menu-bar-items', () => {
  it('lists the items with their metric and adds one, up to the engine limit', async () => {
    const { f, el } = setup(MenuBarItems);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toEqual(['Bars', 'Bars']);
    });
    expect(el.querySelectorAll('mat-select[aria-label^="Metric of"]')).toHaveLength(2);
    el.querySelector<HTMLButtonElement>('button[aria-label="Add another Number"], button[aria-label="Add Number"]')!.click();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(3);
    });
    expect(el.querySelector('input[maxlength="8"]')).not.toBeNull();
  });

  it('takes the default item and the metrics from the engine choices', async () => {
    const { f, el } = setup(MenuBarItems);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(2);
    });
    const choices = TestBed.inject(WidgetChoicesStore);
    expect(choices.menuBarDefault('ring')).toEqual({ ring: { metric: 'cpuTotal' } });
    expect(choices.menuBarMetrics().map((m) => m.title)).toContain('Network down');
  });
});
