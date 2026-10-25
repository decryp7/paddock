import { PD_ICON_PATHS } from '@decryp7/paddock-ui/icons';
import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideShellTesting, render, settle } from '@decryp7/paddock-ui/shell/testing';
import { WINDOW_CHROME } from './chrome';
import { InPageDesktop } from './in-page';
import { PANELS_IO } from './io';
import { POPOUT_ROUTE } from './popout-route';
import { PdPopoutTitleBar } from './popout-title-bar';
import { FakeChrome, FakeSyncTimers } from './testing';
import { SYNC_TIMERS, UiSync } from './ui-sync';

@Component({ changeDetection: ChangeDetectionStrategy.OnPush, imports: [PdPopoutTitleBar], template: `<pd-popout-title-bar title="Inspector" [path]="path()" host="Incident" [onTop]="onTop()" (dockBack)="docked = docked + 1" (toggleOnTop)="onTop.set(!onTop())" />` })
class Host {
  readonly onTop = signal(false);
  readonly path = signal('src/main.rs');
  docked = 0;
}

async function setup(chrome = new FakeChrome()) {
  const desktop = new InPageDesktop();
  await desktop.window('main').popOut({ panel: 'wt.log' });
  const io = desktop.window('p-main-1');
  TestBed.configureTestingModule({
    providers: [
      ...provideShellTesting(),
      UiSync,
      { provide: SYNC_TIMERS, useValue: new FakeSyncTimers() },
      { provide: PANELS_IO, useValue: io },
      { provide: WINDOW_CHROME, useValue: chrome },
      { provide: POPOUT_ROUTE, useValue: { panel: { panel: 'wt.log', view: 'wt.log', key: null }, host: 'main', label: 'p-main-1' } },
    ],
  });
  const view = await render(Host);
  const button = (id: string) => view.element.querySelector<HTMLButtonElement>(`[data-testid="${id}"]`);
  const sync = TestBed.inject(UiSync);
  sync.start(io, 'popout', 'p-main-1', []);
  return { view, chrome, button, sync, desktop };
}

const pointer = (type: string, init: PointerEventInit = {}) => new PointerEvent(type, { bubbles: true, button: 0, detail: 1, ...init });

