import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { Component } from '@angular/core';
import { ClockService } from '../core/clock.service';
import { ToastStore } from '../state/toast.store';
import { WatchesStore } from '../state/watches.store';
import { SettingsStore } from '../state/settings.store';
import { appProviders, byText, flush, makeMock, NOW, settle, text } from '../../testing/test-helpers';
import { ConnectionBanner } from './connection-banner';
import { Sidebar } from './sidebar';
import { Toasts } from './toasts';

@Component({ template: '' })
class Stub {}
const routes = [
  { path: '', component: Stub },
  { path: 'watches', component: Stub },
  { path: 'settings', component: Stub },
];

describe('ConnectionBanner', () => {
  it('is hidden while the engine answers', async () => {
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend) });
    await TestBed.inject(WatchesStore).load();
    const f = TestBed.createComponent(ConnectionBanner);
    await settle(f);
    expect(f.nativeElement.querySelector('[role=alert]')).toBeNull();
  });

  it('says what happened and retries until the engine is back', async () => {
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend) });
    const real = backend.listWatches.bind(backend);
    let fail = true;
    vi.spyOn(backend, 'listWatches').mockImplementation(() => (fail ? Promise.reject({ code: 'unavailable', message: 'Engine is down.', problems: [] }) : real()));
    const store = TestBed.inject(WatchesStore);
    await store.load();
    const f = TestBed.createComponent(ConnectionBanner);
    await settle(f);
    const banner = f.nativeElement.querySelector('[role=alert]');
    expect(text(banner)).toContain('Spotter cannot reach its engine');
    expect(text(banner)).toContain('Engine is down.');
    fail = false;
    (byText(banner, 'button', 'Retry') as HTMLButtonElement).click();
    await settle(f);
    expect(f.nativeElement.querySelector('[role=alert]')).toBeNull();
    expect(store.watches()).toHaveLength(8);
  });
});

describe('Toasts', () => {
  it('lists toasts politely and dismisses one', async () => {
    TestBed.configureTestingModule({ providers: appProviders(makeMock()) });
    const toasts = TestBed.inject(ToastStore);
    toasts.success('Saved', 'All good');
    toasts.error('Broke');
    const f = TestBed.createComponent(Toasts);
    await settle(f);
    expect(f.nativeElement.querySelector('[aria-live=polite]')).not.toBeNull();
    const items = f.nativeElement.querySelectorAll('.toast');
    expect(items).toHaveLength(2);
    expect(text(items[0])).toContain('All good');
    expect(items[1].classList.contains('danger')).toBe(true);
    (items[0].querySelector('button[aria-label=Dismiss]') as HTMLButtonElement).click();
    await settle(f);
    expect(f.nativeElement.querySelectorAll('.toast')).toHaveLength(1);
  });
});

describe('Sidebar', () => {
  async function make() {
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend, routes) });
    const watches = TestBed.inject(WatchesStore);
    await watches.load();
    const f = TestBed.createComponent(Sidebar);
    await TestBed.inject(Router).navigateByUrl('/watches');
    await settle(f);
    return { f, backend, watches };
  }

  it('links the pages, marks the current one and counts unseen releases', async () => {
    const { f, watches } = await make();
    const links = Array.from(f.nativeElement.querySelectorAll('nav a')) as HTMLAnchorElement[];
    expect(links.map((a) => text(a).replace(/\d+$/, '').trim())).toEqual(['Dashboard', 'Watches', 'Settings']);
    const current = links.filter((a) => a.getAttribute('aria-current') === 'page');
    expect(current).toHaveLength(1);
    expect(text(current[0])).toContain('Watches');
    expect(links[1]!.querySelector('.count')?.textContent?.trim()).toBe(String(watches.unseen().length));
    expect(links[0]!.querySelector('.count')).toBeNull();
  });

  it('moves focus between links with the arrow keys, Home and End', async () => {
    const { f } = await make();
    document.body.appendChild(f.nativeElement);
    const links = Array.from(f.nativeElement.querySelectorAll('nav a')) as HTMLAnchorElement[];
    const press = (key: string) => {
      const e = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
      (document.activeElement ?? links[0]!).dispatchEvent(e);
      return e;
    };
    links[0]!.focus();
    expect(press('ArrowDown').defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(links[1]);
    press('End');
    expect(document.activeElement).toBe(links[2]);
    press('ArrowDown');
    expect(document.activeElement).toBe(links[0]);
    press('ArrowUp');
    expect(document.activeElement).toBe(links[2]);
    press('Home');
    expect(document.activeElement).toBe(links[0]);
    expect(press('a').defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(links[0]);
    f.nativeElement.remove();
  });

  it('shows how long notifications stay paused, and nothing once it passed', async () => {
    const { f, backend } = await make();
    const clock = TestBed.inject(ClockService);
    clock.set(NOW);
    const settings = TestBed.inject(SettingsStore);
    expect(f.nativeElement.querySelector('.paused')).toBeNull();
    await settings.pauseNotifications(90);
    await settle(f);
    expect(text(f.nativeElement.querySelector('.paused'))).toContain('Notifications paused for another 2 h');
    clock.set(NOW + 3 * 3_600_000);
    await settle(f);
    expect(f.nativeElement.querySelector('.paused')).toBeNull();
    expect(backend.calls).toContain('pauseNotifications');
    await flush();
  });
});
