import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { WatchesStore } from './state/watches.store';
import { SettingsStore } from './state/settings.store';
import { App, isTrayWindow } from './app';
import { appProviders, byText, flush, makeMock, settle } from '../testing/test-helpers';

@Component({ template: '<p class="page">page</p>' })
class Stub {}
const routes = [
  { path: '', component: Stub },
  { path: 'watches', component: Stub },
  { path: 'watches/:id', component: Stub },
  { path: 'settings', component: Stub },
];

describe('isTrayWindow', () => {
  it.each([
    ['#/tray', true],
    ['#/tray/', true],
    ['#/tray?x=1', true],
    ['#/trays', false],
    ['#/watches', false],
    ['', false],
  ])('%j -> %s', (hash, expected) => expect(isTrayWindow(hash)).toBe(expected));
});

describe('App (main window)', () => {
  afterEach(() => document.documentElement.classList.remove('pd-tray'));

  it('shows the shell, starts loading in the background and lets the skip link focus the page', async () => {
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
    const f = TestBed.createComponent(App);
    document.body.appendChild(f.nativeElement);
    await TestBed.inject(Router).navigateByUrl('/');
    await settle(f);
    expect(f.nativeElement.querySelector('sp-sidebar')).not.toBeNull();
    expect(f.nativeElement.querySelector('main#main .page')).not.toBeNull();
    expect(f.nativeElement.querySelector('sp-toasts')).not.toBeNull();
    expect(TestBed.inject(WatchesStore).watches()).toHaveLength(8);
    expect(TestBed.inject(SettingsStore).settings().value).not.toBeNull();
    const skip = byText(f.nativeElement, 'a.skip-link', 'Skip to content') as HTMLAnchorElement;
    const click = new MouseEvent('click', { bubbles: true, cancelable: true });
    skip.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(f.nativeElement.querySelector('main'));
    f.nativeElement.remove();
  });

  it('shows a route the tray asked for, but only known ones', async () => {
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
    const f = TestBed.createComponent(App);
    await settle(f);
    const router = TestBed.inject(Router);
    backend.navigate('/watches/w-3');
    await flush(5);
    expect(router.url).toBe('/watches/w-3');
    backend.navigate('/settings');
    await flush(5);
    expect(router.url).toBe('/settings');
    backend.navigate('/evil/../x');
    backend.navigate('https://elsewhere.example');
    await flush(5);
    expect(router.url).toBe('/settings');
  });

  it('stops listening for navigation when destroyed', async () => {
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
    const f = TestBed.createComponent(App);
    await settle(f);
    f.destroy();
    backend.navigate('/watches');
    await flush(5);
    expect(TestBed.inject(Router).url).not.toBe('/watches');
  });

  it('unsubscribes at once when destroyed before the subscription arrived, and survives a failing one', async () => {
    const backend = makeMock();
    const off = vi.fn();
    vi.spyOn(backend, 'onNavigate').mockImplementationOnce(() => Promise.resolve(off));
    TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
    TestBed.createComponent(App).destroy();
    await flush();
    expect(off).toHaveBeenCalled();
    vi.spyOn(backend, 'onNavigate').mockRejectedValueOnce(new Error('no events'));
    const g = TestBed.createComponent(App);
    await settle(g);
    expect(g.nativeElement.querySelector('sp-sidebar')).not.toBeNull();
  });
});

describe('App (tray window)', () => {
  it('renders only the routed popup and does not start the main window work', async () => {
    const original = window.location.hash;
    window.location.hash = '#/tray';
    try {
      const backend = makeMock();
      TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
      const f = TestBed.createComponent(App);
      await settle(f);
      expect(f.nativeElement.querySelector('sp-sidebar')).toBeNull();
      expect(f.nativeElement.querySelector('sp-toasts')).toBeNull();
      expect(f.nativeElement.querySelector('sp-copy-menu')).not.toBeNull();
      expect(document.documentElement.classList.contains('pd-tray')).toBe(true);
      expect(backend.calls).toEqual([]);
    } finally {
      window.location.hash = original;
      document.documentElement.classList.remove('pd-tray');
    }
  });
});
