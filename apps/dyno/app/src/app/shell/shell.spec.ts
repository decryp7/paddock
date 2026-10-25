import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { provideIcons } from '../shared/icons';
import { LiveStore } from '../state/live.store';
import { COMING, NAV } from './nav';
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

describe('navigation entries', () => {
  it('lists Backup health and Extras, each once, and nothing is left under Coming', () => {
    const labels = NAV.map((n) => n.label);
    expect(labels.filter((l) => l === 'Backup health')).toHaveLength(1);
    expect(labels.filter((l) => l === 'Extras')).toHaveLength(1);
    expect(NAV.find((n) => n.label === 'Backup health')!.path).toBe('/backup');
    expect(NAV.find((n) => n.label === 'Extras')!.path).toBe('/extras');
    expect(COMING).toEqual([]);
  });
});

describe('NAV', () => {
  it('has unique paths and starts at the dashboard', () => {
    expect(new Set(NAV.map((n) => n.path)).size).toBe(NAV.length);
    expect(NAV[0]!.path).toBe('/');
  });
  it('lists unbuilt features only in the Coming list, never as routes', () => {
    expect(COMING.map((c) => c.label)).toEqual([]);
    expect(NAV.some((n) => COMING.some((c) => c.label === n.label))).toBe(false);
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

  it('lists the built screens and shows the Coming button only while something is unbuilt', async () => {
    const { el } = await make();
    const label = (a: Element) => a.querySelector('.label')?.textContent?.trim();
    expect([...el.querySelectorAll('nav ul a')].map(label)).toEqual(NAV.map((n) => n.label));
    expect(el.querySelector('button.more') !== null).toBe(COMING.length > 0);
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
    expect(el.querySelector('.agent')).not.toBeNull();
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
