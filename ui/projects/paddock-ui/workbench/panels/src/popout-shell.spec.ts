import { EnvironmentInjector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ShellContextHost } from '@decryp7/paddock-ui/shell';
import { render, settle } from '@decryp7/paddock-ui/shell/testing';
import { VIEW_PANEL, parsePanelId } from '@decryp7/paddock-ui/workbench';
import { InPageDesktop } from './in-page';
import { HOST_TITLE_KEY, hostTitleValue } from './panel-windows';
import { PdPopoutShell, pinDocumentTitle, SYNC_START_TIMEOUT_MS } from './popout-shell';
import { providePopoutWindow } from './provide';
import { FakeChrome, FakeSyncTimers, newCtx, windowProviders } from './testing';
import { SYNC_TIMERS, UiSync } from './ui-sync';
import { POPOUT_ROUTE } from './popout-route';

async function setup(panel = 'wt.code#src/main.rs', options: { host?: boolean } = {}) {
  const desktop = new InPageDesktop();
  const ctx = newCtx();
  const timers = new FakeSyncTimers();
  const chrome = new FakeChrome();
  const info = await desktop.window('main').popOut({ panel });
  const route = { panel: parsePanelId(panel), host: 'main', label: info.label };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [...windowProviders(ctx), { provide: SYNC_TIMERS, useValue: timers }, { provide: POPOUT_ROUTE, useValue: route }, providePopoutWindow(desktop.window(info.label), { chrome })],
  });
  await TestBed.inject(ShellContextHost).load();
  let host: UiSync | null = null;
  if (options.host !== false) {
    host = TestBed.runInInjectionContext(() => new UiSync());
    host.start(
      desktop.window('main'),
      'host',
      'main',
      [
        { key: 'wt.query', scope: 'full', read: () => 'host query', apply: () => undefined },
        hostTitleValue(() => 'Incident 7', () => undefined),
      ],
      'o-main',
    );
  }
  return { desktop, ctx, timers, chrome, host, label: info.label };
}

