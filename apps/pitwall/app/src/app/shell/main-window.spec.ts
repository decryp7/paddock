import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { BACKEND } from '../core/backend';
import { LiveStore } from '../core/live.store';
import { MockBackend } from '../core/mock-backend';
import { WINDOW_CONTROL } from '../core/window-control';
import { dialogsAnswering } from '../../testing/render';
import { OPEN_SETTINGS } from '@decryp7/paddock-ui/shell/notify';
import { startMainWindow } from './main-window';

/** Runs Angular's effects and microtasks until `check` passes (it throws until then). */
const until = (check: () => void) =>
  vi.waitFor(() => {
    TestBed.tick();
    check();
  });

describe('the main window’s start', () => {
  function setup(backend: MockBackend) {
    let navigate: (route: string) => void = () => undefined;
    const settingsPages: string[] = [];
    const dialogs = dialogsAnswering(() => undefined);
    TestBed.configureTestingModule({
      providers: [
        { provide: BACKEND, useValue: backend },
        provideRouter([]),
        dialogs,
        { provide: OPEN_SETTINGS, useValue: (page: string) => void settingsPages.push(page) },
        {
          provide: WINDOW_CONTROL,
          useValue: {
            openMain: () => Promise.resolve(),
            onNavigate: (h: (r: string) => void) => ((navigate = h), () => undefined),
            fitTray: () => Promise.resolve(),
            closeTray: () => Promise.resolve(),
            quit: () => Promise.resolve(),
          },
        },
      ],
    });
    TestBed.runInInjectionContext(startMainWindow);
    return { opened: dialogs.opened, settingsPages, navigate: (r: string) => navigate(r) };
  }

  it('loads the live picture in the background and asks nothing when nothing is pending', async () => {
    const { opened } = setup(new MockBackend());
    await until(() => expect(TestBed.inject(LiveStore).server()).not.toBeNull());
    expect(opened).toEqual([]);
  });

  it('asks the first-run service question and the quit question the app asked for before the window loaded', async () => {
    const { opened } = setup(new MockBackend({ askConsent: true, quitAsk: true }));
    await until(() => expect(opened.length).toBe(2));
    expect(opened.map((o) => o.id).sort()).toEqual(['pw.quit', 'pw.service-consent']);
  });

  it('goes to the screen the tray popover asks for once the window has routed', async () => {
    const { navigate } = setup(new MockBackend());
    const router = TestBed.inject(Router);
    vi.spyOn(router, 'navigated', 'get').mockReturnValue(true);
    const go = vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true);
    navigate('/models');
    expect(go).toHaveBeenCalledWith('/models');
  });

  it('opens the Settings window at the page a /settings route stands for, and does not navigate', async () => {
    const { navigate, settingsPages } = setup(new MockBackend());
    const router = TestBed.inject(Router);
    vi.spyOn(router, 'navigated', 'get').mockReturnValue(true);
    const go = vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true);
    navigate('/settings/tray-popup');
    navigate('/settings');
    expect(settingsPages).toEqual(['pw.tray', 'pw.general']);
    expect(go).not.toHaveBeenCalled();
    navigate('/settings/unknown');
    expect(settingsPages.length).toBe(2);
    expect(go).toHaveBeenCalledWith('/settings/unknown');
  });

  it('a route taken while the window starts waits for its first navigation, so the start does not undo it', async () => {
    const { navigate } = setup(new MockBackend());
    const router = TestBed.inject(Router);
    const real = router.navigateByUrl.bind(router);
    const go = vi.spyOn(router, 'navigateByUrl').mockImplementation((url) => (url === '/models' ? Promise.resolve(true) : real(url)));
    navigate('/models');
    expect(go).not.toHaveBeenCalled();
    await router.navigateByUrl('/');
    expect(go).toHaveBeenLastCalledWith('/models');
  });
});
