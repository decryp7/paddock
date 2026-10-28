import { provideRouter } from '@angular/router';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { LiveStore } from '../../state/live.store';
import { ProcessesStore } from '../../state/processes.store';
import { ProcessDetailsPanel } from './process-details';

const NOW = 1_700_000_000_000;

async function make(os: 'windows' | 'macos' = 'windows', fail?: Set<string>) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, os, fail });
  TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([]), { provide: BACKEND, useValue: mock }] });
  const store = TestBed.inject(ProcessesStore);
  TestBed.inject(LiveStore);
  store.start();
  await new Promise((r) => setTimeout(r, 5));
  const f = TestBed.createComponent(ProcessDetailsPanel);
  const el = f.nativeElement as HTMLElement;
  const settle = async () => {
    await f.whenStable();
    f.detectChanges();
  };
  const tab = async (label: string) => {
    [...el.querySelectorAll<HTMLButtonElement>('[role=tab]')].find((b) => b.textContent?.trim() === label)!.click();
    await settle();
  };
  return { f, store, mock, el, settle, tab };
}

describe('ProcessDetailsPanel', () => {
  it('shows a skeleton while nothing is selected or loaded', async () => {
    const { f, el, settle } = await make();
    f.detectChanges();
    await settle();
    expect(el.querySelector('[aria-busy="true"]')).not.toBeNull();
    expect(el.querySelector('h2')?.textContent).toBe('Process');
  });

  it('shows the overview of the selected process', async () => {
    const { f, el, store, settle } = await make();
    const p = store.processes().find((x) => x.name.startsWith('rustc'))!;
    await store.select({ pid: p.pid, started: p.started });
    f.detectChanges();
    await settle();
    expect(el.querySelector('h2')?.textContent).toBe(p.name);
    expect(el.textContent).toContain(`PID ${p.pid}`);
    expect(el.querySelector('dl')?.textContent).toContain('Command line');
    expect(el.querySelector('dl')?.textContent).toContain('Efficiency mode');
    expect(el.querySelector('dl')?.textContent).not.toContain('Not available on this PC');
  });

  it('walks the tabs: parents, files, network, libraries', async () => {
    const { f, el, store, settle, tab } = await make();
    const withParent = store.processes().find((x) => x.parent !== null && x.name.startsWith('rustc'))!;
    await store.select({ pid: withParent.pid, started: withParent.started });
    f.detectChanges();
    await settle();
    await tab('Parents');
    expect(el.querySelector('ol.chain li.me')?.textContent).toContain(withParent.name);
    expect(el.querySelectorAll('ol.chain li').length).toBeGreaterThan(1);
    // open a parent
    el.querySelector<HTMLButtonElement>('ol.chain button')!.click();
    await settle();
    expect(store.selected()?.pid).not.toBe(withParent.pid);
    await store.select({ pid: withParent.pid, started: withParent.started });
    await settle();
    await tab('Files');
    expect(el.querySelectorAll('ul.plain li').length).toBeGreaterThan(0);
    await tab('Network');
    expect(el.textContent).toMatch(/Nothing is listening|Connections|Listening/);
    await tab('Libraries');
    expect(el.textContent).toContain('Only the first entries are shown.');
  });

  it('lists connections for the browser', async () => {
    const { f, el, store, settle, tab, mock } = await make();
    const chrome = store.processes().find((x) => x.name === 'chrome.exe' || x.name.toLowerCase().includes('chrome'))!;
    await store.select({ pid: chrome.pid, started: chrome.started });
    f.detectChanges();
    await settle();
    const snap = await mock.network({ showLoopback: true, proto: null, state: null, exposedOnly: false, search: '' });
    const own = { kind: 'process' as const, pid: chrome.pid, name: 'chrome', path: null };
    vi.spyOn(mock, 'network').mockResolvedValue({
      ...snap,
      connections: [{ ...snap.connections[0]!, owner: own, remote: '142.250.74.46', remotePort: 443 }],
      listeners: [{ ...snap.listeners[0]!, owner: own }],
      traffic: [{ pid: chrome.pid, name: 'chrome', rxBps: 2048, txBps: 512 }],
    });
    await tab('Network');
    await settle();
    expect(el.textContent).toContain('142.250.74.46:443');
    expect(el.textContent).toContain('Listening');
    expect(el.textContent).toContain('↓');
    expect(mock).toBeDefined();
  });

  it('shows "Not available" for cores and priority-less data on macOS', async () => {
    const { f, el, store, settle } = await make('macos');
    const p = store.processes().find((x) => x.name.startsWith('rustc'))!;
    await store.select({ pid: p.pid, started: p.started });
    f.detectChanges();
    await settle();
    expect(el.querySelector('dl')?.textContent).toContain('Not available on this PC');
  });

  it('shows the error with a hint and Retry reloads', async () => {
    const { f, el, store, settle } = await make('windows', new Set(['processDetails']));
    const p = store.processes().find((x) => x.name.startsWith('rustc'))!;
    await store.select({ pid: p.pid, started: p.started });
    f.detectChanges();
    await settle();
    const alert = el.querySelector('[role=alert]');
    expect(alert).not.toBeNull();
    const spy = vi.spyOn(store, 'select');
    [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === 'Retry')!.click();
    expect(spy).toHaveBeenCalledWith(store.selected());
  });

  it('close clears the selection; reload without selection does nothing; open ignores unknown pids', async () => {
    const { f, el, store, settle } = await make();
    const p = store.processes().find((x) => x.name.startsWith('rustc'))!;
    await store.select({ pid: p.pid, started: p.started });
    f.detectChanges();
    await settle();
    const spy = vi.spyOn(store, 'select');
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const c = f.componentInstance as any;
    c.open(-1);
    expect(spy).not.toHaveBeenCalled();
    el.querySelector<HTMLButtonElement>('[aria-label="Close details"]')!.click();
    expect(spy).toHaveBeenCalledWith(null);
    c.reload();
    expect(spy).toHaveBeenCalledTimes(1);
  });
});