describe('PdPopoutTitleBar', () => {
  afterEach(() => TestBed.inject(UiSync).stop());

  it('shows the panel and host names and offers Dock back and Keep on top', async () => {
    const { view, button } = await setup();
    expect(view.element.querySelector('[data-testid="popout-title"]')?.textContent).toBe('Inspector');
    expect(view.element.querySelector('[data-testid="popout-host"]')?.textContent).toBe('Incident');
    button('popout-dock-back')!.click();
    expect(view.component.docked).toBe(1);
    const onTop = button('popout-on-top')!;
    expect(onTop.getAttribute('aria-pressed')).toBe('false');
    onTop.click();
    await settle(view.fixture);
    expect(button('popout-on-top')!.getAttribute('aria-pressed')).toBe('true');
    expect(button('popout-on-top')!.title).toContain('(on)');
  });

  it('draws Dock back with the dock-in glyph and Keep on top with layers, tinted when on', async () => {
    const { view, button } = await setup();
    const d = (id: string) => button(id)!.querySelector('svg path')!.getAttribute('d');
    expect(d('popout-dock-back')).toBe(PD_ICON_PATHS['dock-in']);
    expect(d('popout-on-top')).toBe(PD_ICON_PATHS['layers']);
    button('popout-on-top')!.click();
    await settle(view.fixture);
    expect(button('popout-on-top')!.classList.contains('pd-on')).toBe(true);
    const css = Array.from(document.querySelectorAll('style')).map((st) => st.textContent).join('');
    expect(css).toMatch(/\.pd-btn\.pd-on[^{]*\{[^}]*background: var\(--pd-accent-tint\)/);
  });

  it('draws minimise, maximise and close and the resize edges where the window has no frame', async () => {
    const { view, button, chrome } = await setup();
    button('popout-minimise')!.click();
    button('popout-maximise')!.click();
    await settle(view.fixture);
    expect(chrome.calls).toEqual(['minimize', 'toggleMaximize']);
    expect(button('popout-maximise')!.getAttribute('aria-label')).toBe('Restore');
    expect(view.element.querySelectorAll('.pd-edge')).toHaveLength(0);
    button('popout-close')!.click();
    expect(chrome.calls.at(-1)).toBe('close');
    button('popout-maximise')!.click();
    await settle(view.fixture);
    const edges = view.element.querySelectorAll<HTMLElement>('.pd-edge');
    expect(edges).toHaveLength(8);
    edges[7]!.dispatchEvent(pointer('pointerdown'));
    edges[0]!.dispatchEvent(pointer('pointerdown', { button: 2 }));
    expect(chrome.calls.at(-1)).toBe('resize:SouthEast');
    expect(chrome.calls.filter((c) => c.startsWith('resize')).length).toBe(1);
    // A resize that changes the maximised state is noticed.
    chrome.maximized = true;
    chrome.resizeHandlers.forEach((h) => h());
    await settle(view.fixture);
    expect(button('popout-maximise')!.getAttribute('aria-label')).toBe('Restore');
  });

  it('leaves the frame to macOS and the browser, and hides Keep on top where it cannot work', async () => {
    const mac = await setup(new FakeChrome('overlay'));
    expect(mac.button('popout-minimise')).toBeNull();
    expect(mac.view.element.querySelector('.pd-edge')).toBeNull();
    TestBed.resetTestingModule();
    const browser = await setup(new FakeChrome('browser', false));
    expect(browser.button('popout-on-top')).toBeNull();
    expect(browser.button('popout-close')).toBeNull();
  });

  it('double-click maximises (not on a button, not in a browser)', async () => {
    const { view, chrome } = await setup();
    const bar = view.element.querySelector<HTMLElement>('[data-testid="popout-bar"]')!;
    bar.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await settle(view.fixture);
    expect(chrome.calls).toEqual(['toggleMaximize']);
    view.element.querySelector<HTMLElement>('[data-testid="popout-dock-back"]')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    expect(chrome.calls).toEqual(['toggleMaximize']);
    TestBed.resetTestingModule();
    const browser = await setup(new FakeChrome('browser'));
    browser.view.element.querySelector<HTMLElement>('[data-testid="popout-bar"]')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    expect(browser.chrome.calls).toEqual([]);
  });

  it('pressing the bar lets the OS move the window and tells the host when it was dropped', async () => {
    const { view, chrome, desktop } = await setup();
    const commands: string[] = [];
    const host = TestBed.runInInjectionContext(() => new UiSync());
    host.start(desktop.window('main'), 'host', 'main', []);
    host.onCommand((c, a) => void commands.push(`${c}:${a}`));
    const bar = view.element.querySelector<HTMLElement>('[data-testid="popout-bar"]')!;
    bar.dispatchEvent(pointer('pointerdown'));
    await settle(view.fixture);
    expect(chrome.calls).toEqual(['startDragging']);
    expect(commands).toEqual(['dragEnd:p-main-1']);
    // Not for a second click, another button, a press on a button, or a platform that cannot.
    bar.dispatchEvent(pointer('pointerdown', { detail: 2 }));
    bar.dispatchEvent(pointer('pointerdown', { button: 2 }));
    view.element.querySelector<HTMLElement>('[data-testid="popout-dock-back"]')!.dispatchEvent(pointer('pointerdown'));
    chrome.dragged = false;
    bar.dispatchEvent(pointer('pointerdown'));
    await settle(view.fixture);
    expect(chrome.calls).toEqual(['startDragging', 'startDragging']);
    expect(commands).toEqual(['dragEnd:p-main-1']);
    host.stop();
  });

  it('in a browser the bar reports its own pointer to the host', async () => {
    const { view, desktop } = await setup(new FakeChrome('browser'));
    const commands: string[] = [];
    const host = TestBed.runInInjectionContext(() => new UiSync());
    host.start(desktop.window('main'), 'host', 'main', []);
    host.onCommand((c, a) => void commands.push(`${c}:${a}`));
    const bar = view.element.querySelector<HTMLElement>('[data-testid="popout-bar"]')!;
    bar.dispatchEvent(pointer('pointerdown', { pointerId: 7 }));
    bar.dispatchEvent(pointer('pointermove', { screenX: 30, screenY: 40 }));
    bar.dispatchEvent(pointer('pointerup'));
    bar.dispatchEvent(pointer('pointermove', { screenX: 99, screenY: 99 }));
    await settle(view.fixture);
    expect(commands).toEqual(['dragOver:{"label":"p-main-1","x":30,"y":40}', 'dragEnd:p-main-1']);
    host.stop();
  });

  const allCss = () => Array.from(document.querySelectorAll('style')).map((st) => st.textContent).join('');

  it('keeps the title bar on the 12 px pane inset, flush at the right only where the window draws its own controls', async () => {
    const frameless = await setup();
    expect(allCss()).toMatch(/\.pd-bar[^{]*\{[^}]*padding-inline: var\(--pd-space-3\)/);
    expect(allCss()).toMatch(/\.pd-frameless[^{]*\.pd-bar[^{]*\{\s*padding-inline-end: 0/);
    expect(frameless.view.element.querySelector('pd-popout-title-bar')?.classList.contains('pd-frameless')).toBe(true);
    TestBed.resetTestingModule();
    const mac = await setup(new FakeChrome('overlay'));
    expect(mac.view.element.querySelector('pd-popout-title-bar')?.classList.contains('pd-frameless')).toBe(false);
  });

  it('puts the panel path in the tooltip of the title, and none for a view without instances', async () => {
    const { view } = await setup();
    expect(view.element.querySelector('[data-testid="popout-title"]')!.getAttribute('title')).toBe('src/main.rs');
    view.component.path.set('');
    await settle(view.fixture);
    expect(view.element.querySelector('[data-testid="popout-title"]')!.hasAttribute('title')).toBe(false);
  });

  it('dims while the window is not active, and the dimmed bar loses its accent line and mutes its text', async () => {
    const { view } = await setup();
    expect(allCss()).toMatch(/\.pd-inactive[^{]*\.pd-bar[^{]*\{[^}]*box-shadow: none;[^}]*color: var\(--pd-text-muted\)/);
    window.dispatchEvent(new Event('blur'));
    await settle(view.fixture);
    expect(view.element.querySelector('pd-popout-title-bar')?.classList.contains('pd-inactive')).toBe(true);
    window.dispatchEvent(new Event('focus'));
    await settle(view.fixture);
    expect(view.element.querySelector('pd-popout-title-bar')?.classList.contains('pd-inactive')).toBe(false);
  });
});
