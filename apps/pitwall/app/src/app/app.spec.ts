import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { Router, provideRouter } from '@angular/router';
import { App } from './app';
import { routes } from './app.routes';
import { BACKEND } from './core/backend';
import { LiveStore } from './core/live.store';
import { MockBackend } from './core/mock-backend';
import { WINDOW_CONTROL } from './core/window-control';
import { registerIcons } from './shared/icons';
import { fakeChartProvider, settle } from '../testing/render';

describe('App', () => {
  const nav: { handler: ((route: string) => void) | null; stopped: boolean } = { handler: null, stopped: false };

  function create(backend = new MockBackend({ latencyMs: 0 })) {
    nav.handler = null;
    nav.stopped = false;
    TestBed.configureTestingModule({
      imports: [App],
      providers: [
        { provide: BACKEND, useValue: backend },
        provideRouter(routes),
        fakeChartProvider,
        {
          provide: WINDOW_CONTROL,
          useValue: {
            openMain: () => Promise.resolve(),
            onNavigate: (h: (route: string) => void) => {
              nav.handler = h;
              return () => (nav.stopped = true);
            },
          },
        },
      ],
    });
    registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    const fixture = TestBed.createComponent(App);
    return { fixture, el: fixture.nativeElement as HTMLElement, router: TestBed.inject(Router), live: TestBed.inject(LiveStore) };
  }

  afterEach(() => {
    window.location.hash = '';
    document.documentElement.classList.remove('mac', 'translucent');
  });

  it('renders the window layout: titlebar, sidebar, main and the skip link', async () => {
    const { fixture, el, router } = create();
    await router.navigateByUrl('/');
    await settle(fixture, 6);
    expect(el.querySelector('.titlebar')).not.toBeNull();
    expect(el.querySelector('pw-sidebar')).not.toBeNull();
    expect(el.querySelector('main#main')).not.toBeNull();
    expect(el.querySelector('a.skip')?.textContent).toContain('Skip to content');
    expect(el.querySelector('pw-toast-host')).not.toBeNull();
  });

  it('starts loading the live data and subscribes once', async () => {
    const backend = new MockBackend({ latencyMs: 0 });
    const subscribe = vi.spyOn(backend, 'subscribe');
    const { fixture, live } = create(backend);
    await settle(fixture, 6);
    expect(live.server()?.port).toBe(11500);
    expect(subscribe).toHaveBeenCalledTimes(1);
  });

  it('the skip link moves focus to the main region instead of changing the hash', async () => {
    const { fixture, el, router } = create();
    await router.navigateByUrl('/');
    await settle(fixture);
    const link = el.querySelector<HTMLAnchorElement>('a.skip')!;
    const ev = new MouseEvent('click', { bubbles: true, cancelable: true });
    document.body.appendChild(el);
    link.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(el.querySelector('main'));
    el.remove();
  });

  it('shows the tray popover alone, without sidebar or skip link, when the URL is #/tray', async () => {
    window.location.hash = '#/tray';
    const { fixture, el, router } = create();
    await settle(fixture);
    expect(el.querySelector('pw-sidebar')).toBeNull();
    expect(el.querySelector('.titlebar')).toBeNull();
    expect(el.querySelector('a.skip')).toBeNull();
    expect(el.querySelector('pw-toast-host')).not.toBeNull();
    await router.navigateByUrl('/tray');
    await settle(fixture, 6);
    expect(el.querySelector('pw-sidebar')).toBeNull();
  });

  it('switches layout when navigating between the tray and a window route', async () => {
    const { fixture, el, router } = create();
    await router.navigateByUrl('/tray');
    await settle(fixture, 6);
    expect(el.querySelector('pw-sidebar')).toBeNull();
    await router.navigateByUrl('/models');
    await settle(fixture, 6);
    expect(el.querySelector('pw-sidebar')).not.toBeNull();
    expect(el.querySelector('main h1')?.textContent).toContain('Models');
  });

  it('opens the screen the tray asks for and stops listening when destroyed', async () => {
    const { fixture, router } = create();
    await settle(fixture);
    expect(nav.handler).not.toBeNull();
    nav.handler!('/benchmarks');
    await settle(fixture, 6);
    expect(router.url).toBe('/benchmarks');
    fixture.destroy();
    expect(nav.stopped).toBe(true);
  });

  it('marks the platform: translucent by query, mac by user agent', async () => {
    window.history.replaceState(null, '', '/?translucent');
    try {
      create();
      expect(document.documentElement.classList.contains('translucent')).toBe(true);
      expect(document.documentElement.classList.contains('mac')).toBe(/Mac OS X|Macintosh/.test(navigator.userAgent) && !/iPhone|iPad/.test(navigator.userAgent));
    } finally {
      window.history.replaceState(null, '', '/');
    }
  });

  it('keeps working when the server is unreachable', async () => {
    const { fixture, el, live } = create(new MockBackend({ failing: ['overview'] }));
    await settle(fixture, 6);
    expect(live.error()?.code).toBe('serverUnavailable');
    expect(el.querySelector('pw-sidebar .state')?.textContent).toContain('Server unreachable');
  });
});
