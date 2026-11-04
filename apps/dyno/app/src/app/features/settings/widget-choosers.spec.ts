import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { WidgetChoicesStore } from '../../state/widget-choices';
import { WidgetLayouts } from '../../state/widget-layouts';
import { MenuBarItems } from './menu-bar-items';
import { PopupWidgets } from './popup-widgets';

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
    expect(el.querySelector('mat-select[aria-label="Volume of Disk space, position 2 of 3"]')).not.toBeNull();
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

  it('takes the default widget of a kind from the engine choices', async () => {
    setup(PopupWidgets);
    const choices = TestBed.inject(WidgetChoicesStore);
    await choices.load();
    expect(choices.popoverDefault('diskSpace')).toEqual({ diskSpace: { volume: 'all' } });
    expect(choices.popoverDefault('network')).toBe('network');
  });
});

describe('dy-menu-bar-items', () => {
  it('lists the items with their metric and adds one, up to the engine limit', async () => {
    const { f, el } = setup(MenuBarItems);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toEqual(['Number', 'Number', 'Number', 'Number']);
    });
    expect(el.querySelectorAll('mat-select[aria-label^="Metric of"]')).toHaveLength(4);
    expect(el.querySelector('[role="note"]')).toBeNull();
    el.querySelector<HTMLButtonElement>('button[aria-label="Add another Number"], button[aria-label="Add Number"]')!.click();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(5);
    });
    expect(el.querySelector('input[maxlength="8"]')).not.toBeNull();
    // Past four items the page says what the Mac menu bar and the Windows 11 overflow do with items that do not fit.
    expect(el.querySelector('[role="note"]')?.textContent).toMatch(/menu bar hides.*overflow flyout/s);
  });

  it('sets a number item to one line and back', async () => {
    const { f, el } = setup(MenuBarItems);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(4);
    });
    const layouts = TestBed.inject(WidgetLayouts);
    const first = layouts.menuBar()!.layout.placements[0]!;
    const page = f.componentInstance as unknown as { setFormat(p: typeof first, v: string): void };
    page.setFormat(first, 'line');
    await vi.waitFor(() => {
      const now = layouts.menuBar()!.layout.placements[0]!.widget as { number: { format: string } };
      expect(now.number.format).toBe('line');
    });
    page.setFormat(layouts.menuBar()!.layout.placements[0]!, 'stacked');
    await vi.waitFor(() => {
      const now = layouts.menuBar()!.layout.placements[0]!.widget as { number: { format: string } };
      expect(now.number.format).toBe('stacked');
    });
  });

  it('disables every add button once the menu bar holds its 32 items', async () => {
    const ctx = setup(MenuBarItems);
    const { f, el } = ctx;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(4);
    });
    // The engine takes the first 27 items directly: adding them one by one through the page re-renders a growing list of 5 to 32 rows
    // of Material fields 28 times, which is slow enough on a loaded runner to pass the test's 5 s (CI run 7501). The last one goes
    // through the page's own button, so the step that reaches the limit is the one the user takes.
    const { mock } = ctx;
    for (let n = 5; n <= 31; n++) {
      mock.widgets.edit({ menuBar: { add: { widget: { number: { metric: 'cpuTotal', unit: '', label: '', format: 'stacked' } }, span: null, at: null } } });
    }
    TestBed.inject(WidgetLayouts).accept(mock.widgets.view('menuBar'));
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(31);
    });
    el.querySelector<HTMLButtonElement>('button[aria-label="Add another Number"], button[aria-label="Add Number"]')!.click();
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(32);
    });
    const adds = [...el.querySelectorAll<HTMLButtonElement>('li.pd-wc-add button')];
    expect(adds.length).toBeGreaterThan(0);
    expect(adds.every((b) => b.disabled)).toBe(true);
    expect(el.querySelector('.pd-wc-limit')?.textContent).toContain('holds 32 widgets');
  });

  it('an edit made from a row that is one edit behind keeps the first edit (metric, then unit)', async () => {
    const { f, el } = setup(MenuBarItems);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(4);
    });
    el.querySelector<HTMLButtonElement>('button[aria-label="Add another Number"], button[aria-label="Add Number"]')!.click();
    const layouts = TestBed.inject(WidgetLayouts);
    await vi.waitFor(() => expect(layouts.menuBar()?.layout.placements).toHaveLength(5));
    const behind = layouts.menuBar()!.layout.placements[4]!;
    const page = f.componentInstance as unknown as { setMetric(p: typeof behind, title: string): void; setNumber(p: typeof behind, key: 'unit', value: string): void };
    // The row the person edits is the one drawn before the first edit arrived: both edits are made from it.
    page.setMetric(behind, 'Temperature');
    page.setNumber(behind, 'unit', '°C');
    await vi.waitFor(() => {
      const now = layouts.menuBar()!.layout.placements[4]!.widget as { number: { metric: unknown; unit: string } };
      expect(now.number.unit).toBe('°C');
      expect(now.number.metric).toBe('temperature');
    });
  });

  it('takes the default item and the metrics from the engine choices', async () => {
    const { f, el } = setup(MenuBarItems);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(4);
    });
    const choices = TestBed.inject(WidgetChoicesStore);
    expect(choices.menuBarDefault('ring')).toEqual({ ring: { metric: 'cpuTotal' } });
    expect(choices.menuBarMetrics().map((m) => m.title)).toContain('Network down');
  });
});

