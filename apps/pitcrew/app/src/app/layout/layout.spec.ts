import { Dialog } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../core/mock-backend';
import { ServiceStore } from '../state/service.store';
import { StatusStore } from '../state/status.store';
import { ToastStore } from '../state/toast.store';
import { appProviders, byText, flush, makeMock, settle, text } from '../../testing/test-helpers';
import { ConnectionBanner } from './connection-banner';
import { RegisterLauncher } from './register-launcher';
import { Sidebar } from './sidebar';
import { Toasts } from './toasts';

let mock: MockBackend;
afterEach(() => {
  mock?.dispose();
  TestBed.inject(Dialog).closeAll();
});

function configure(options: Parameters<typeof makeMock>[0] = {}, extra: unknown[] = []) {
  mock = makeMock(options);
  TestBed.configureTestingModule({ providers: [...appProviders(mock), ...(extra as never[])] });
}

describe('Toasts', () => {
  it('lists toasts and dismisses them', async () => {
    configure();
    const f = TestBed.createComponent(Toasts);
    const store = TestBed.inject(ToastStore);
    store.show({ tone: 'success', icon: 'check-circle', title: 'Saved', body: 'All good' });
    store.show({ tone: 'danger', icon: 'alert', title: 'Failed', body: null });
    await settle(f, 1);
    const el = f.nativeElement as HTMLElement;
    const toasts = el.querySelectorAll('.toast');
    expect(toasts).toHaveLength(2);
    expect(text(toasts[0])).toContain('Saved');
    expect(text(toasts[0])).toContain('All good');
    expect(toasts[0]!.classList.contains('success')).toBe(true);
    expect(toasts[1]!.querySelectorAll('.body span')).toHaveLength(0);
    (toasts[0]!.querySelector('button[aria-label="Dismiss"]') as HTMLButtonElement).click();
    f.detectChanges();
    expect(el.querySelectorAll('.toast')).toHaveLength(1);
    expect(el.querySelector('[aria-live="polite"]')).not.toBeNull();
  });
});

describe('Sidebar', () => {
  it('links every screen, with counts and the service state', async () => {
    configure();
    const f = TestBed.createComponent(Sidebar);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    const links = Array.from(el.querySelectorAll('nav a'));
    expect(links.map((a) => text(a).replace(/\d+/g, '').trim())).toEqual(['Dashboard', 'Runners', 'Activity', 'Insights', 'Storage', 'Logs', 'Updates', 'Settings']);
    expect(links.map((a) => a.getAttribute('href'))).toEqual(['/', '/runners', '/activity', '/insights', '/storage', '/logs', '/updates', '/settings']);
    expect(el.querySelector('.count.danger')?.getAttribute('aria-label')).toBe('1 need attention');
    expect(el.querySelector('.count.accent')?.getAttribute('aria-label')).toBe('2 updates available');
    expect(text(el.querySelector('.service'))).toBe('Service 0.1.0 · connected');
    expect(el.querySelector('.service')?.classList.contains('success')).toBe(true);
  });

  it('shows the runner count when nothing needs attention', async () => {
    configure({ scenario: 'demo' });
    const f = TestBed.createComponent(Sidebar);
    await settle(f);
    vi.spyOn(TestBed.inject(StatusStore), 'attention');
    const el = f.nativeElement as HTMLElement;
    await mock.stop('build-win');
    await settle(f);
    expect(el.querySelector('.count.danger')).not.toBeNull();
  });

  it('reflects connection problems', async () => {
    configure({ scenario: 'mismatch' });
    const f = TestBed.createComponent(Sidebar);
    await settle(f);
    expect(text(f.nativeElement.querySelector('.service'))).toBe('Service version mismatch');
    expect(f.nativeElement.querySelector('.service').classList.contains('danger')).toBe(true);
    TestBed.resetTestingModule();
    configure({ scenario: 'unavailable' });
    const g = TestBed.createComponent(Sidebar);
    await settle(g);
    expect(text(g.nativeElement.querySelector('.service'))).toBe('Service unavailable');
  });

  it('shows connecting before the backend answers', () => {
    configure();
    const f = TestBed.createComponent(Sidebar);
    f.detectChanges();
    expect(text(f.nativeElement.querySelector('.service'))).toBe('Connecting to the service…');
    expect(f.nativeElement.querySelector('.service').classList.contains('neutral')).toBe(true);
  });

  it('moves focus with the arrow keys, Home and End', async () => {
    configure();
    const f = TestBed.createComponent(Sidebar);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    const links = Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLAnchorElement>('nav a'));
    links[0]!.focus();
    const press = (key: string) => {
      const e = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
      document.activeElement!.dispatchEvent(e);
      return e;
    };
    press('ArrowDown');
    expect(document.activeElement).toBe(links[1]);
    press('ArrowUp');
    press('ArrowUp');
    expect(document.activeElement).toBe(links[7]);
    press('Home');
    expect(document.activeElement).toBe(links[0]);
    press('End');
    expect(document.activeElement).toBe(links[7]);
    expect(press('a').defaultPrevented).toBe(false);
    f.nativeElement.remove();
  });

  it('has no register button (the page headers carry it) and explains the service line in a tooltip', async () => {
    configure();
    const f = TestBed.createComponent(Sidebar);
    await settle(f, 1);
    const el: HTMLElement = f.nativeElement;
    expect(el.querySelector('button')).toBeNull();
    expect(text(el)).not.toContain('Register runner');
    expect(el.querySelector('.service')?.getAttribute('title')).toBe(text(el.querySelector('.service')));
  });
});

