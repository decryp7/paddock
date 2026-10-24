import { TestBed } from '@angular/core/testing';
import { ShellContextHost } from '@decryp7/paddock-ui/shell';
import { settle } from '@decryp7/paddock-ui/shell/testing';
import { DIALOG_ROUTE } from '@decryp7/paddock-ui/shell/dialogs';
import { PANEL_WINDOWS, VIEW_PANEL } from '@decryp7/paddock-ui/workbench';
import { InPageDesktop } from './in-page';
import { PANELS_IO } from './io';
import { PanelWindows } from './panel-windows';
import { POPOUT_ROUTE } from './popout-route';
import { PopoutState } from './popout-state';
import { provideDialogUiSync, providePanelWindows, providePopoutWindow } from './provide';
import { FakeSyncTimers, newCtx, windowProviders } from './testing';
import { SYNC_TIMERS, UiSync } from './ui-sync';

describe('providers', () => {
  beforeEach(() => TestBed.resetTestingModule());

  it('providePanelWindows gives the core port and starts the host at app start', async () => {
    const desktop = new InPageDesktop();
    await desktop.window('main').popOut({ panel: 'wt.log' });
    TestBed.configureTestingModule({ providers: [...windowProviders(newCtx()), { provide: SYNC_TIMERS, useValue: new FakeSyncTimers() }, providePanelWindows(desktop.window('main'))] });
    const windows = TestBed.inject(PanelWindows);
    expect(TestBed.inject(PANEL_WINDOWS)).toBe(windows);
    expect(TestBed.inject(PANELS_IO)).toBe(desktop.window('main'));
    await TestBed.inject(ShellContextHost).load();
    await windows.start();
    expect(windows.poppedOut()).toEqual(['wt.log']);
    expect(TestBed.inject(UiSync).started).toBe(true);
  });

  it('providePopoutWindow makes the window a guest: its panel, no pop-outs of its own, its own state', () => {
    const desktop = new InPageDesktop();
    const route = { panel: { panel: 'wt.code#k', view: 'wt.code', key: 'k' }, host: 'main', label: 'p-main-1' };
    TestBed.configureTestingModule({ providers: [{ provide: POPOUT_ROUTE, useValue: route }, providePopoutWindow(desktop.window('p-main-1'))] });
    expect(TestBed.inject(VIEW_PANEL)).toBe(route.panel);
    expect(TestBed.inject(PANEL_WINDOWS)).toBeNull();
    expect(TestBed.inject(PopoutState).keepOnTop()).toBe(false);
  });

  it('providePopoutWindow refuses a window that is not a pop-out', () => {
    TestBed.configureTestingModule({ providers: [{ provide: POPOUT_ROUTE, useValue: null }, providePopoutWindow(new InPageDesktop().window('main'))] });
    expect(() => TestBed.inject(VIEW_PANEL)).toThrow(/not a pop-out/);
  });

  it('provideDialogUiSync shares display state only, and only in a dialog window', async () => {
    const desktop = new InPageDesktop();
    const ctx = newCtx();
    TestBed.configureTestingModule({
      providers: [...windowProviders(ctx), { provide: SYNC_TIMERS, useValue: new FakeSyncTimers() }, { provide: DIALOG_ROUTE, useValue: { id: 'wt.about', host: 'main', label: 'd-main-1' } }, provideDialogUiSync(desktop.window('d-main-1'))],
    });
    await TestBed.inject(ShellContextHost).load();
    await settle();
    expect(TestBed.inject(UiSync).started).toBe(true);
    TestBed.inject(UiSync).stop();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [...windowProviders(newCtx()), { provide: DIALOG_ROUTE, useValue: null }, provideDialogUiSync(desktop.window('d-main-1'))] });
    await TestBed.inject(ShellContextHost).load();
    await settle();
    expect(TestBed.inject(UiSync).started).toBe(false);
  });
});

describe('mountPopoutWindowHost', () => {
  it('puts the pop-out shell in place of the main window root (regression: a pop-out window booted into an empty page)', async () => {
    const { mountPopoutWindowHost } = await import('./provide');
    const doc = document.implementation.createHTMLDocument('popout');
    doc.body.innerHTML = '<app-root><div class="pd-splash"></div></app-root>';
    const el = mountPopoutWindowHost(doc);
    expect(el.tagName.toLowerCase()).toBe('pd-popout-shell');
    expect(doc.body.children).toHaveLength(1);
    expect(doc.querySelector('app-root')).toBeNull();
    const own = mountPopoutWindowHost();
    expect(document.body.firstElementChild).toBe(own);
    own.remove();
  });
});
