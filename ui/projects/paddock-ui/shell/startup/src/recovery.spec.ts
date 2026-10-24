import { provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import { MockStartupBackend, provideStartupBackend } from './backend';
import { CHECKPOINT_MS, provideUiCheckpoint, RECOVERY_NOTICE_KIND, RECOVERY_NOTICES, WindowRecovery, type UiCheckpoint } from './recovery';

function setup(checkpoint?: () => UiCheckpoint) {
  const notices: ShellNotice[] = [];
  const backend = new MockStartupBackend();
  const checkpoints: unknown[] = [];
  backend.checkpointUi = async (ui) => void checkpoints.push(ui);
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      provideStartupBackend(() => backend),
      { provide: SHELL_PORTS, useValue: { showPage: vi.fn(), openDialog: vi.fn(), notify: (n: ShellNotice) => notices.push(n) } },
      ...(checkpoint ? [provideUiCheckpoint(checkpoint)] : []),
    ],
  });
  return { recovery: TestBed.inject(WindowRecovery), notices, checkpoints, backend };
}

/** A note field as the window's UI state. */
function noteCheckpoint(loaded: Promise<void> = Promise.resolve()) {
  const note = signal('');
  const restored: unknown[] = [];
  const checkpoint: UiCheckpoint = {
    capture: () => ({ note: note() }),
    restore: (ui) => {
      restored.push(ui);
      note.set((ui as { note?: string }).note ?? '');
    },
    whenLoaded: () => loaded,
  };
  return { note, restored, checkpoint };
}

describe('WindowRecovery', () => {
  afterEach(() => vi.useRealTimers());

  it('a recovery reload restores the kept state once the data is loaded, and says so calmly', async () => {
    let load!: () => void;
    const n = noteCheckpoint(new Promise<void>((r) => (load = r)));
    const { recovery, notices } = setup(() => n.checkpoint);
    const done = recovery.afterReady({ recovered: 'hung', ui: { note: 'timeout' } });
    await Promise.resolve();
    expect(notices).toEqual([{ level: 'info', kind: RECOVERY_NOTICE_KIND, message: RECOVERY_NOTICES.hung.text }]);
    expect(n.restored, 'not before the data is there').toEqual([]);
    load();
    await done;
    expect(n.note()).toBe('timeout');
  });

  it('a normal start restores nothing and shows no notice; a recovery without state only tells', async () => {
    const n = noteCheckpoint();
    const { recovery, notices } = setup(() => n.checkpoint);
    await recovery.afterReady({ recovered: null, ui: { note: 'stale' } });
    await recovery.afterReady(null);
    expect(notices).toEqual([]);
    await recovery.afterReady({ recovered: 'terminated', ui: null });
    expect(n.restored).toEqual([]);
    expect(notices.map((x) => x.message)).toEqual([RECOVERY_NOTICES.terminated.text]);
  });

  it('a window without a checkpoint only tells, and never checkpoints', async () => {
    const { recovery, notices, checkpoints } = setup();
    await recovery.afterReady({ recovered: 'unresponsive', ui: { a: 1 } });
    expect(notices.map((x) => x.message)).toEqual([RECOVERY_NOTICES.unresponsive.text]);
    TestBed.tick();
    expect(checkpoints).toEqual([]);
  });

  it('a restore that fails is a warning and checkpoints still start', async () => {
    const { recovery, notices, checkpoints } = setup(() => ({
      capture: () => ({ v: 1 }),
      restore: () => {
        throw new Error('bad state');
      },
    }));
    vi.useFakeTimers();
    await recovery.afterReady({ recovered: 'terminated', ui: {} });
    expect(notices.at(-1)).toEqual({ level: 'warning', message: "The window's view could not be restored: bad state" });
    TestBed.tick();
    await vi.advanceTimersByTimeAsync(CHECKPOINT_MS);
    expect(checkpoints).toEqual([{ v: 1 }]);
    TestBed.resetTestingModule();
    const other = setup(() => ({ capture: () => null, restore: () => Promise.reject('plain') }));
    await other.recovery.afterReady({ recovered: 'hung', ui: {} });
    expect(other.notices.at(-1)?.message).toBe("The window's view could not be restored: plain");
  });

  it('checkpoints the UI state after a quiet second, only once the reply was handled, and only when it changed', async () => {
    const n = noteCheckpoint();
    const { recovery, checkpoints } = setup(() => n.checkpoint);
    vi.useFakeTimers();
    n.note.set('before ready');
    TestBed.tick();
    await vi.advanceTimersByTimeAsync(CHECKPOINT_MS * 3);
    expect(checkpoints).toEqual([]);
    await recovery.afterReady(null);
    recovery.startCheckpoints(); // idempotent
    TestBed.tick();
    await vi.advanceTimersByTimeAsync(CHECKPOINT_MS);
    expect(checkpoints).toEqual([{ note: 'before ready' }]);
    n.note.set('a');
    TestBed.tick();
    await vi.advanceTimersByTimeAsync(CHECKPOINT_MS - 1);
    n.note.set('ab');
    TestBed.tick();
    await vi.advanceTimersByTimeAsync(CHECKPOINT_MS - 1);
    expect(checkpoints, 'not before a quiet second').toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(checkpoints.at(-1)).toEqual({ note: 'ab' });
    expect(checkpoints).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(CHECKPOINT_MS * 5);
    expect(checkpoints).toHaveLength(2);
  });

  it('a failed checkpoint is ignored and a pending one is dropped with the window', async () => {
    const n = noteCheckpoint();
    const { recovery, backend } = setup(() => n.checkpoint);
    backend.checkpointUi = () => Promise.reject(new Error('gone'));
    vi.useFakeTimers();
    await recovery.afterReady(null);
    TestBed.tick();
    await vi.advanceTimersByTimeAsync(CHECKPOINT_MS);
    n.note.set('x');
    TestBed.tick();
    const spy = vi.spyOn(backend, 'checkpointUi');
    TestBed.resetTestingModule();
    await vi.advanceTimersByTimeAsync(CHECKPOINT_MS);
    expect(spy).not.toHaveBeenCalled();
  });
});
