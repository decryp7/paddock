import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import type { StartReport, UncleanShutdown } from '../generated/startup';
import { MockStartupBackend, provideStartupBackend } from './backend';
import { provideUiCheckpoint, type UiCheckpoint } from './recovery';
import { RecoveryStore, RESTORED_NOTICE_KIND } from './recovery.store';

const UNCLEAN: UncleanShutdown = {
  crash: null,
  session: { available: true, windows: 2, problem: null },
  offers: { restore: true, safeMode: true, startFresh: true },
};

function setup(report: StartReport | Error, checkpoint?: UiCheckpoint) {
  const notices: ShellNotice[] = [];
  const backend = new MockStartupBackend();
  backend.recoveryStatus = () => (report instanceof Error ? Promise.reject(report) : Promise.resolve(report));
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      provideStartupBackend(() => backend),
      { provide: SHELL_PORTS, useValue: { showPage: vi.fn(), openDialog: vi.fn(), notify: (n: ShellNotice) => notices.push(n) } },
      ...(checkpoint ? [provideUiCheckpoint(() => checkpoint)] : []),
      RecoveryStore,
    ],
  });
  const store = TestBed.inject(RecoveryStore);
  return { store, backend, notices };
}

describe('RecoveryStore', () => {
  afterEach(() => sessionStorage.clear());

  it('asks Rust how the run started when it is created, and a normal start has no notice', async () => {
    const { store } = setup({ safeMode: false, unclean: null });
    await vi.waitFor(() => expect(store.isLoaded()).toBe(true));
    expect(store.unclean()).toBeNull();
    expect(store.safeMode()).toBe(false);
  });

  it('safe mode is reported for the title bar', async () => {
    const { store } = setup({ safeMode: true, unclean: null });
    await vi.waitFor(() => expect(store.safeMode()).toBe(true));
  });

  it('a window that cannot be asked shows nothing and keeps the reason', async () => {
    const { store } = setup(new Error('no such command'));
    await vi.waitFor(() => expect(store.requestError()).toBe('no such command'));
    expect(store.unclean()).toBeNull();
    expect(store.safeMode()).toBe(false);
  });

  it('Restore hands the previous session to the window and tells the other windows, then the notice is gone', async () => {
    const restored: unknown[] = [];
    const windows: (readonly string[])[] = [];
    const order: string[] = [];
    const { store, backend, notices } = setup(
      { safeMode: false, unclean: UNCLEAN },
      {
        capture: () => ({}),
        whenLoaded: async () => void order.push('loaded'),
        restore: (ui) => void (restored.push(ui), order.push('restore')),
        restoreWindows: (labels) => void (windows.push(labels), order.push('windows')),
      },
    );
    backend.recoveryRestore = async () => ({ windows: ['main', 'w-2'], ui: { note: 'kept' } });
    await vi.waitFor(() => expect(store.unclean()).not.toBeNull());
    await store.restore();
    expect(restored).toEqual([{ note: 'kept' }]);
    expect(windows).toEqual([['main', 'w-2']]);
    expect(order, 'after the data is there, window state first').toEqual(['loaded', 'restore', 'windows']);
    expect(store.unclean()).toBeNull();
    expect(store.acting()).toBeNull();
    expect(notices.map((n) => n.kind)).toEqual([RESTORED_NOTICE_KIND]);
  });

  it('Restore without state for this window, or without a checkpoint, still settles the notice', async () => {
    const withCp = setup({ safeMode: false, unclean: UNCLEAN }, { capture: () => ({}), restore: vi.fn() });
    withCp.backend.recoveryRestore = async () => ({ windows: ['w-2'], ui: null });
    await vi.waitFor(() => expect(withCp.store.unclean()).not.toBeNull());
    await withCp.store.restore();
    expect(withCp.store.unclean()).toBeNull();
    TestBed.resetTestingModule();
    const without = setup({ safeMode: false, unclean: UNCLEAN });
    await vi.waitFor(() => expect(without.store.unclean()).not.toBeNull());
    await without.store.restore();
    expect(without.store.unclean()).toBeNull();
  });

  it('a Restore that fails keeps the notice, with the reason, so the user can choose again', async () => {
    const { store, backend } = setup({ safeMode: false, unclean: UNCLEAN }, { capture: () => ({}), restore: () => Promise.reject(new Error('the view is gone')) });
    backend.recoveryRestore = async () => ({ windows: ['main'], ui: { a: 1 } });
    await vi.waitFor(() => expect(store.unclean()).not.toBeNull());
    await store.restore();
    expect(store.unclean()).not.toBeNull();
    expect(store.requestError()).toBe('the view is gone');
    expect(store.acting()).toBeNull();
    // Choosing again works.
    backend.recoveryStartFresh = async () => undefined;
    await store.startFresh();
    expect(store.unclean()).toBeNull();
    expect(store.requestError()).toBeNull();
  });

  it('Start fresh asks Rust to forget the session and ends the notice', async () => {
    const { store, backend } = setup({ safeMode: false, unclean: UNCLEAN });
    await vi.waitFor(() => expect(store.unclean()).not.toBeNull());
    await store.startFresh();
    expect(backend.recoveryCalls).toEqual(['start-fresh']);
    expect(store.unclean()).toBeNull();
  });

  it('Safe mode asks Rust to restart; the window says so and takes no second choice', async () => {
    const { store, backend } = setup({ safeMode: false, unclean: UNCLEAN });
    await vi.waitFor(() => expect(store.unclean()).not.toBeNull());
    await store.safeModeRestart();
    expect(store.restarting()).toBe(true);
    await store.startFresh();
    await store.restore();
    expect(backend.recoveryCalls, 'one choice at a time').toEqual(['safe-mode']);
  });

  it('a refused Safe mode restart keeps the notice and the other choices', async () => {
    const { store, backend } = setup({ safeMode: false, unclean: UNCLEAN });
    backend.recoverySafeMode = () => Promise.reject({ code: 'safeModeFailed', message: 'could not relaunch' });
    await vi.waitFor(() => expect(store.unclean()).not.toBeNull());
    await store.safeModeRestart();
    expect(store.restarting()).toBe(false);
    expect(store.requestError()).toBe('could not relaunch');
    expect(store.unclean()).not.toBeNull();
  });

  it('the browser mock plays the whole flow through session storage', async () => {
    sessionStorage.setItem('paddock.mock.unclean', '1');
    sessionStorage.setItem('paddock.mock.uiCheckpoint', JSON.stringify({ note: 'draft' }));
    const restored: unknown[] = [];
    const notices: ShellNotice[] = [];
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideStartupBackend(() => new MockStartupBackend()),
        { provide: SHELL_PORTS, useValue: { showPage: vi.fn(), openDialog: vi.fn(), notify: (n: ShellNotice) => notices.push(n) } },
        provideUiCheckpoint(() => ({ capture: () => ({}), restore: (ui) => void restored.push(ui) })),
        RecoveryStore,
      ],
    });
    const store = TestBed.inject(RecoveryStore);
    await vi.waitFor(() => expect(store.unclean()).not.toBeNull());
    await store.restore();
    expect(restored).toEqual([{ note: 'draft' }]);
    expect(sessionStorage.getItem('paddock.mock.unclean')).toBeNull();
  });
});
