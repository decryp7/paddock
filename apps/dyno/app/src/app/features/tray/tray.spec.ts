import { CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { By } from '@angular/platform-browser';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend, type MockOptions } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { QuitStore } from '../../state/quit.store';
import { Tray } from './tray';

const NOW = 1_700_000_000_000;

async function make(options: MockOptions = {}) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([]), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(Tray);
  f.detectChanges();
  const el = f.nativeElement as HTMLElement;
  await vi.waitFor(() => {
    f.detectChanges();
    expect(el.querySelectorAll('pd-widget-frame').length).toBeGreaterThan(0);
  });
  return { f, mock, el };
}
const titles = (el: HTMLElement) => [...el.querySelectorAll('pd-widget-frame section, pd-widget-frame [role="group"]')].map((e) => e.getAttribute('aria-label') ?? '');
const frames = (el: HTMLElement) => [...el.querySelectorAll('pd-widget-frame')];

describe('dy-tray', () => {
  it('draws the engine layout in order: alarms, disk space, CPU and memory, with their values', async () => {
    const { el } = await make();
    const text = frames(el).map((f) => f.textContent ?? '');
    expect(text).toHaveLength(3);
    expect(text[0]).toContain('Active alarms');
    expect(text[0]).toContain('Memory pressure is high');
    expect(text[1]).toContain('Disk space');
    expect(text[1]).toContain('Macintosh HD');
    expect(text[2]).toContain('CPU and memory');
    expect(titles(el).length).toBeGreaterThanOrEqual(0);
  });

  it('lists every alarm and volume the engine sends, scrolling inside the popover and never ending in "+N more"', async () => {
    const { f, el } = await make({ longLists: true });
    const text = el.textContent ?? '';
    expect(text).not.toMatch(/\+\d+ more/);
    const alarms = el.querySelector<HTMLElement>('[aria-label="Active alarms"]')!;
    expect(alarms.tagName.toLowerCase()).toBe('cdk-virtual-scroll-viewport');
    expect(alarms.getAttribute('tabindex')).toBe('0');
    const viewport = f.debugElement.query(By.directive(CdkVirtualScrollViewport)).componentInstance as CdkVirtualScrollViewport;
    expect(viewport.getDataLength()).toBe(200);
    // The engine's bound cuts the list, and the popover says so honestly, with the way to the full page.
    expect(text).toContain('Showing the 200 most severe of 205.');
    expect([...el.querySelectorAll('button')].some((b) => b.textContent?.trim() === 'Open alarms')).toBe(true);
    const volumes = el.querySelector<HTMLElement>('ul[aria-label="Volumes"]')!;
    expect(volumes.getAttribute('tabindex')).toBe('0');
    expect(volumes.querySelectorAll('li')).toHaveLength(30);
    // The virtual rows say where they are in the whole list.
    const first = alarms.querySelector('[role="listitem"]')!;
    expect(first.getAttribute('aria-setsize')).toBe('200');
    expect(first.getAttribute('aria-posinset')).toBe('1');
  });

  it('follows a change of the layout made elsewhere, keeping the engine order', async () => {
    const { f, mock, el } = await make();
    mock.widgets.edit({ popover: { move: { id: 'cpu-memory', to: 0 } } });
    await vi.waitFor(() => {
      f.detectChanges();
      expect(frames(el)[0]!.textContent).toContain('CPU and memory');
    });
  });

  it('shows the agent notice once, with Restart agent running the engine action', async () => {
    const { f, mock, el } = await make({ widgets: { agentStopped: true } });
    const notice = el.querySelectorAll('.pd-wa-notice');
    expect(notice).toHaveLength(1);
    const restart = [...el.querySelectorAll<HTMLButtonElement>('.pd-wa-notice button')].find((b) => b.textContent?.includes('Restart agent'))!;
    restart.click();
    await vi.waitFor(() => expect(mock.widgetActions).toContainEqual('restartAgent'));
    f.detectChanges();
  });

  it('shows a calm alarms row when there are none, a retry on error and the next step when empty', async () => {
    const { f, mock, el } = await make({ widgets: { popup: { activeAlarms: 'empty', diskSpace: 'error', cpuMemory: 'ready' } } });
    expect(frames(el)[0]!.textContent).toContain('No active alarms');
    const retry = [...frames(el)[1]!.querySelectorAll('button')].find((b) => b.textContent?.includes('Retry'))!;
    retry.click();
    await vi.waitFor(() => expect(mock.widgetActions).toContainEqual({ retry: 'disk-space' }));
    f.detectChanges();
  });

  it('uses the shared popover footer: Open Dyno shows the main window, the ⋯ menu has its icons and one separator before Quit', async () => {
    const { f, mock, el } = await make();
    expect(el.querySelector('.foot')).toBeNull();
    const open = el.querySelector<HTMLButtonElement>('pd-tray-popover footer .pd-tray-primary')!;
    expect(open.textContent?.trim()).toBe('Open Dyno');
    const show = vi.spyOn(mock, 'showMainWindow');
    open.click();
    await vi.waitFor(() => expect(show).toHaveBeenCalledTimes(1));
    el.querySelector<HTMLButtonElement>('pd-tray-popover footer .pd-tray-more')!.click();
    f.detectChanges();
    const panel = document.querySelector('.mat-mdc-menu-panel')!;
    expect([...panel.querySelectorAll('[role="menuitem"] mat-icon')].map((i) => i.getAttribute('data-mat-icon-name'))).toEqual(['sliders', 'settings', 'power', 'power']);
    expect(panel.querySelectorAll('hr.pd-tray-sep')).toHaveLength(1);
    expect(panel.querySelector('hr.pd-tray-sep')!.nextElementSibling?.textContent?.trim()).toBe('Quit Dyno');
  });

  it('has a footer menu with Customize widgets, Settings and Quit, and shows no shortcuts the popover does not bind', async () => {
    const { f, mock, el } = await make();
    const more = el.querySelector<HTMLButtonElement>('button[aria-label="More actions"]')!;
    more.click();
    f.detectChanges();
    const items = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    expect(items.map((i) => i.textContent?.trim())).toEqual(['Customize widgets…', 'Settings…', 'Quit Dyno', 'Quit and stop measuring']);
    expect(document.querySelector('.mat-mdc-menu-panel kbd')).toBeNull();
    items[0]!.click();
    await vi.waitFor(() => expect(mock.widgetActions).toContainEqual('customizeWidgets'));
    more.click();
    f.detectChanges();
    [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')][1]!.click();
    await vi.waitFor(() => expect(mock.widgetActions).toContainEqual('openSettings'));
  });

  it('an empty layout offers Choose widgets', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    for (const id of ['active-alarms', 'disk-space', 'cpu-memory']) mock.widgets.edit({ popover: { remove: { id } } });
    TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([]), { provide: BACKEND, useValue: mock }] });
    const f = TestBed.createComponent(Tray);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(el.textContent).toContain('Choose what the popup shows');
    });
    el.querySelector<HTMLButtonElement>('.pd-wa-ea')!.click();
    await vi.waitFor(() => expect(mock.widgetActions).toContainEqual('customizeWidgets'));
  });

  it('says Quit Dyno (keeps measuring) when the engine says a service keeps the agent, and Quit and stop measuring asks the engine', async () => {
    const { f, mock, el } = await make();
    mock.keepsMeasuring = true;
    await TestBed.inject(QuitStore).refresh();
    el.querySelector<HTMLButtonElement>('button[aria-label="More actions"]')!.click();
    f.detectChanges();
    await vi.waitFor(() => {
      f.detectChanges();
      expect([...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].map((i) => i.textContent?.trim())).toContain('Quit Dyno (keeps measuring)');
    });
    const stop = vi.spyOn(mock, 'quitAndStopAgent');
    [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((i) => i.textContent?.includes('Quit and stop measuring'))!.click();
    await vi.waitFor(() => expect(stop).toHaveBeenCalledTimes(1));
  });
});
