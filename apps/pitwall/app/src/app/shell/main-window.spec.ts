import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { BACKEND } from '../core/backend';
import { LiveStore } from '../core/live.store';
import { MockBackend } from '../core/mock-backend';
import { WINDOW_CONTROL } from '../core/window-control';
import { dialogsAnswering } from '../../testing/render';
import { startMainWindow } from './main-window';

const flush = async () => {
  for (let i = 0; i < 4; i++) {
    TestBed.tick();
    await new Promise((r) => setTimeout(r, 0));
  }
};

describe('the main window’s start', () => {
  function setup(backend: MockBackend) {
    let navigate: (route: string) => void = () => undefined;
    const dialogs = dialogsAnswering(() => undefined);
    TestBed.configureTestingModule({
      providers: [
        { provide: BACKEND, useValue: backend },
        provideRouter([]),
        dialogs,
        { provide: WINDOW_CONTROL, useValue: { openMain: () => Promise.resolve(), onNavigate: (h: (r: string) => void) => ((navigate = h), () => undefined) } },
      ],
    });
    TestBed.runInInjectionContext(startMainWindow);
    return { opened: dialogs.opened, navigate: (r: string) => navigate(r) };
  }

  it('loads the live picture in the background and asks nothing when nothing is pending', async () => {
    const { opened } = setup(new MockBackend());
    await flush();
    expect(TestBed.inject(LiveStore).server()).not.toBeNull();
    expect(opened).toEqual([]);
  });

  it('asks the first-run service question and the quit question the app asked for before the window loaded', async () => {
    const { opened } = setup(new MockBackend({ askConsent: true, quitAsk: true }));
    await flush();
    expect(opened.map((o) => o.id).sort()).toEqual(['pw.quit', 'pw.service-consent']);
  });

  it('goes to the screen the tray popover asks for', async () => {
    const { navigate } = setup(new MockBackend());
    const go = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    navigate('/models');
    expect(go).toHaveBeenCalledWith('/models');
  });
});