describe('dy-menu-bar-items: what the tray draws, and the limit', () => {
  it('says the limit the engine holds, once, and the counter agrees', async () => {
    const { f, el } = setup(MenuBarItems);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(4);
    });
    const limit = TestBed.inject(WidgetLayouts).menuBar()!.catalogue.surface.maxPlacements;
    expect(el.querySelector('p.sub')?.textContent).toContain(`Add up to ${limit},`);
    expect(el.querySelector('.pd-wc-count')?.textContent?.trim()).toBe(`4 of ${limit}`);
    expect(el.textContent).not.toMatch(/up to four/i);
  });

  it('offers both formats where the tray draws both', async () => {
    const { f, el } = setup(MenuBarItems);
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(4);
    });
    const trigger = el.querySelector<HTMLElement>('mat-select[aria-label^="Format of"]')!;
    expect(trigger.getAttribute('aria-disabled')).not.toBe('true');
    expect(el.querySelector('[role="note"]')).toBeNull();
  });

  it('says why a small Windows tray has no format to choose once, in the note, and not on every row', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW, os: 'windows', trayPx: 16 });
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(MenuBarItems);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(4);
    });
    // No select to contradict the note: the row says what is drawn.
    expect(el.querySelector('mat-select[aria-label^="Format of"]')).toBeNull();
    expect(el.querySelectorAll('.fixed')).toHaveLength(0);
    expect(el.querySelector('[role="note"]')?.textContent).toContain('16 px, too small for a label');
    expect(el.textContent).not.toContain('Label over value');
    // Label and Unit are one group.
    expect(el.querySelectorAll('.pair input')).toHaveLength(8);
  });

  it('has no format select where the Windows icon has room for the label, and the note says the one layout', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW, os: 'windows', trayPx: 24 });
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(MenuBarItems);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(4);
    });
    expect(el.querySelector('mat-select[aria-label^="Format of"]')).toBeNull();
    expect(el.querySelectorAll('.fixed')).toHaveLength(0);
    expect(el.querySelector('[role="note"]')?.textContent).toContain('one number layout');
  });

  it('reads the choices again when Settings opens', async () => {
    const { f, mock, el } = setup(MenuBarItems);
    const spy = vi.spyOn(mock, 'widgetChoices');
    await vi.waitFor(() => {
      f.detectChanges();
      expect(rows(el)).toHaveLength(4);
    });
    const before = spy.mock.calls.length;
    await TestBed.inject(WidgetChoicesStore).reload();
    expect(spy.mock.calls.length).toBe(before + 1);
  });
});