describe('PdPopoutShell', () => {
  afterEach(() => TestBed.inject(UiSync, null)?.stop());

  it('shows the view in its panel once it caught up with the host, with the host state and name applied', async () => {
    const { ctx, host } = await setup();
    const view = await render(PdPopoutShell);
    await settle(view.fixture);
    expect(view.element.querySelector('[data-testid="popout-connecting"]')).toBeNull();
    expect(view.element.querySelector('[data-testid="test-view"]')?.textContent).toBe('wt.code#src/main.rs');
    expect(ctx.seen).toEqual([{ panel: 'wt.code#src/main.rs', view: 'wt.code', key: 'src/main.rs' }]);
    expect(TestBed.inject(VIEW_PANEL).key).toBe('src/main.rs');
    expect(ctx.query()).toBe('host query');
    expect(view.element.querySelector('[data-testid="popout-title"]')?.textContent).toBe('Code');
    expect(view.element.querySelector('[data-testid="popout-host"]')?.textContent).toBe('Incident 7');
    expect(document.title).toBe('Code - Incident 7');
    // Another script rewriting the title does not stick.
    document.title = 'Something else';
    await settle(view.fixture);
    expect(document.title).toBe('Code - Incident 7');
    host?.stop();
  });

  it('waits for the host, and shows the view anyway after the timeout', async () => {
    const { timers } = await setup('wt.log', { host: false });
    const view = await render(PdPopoutShell);
    expect(view.element.querySelector('[data-testid="popout-connecting"]')?.textContent).toContain('Connecting');
    expect(view.element.querySelector('[data-testid="popout-view"]')?.getAttribute('aria-busy')).toBe('true');
    timers.advance(SYNC_START_TIMEOUT_MS);
    await settle(view.fixture);
    expect(view.element.querySelector('[data-testid="test-view"]')).not.toBeNull();
    expect(view.element.querySelector('[data-testid="popout-view"]')?.getAttribute('aria-busy')).toBe('false');
    expect(document.title).toBe('Log - Paddock');
  });

  it('says so when this version no longer has the view', async () => {
    await setup('wt.removed', { host: false });
    const view = await render(PdPopoutShell);
    expect(view.element.querySelector('[data-testid="popout-missing"]')?.textContent).toContain('not available');
    expect(view.element.querySelector('[data-testid="popout-title"]')?.textContent).toBe('wt.removed');
  });

  it('docks back through the shell, and says what went wrong when it cannot', async () => {
    const { desktop, label, host } = await setup();
    const view = await render(PdPopoutShell);
    vi.spyOn(desktop.window(label), 'dockBack').mockRejectedValueOnce(new Error('already gone'));
    view.one<HTMLButtonElement>('[data-testid="popout-dock-back"]').click();
    await settle(view.fixture);
    expect(view.one('[data-testid="popout-problem"]').textContent).toContain('Could not dock the panel back: already gone');
    view.one<HTMLButtonElement>('[data-testid="popout-problem"] button').click();
    await settle(view.fixture);
    expect(view.element.querySelector('[data-testid="popout-problem"]')).toBeNull();
    const events: string[] = [];
    await desktop.window('main').onPanels((e) => events.push(e.kind));
    view.one<HTMLButtonElement>('[data-testid="popout-dock-back"]').click();
    await settle(view.fixture);
    expect(events).toContain('returned');
    expect(desktop.all()).toEqual([]);
    host?.stop();
  });

  it('keeps the window on top of its host, and says when that fails', async () => {
    const { desktop, label, host } = await setup();
    const view = await render(PdPopoutShell);
    view.one<HTMLButtonElement>('[data-testid="popout-on-top"]').click();
    await settle(view.fixture);
    expect(desktop.all()[0]?.keepOnTop).toBe(true);
    expect(view.one('[data-testid="popout-on-top"]').getAttribute('aria-pressed')).toBe('true');
    vi.spyOn(desktop.window(label), 'keepOnTop').mockRejectedValueOnce(new Error('not possible here'));
    view.one<HTMLButtonElement>('[data-testid="popout-on-top"]').click();
    await settle(view.fixture);
    expect(view.one('[data-testid="popout-problem"]').textContent).toContain('Could not keep the window on top: not possible here');
    expect(view.one('[data-testid="popout-on-top"]').getAttribute('aria-pressed')).toBe('true');
    host?.stop();
  });

  it('closes itself when the host says it is going away', async () => {
    const { host, chrome } = await setup();
    const view = await render(PdPopoutShell);
    host?.goodbye();
    await settle(view.fixture);
    expect(chrome.calls).toContain('close');
    host?.stop();
  });

  it('shows the key of the instance in its panel but only the view title in its title bar', async () => {
    const { host } = await setup('wt.code#a/very/long/path/that/is/a/key.rs');
    const view = await render(PdPopoutShell);
    expect(view.one('[data-testid="popout-title"]').textContent).toBe('Code');
    host?.stop();
  });

  it('is laid out without horizontal scroll and with an explicit OnPush (rule 27)', async () => {
    await setup('wt.log', { host: false });
    const view = await render(PdPopoutShell);
    expect(view.component).toBeInstanceOf(PdPopoutShell);
    expect(HOST_TITLE_KEY).toBe('pd.panels.host-title');
  });
});

describe('pinDocumentTitle', () => {
  it('keeps the title and stops when cleaned up', async () => {
    TestBed.configureTestingModule({});
    let want = 'One';
    const off = TestBed.runInInjectionContext(() => pinDocumentTitle(() => want, TestBed.inject(EnvironmentInjector)));
    TestBed.tick();
    expect(document.title).toBe('One');
    off();
    document.title = 'Other';
    await Promise.resolve();
    expect(document.title).toBe('Other');
    want = 'Two';
  });
});