describe('RegisterLauncher', () => {
  it('opens the wizard dialog on demand', async () => {
    configure();
    await TestBed.inject(RegisterLauncher).open();
    await flush(20);
    expect(TestBed.inject(Dialog).openDialogs).toHaveLength(1);
    expect(document.querySelector('#register-title')).not.toBeNull();
  });
});

describe('ConnectionBanner', () => {
  it('is hidden while connected', async () => {
    configure();
    const f = TestBed.createComponent(ConnectionBanner);
    await settle(f);
    expect(f.nativeElement.querySelector('.banner')).toBeNull();
  });

  it('offers to install a service that is not installed', async () => {
    configure({ scenario: 'notInstalled' });
    const f = TestBed.createComponent(ConnectionBanner);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('strong'))).toBe('The Pitcrew service is not installed');
    expect(text(el)).toContain('not installed on this computer');
    expect(text(el)).toContain('UAC');
    byText<HTMLButtonElement>(el, 'button', 'Install service')!.click();
    f.detectChanges();
    expect(byText(el, 'button', 'Waiting for permission…')).not.toBeNull();
    await settle(f);
    expect(el.querySelector('.banner')).toBeNull();
    expect(TestBed.inject(ServiceStore).registration().value?.installed).toBe(true);
  });

  it('explains Login Items on macOS', async () => {
    configure({ scenario: 'notInstalled' });
    vi.spyOn(mock, 'serviceInstall').mockResolvedValue({ platform: 'macos', installed: false, running: false, canInstall: true, message: null, repair: null, disabledInLoginItems: false, startFailed: false, notInLoginItems: false });
    const f = TestBed.createComponent(ConnectionBanner);
    await settle(f);
    expect(text(f.nativeElement)).toContain('Login Items');
  });

  const macos = { platform: 'macos', installed: true, running: false, canInstall: true, message: null, repair: null, disabledInLoginItems: false, startFailed: false, notInLoginItems: false } as const;

  it('says the repair is working and offers no install while it does', async () => {
    configure({ scenario: 'unavailable' });
    const repair = { state: 'working', message: 'Re-registering the background service with macOS… (this can take a minute)' } as const;
    vi.spyOn(mock, 'serviceInstall').mockResolvedValue({ ...macos, installed: false, repair });
    const f = TestBed.createComponent(ConnectionBanner);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('[data-testid="service-repair"]'))).toContain('Re-registering the background service with macOS… (this can take a minute)');
    expect(byText(el, 'button', 'Install service')).toBeNull();
  });

  it('tells the user the item is off in Login Items and opens that pane, never switching it', async () => {
    configure({ scenario: 'unavailable' });
    vi.spyOn(mock, 'serviceInstall').mockResolvedValue({ ...macos, installed: false, disabledInLoginItems: true });
    const f = TestBed.createComponent(ConnectionBanner);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('[data-testid="service-disabled"]'))).toContain("Pitcrew's background service is turned off in System Settings ▸ General ▸ Login Items");
    expect(byText(el, 'button', 'Install service')).toBeNull();
    byText<HTMLButtonElement>(el, 'button', 'Open Login Items')!.click();
    await settle(f);
    expect(mock.loginItemsOpened).toBe(1);
  });

  it('shows an installed but stopped service without an install button', async () => {
    configure({ scenario: 'unavailable' });
    const f = TestBed.createComponent(ConnectionBanner);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('strong'))).toBe('Pitcrew cannot reach its service');
    expect(byText(el, 'button', 'Install service')).toBeNull();
  });

  it('reports a version mismatch and retries', async () => {
    configure({ scenario: 'mismatch' });
    const f = TestBed.createComponent(ConnectionBanner);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('strong'))).toBe('The app and the service are different versions');
    expect(el.querySelector('.banner')?.classList.contains('mismatch')).toBe(true);
    const status = vi.spyOn(mock, 'status');
    byText<HTMLButtonElement>(el, 'button', 'Retry')!.click();
    await settle(f);
    expect(status).toHaveBeenCalled();
  });
});
