import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { provideIcons } from '../shared/icons';
import { LiveStore } from '../state/live.store';
import { NAV } from './nav';
import { Shell } from './shell';

const NOW = 1_700_000_000_000;

async function make() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [provideRouter([]), provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(Shell);
  f.detectChanges();
  await f.whenStable();
  await new Promise((r) => setTimeout(r, 5)); // the mock reports the agent a tick after subscribing
  f.detectChanges();
  return { f, mock, el: f.nativeElement as HTMLElement, live: TestBed.inject(LiveStore) };
}

describe('NAV', () => {
  it('has unique paths, starts at the dashboard and marks the unbuilt screens', () => {
    expect(new Set(NAV.map((n) => n.path)).size).toBe(NAV.length);
    expect(NAV[0]!.path).toBe('/');
    expect(NAV.filter((n) => n.planned).map((n) => n.label)).toEqual(['Why slow', 'CPU taming', 'Cleanup', 'Health & tuning', 'Reports & alerts']);
  });
});

describe('Shell', () => {
  beforeEach(() => {
    try {
      localStorage.removeItem('dyno.sidebarCollapsed');
    } catch {
      /* no storage */
    }
  });

  it('lists every NAV item, with "Soon" only on the planned ones', async () => {
    const { el } = await make();
    const links = [...el.querySelectorAll<HTMLAnchorElement>('nav a')];
    expect(links.map((a) => a.querySelector('.label')?.textContent?.trim())).toEqual(NAV.map((n) => n.label));
    NAV.forEach((item, i) => {
      const soon = links[i]!.querySelector('.soon');
      if (item.planned) expect(soon?.textContent?.trim()).toBe('Soon');
      else expect(soon).toBeNull();
    });
    expect(el.querySelectorAll('.soon').length).toBe(NAV.filter((n) => n.planned).length);
  });
  it('has a skip link, a main landmark and a search button', async () => {
    const { el } = await make();
    expect(el.querySelector('a.skip')?.textContent).toContain('Skip to content');
    expect(el.querySelector('main#main')).not.toBeNull();
    expect(el.querySelector('button[aria-label="Search and commands"]')).not.toBeNull();
    expect(el.querySelector('aside[aria-label="Navigation"]')).not.toBeNull();
  });
  it('collapses and expands the sidebar, labelling the icon-only links', async () => {
    const { f, el } = await make();
    const button = () => el.querySelector<HTMLButtonElement>('button.collapse')!;
    expect(button().getAttribute('aria-label')).toBe('Collapse sidebar');
    expect(el.querySelector('nav a')!.getAttribute('aria-label')).toBeNull();
    button().click();
    f.detectChanges();
    expect(el.classList.contains('collapsed')).toBe(true);
    expect(button().getAttribute('aria-label')).toBe('Expand sidebar');
    expect(button().getAttribute('aria-expanded')).toBe('false');
    expect(el.querySelector('nav a')!.getAttribute('aria-label')).toBe('Dashboard');
    button().click();
    f.detectChanges();
    expect(el.classList.contains('collapsed')).toBe(false);
  });
  it('shows the agent status and Dyno overhead from the live store', async () => {
    const { f, el, live, mock } = await make();
    expect(el.querySelector('.agent')!.getAttribute('data-state')).toBe('running'); // the mock reports running soon after subscribing
    live.receive(mock.liveUpdate('summary'));
    f.detectChanges();
    expect(el.querySelector('.overhead')!.textContent).toMatch(/\d/);
    live.setAgent({ state: 'stopped', message: 'gone' } as never);
    f.detectChanges();
    expect(el.querySelector('.agent')!.textContent).toContain('Agent stopped');
  });
  it('toasts alerts pushed by the agent', async () => {
    const { f, mock, el } = await make();
    mock.pushAlert({ id: 'x', at: NOW, kind: 'lowDisk', severity: 'warning', title: 'Disk almost full', detail: '' });
    f.detectChanges();
    expect(el.querySelector('dy-toasts')!.textContent).toContain('Disk almost full');
  });
  it('Ctrl+K opens the command palette once', async () => {
    const { f } = await make();
    const press = () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, cancelable: true }));
    press();
    press();
    f.detectChanges();
    expect(document.querySelectorAll('.dy-palette-panel').length).toBe(1);
  });
});
