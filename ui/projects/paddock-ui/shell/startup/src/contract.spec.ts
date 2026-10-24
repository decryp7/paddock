import { ChangeDetectionStrategy, Component, DestroyRef, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import type { ReadyReply } from '../generated/startup';
import { MockStartupBackend, provideStartupBackend, StartupBackend } from './backend';
import { startBootErrorForwarding, reportBootstrapFailure, stopBootErrorForwarding } from './boot-errors';
import { AFTER_READY, answerPings, provideStartupContract, reportReady } from './contract';
import { RECOVERY_NOTICES } from './recovery';

@Component({ selector: 'pd-ready-host', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>frame</p>` })
class Host {}

const fast = { timeoutMs: 1, attempts: 1 };

describe('reportReady', () => {
  afterEach(() => stopBootErrorForwarding());

  it('hands the reply on, recovered or not, and stops boot error forwarding', async () => {
    const send = vi.fn();
    startBootErrorForwarding(send, new EventTarget() as unknown as Window);
    const after = vi.fn();
    await reportReady(async () => ({ recovered: 'terminated', ui: { query: {} } }), after, fast);
    expect(after).toHaveBeenCalledWith({ recovered: 'terminated', ui: { query: {} } });
    reportBootstrapFailure('after ready');
    expect(send).not.toHaveBeenCalled();
    await reportReady(async () => ({ recovered: null, ui: null }), null);
  });

  it('never throws when the report fails (the Rust watchdog acts instead) and still hands on null', async () => {
    const after = vi.fn();
    await expect(reportReady(() => Promise.reject(new Error('down')), after, fast)).resolves.toBeNull();
    expect(after).toHaveBeenCalledWith(null);
  });
});

describe('answerPings', () => {
  function destroyRef() {
    const callbacks: (() => void)[] = [];
    return { ref: { onDestroy: (cb: () => void) => (callbacks.push(cb), () => undefined), destroyed: false } as unknown as DestroyRef, destroy: () => callbacks.forEach((c) => c()) };
  }

  it('answers each ping with its number until destroyed', async () => {
    const b = new MockStartupBackend();
    const d = destroyRef();
    await answerPings(b, d.ref);
    b.ping(1);
    b.ping(2);
    d.destroy();
    b.ping(3);
    expect(b.pongs).toEqual([1, 2]);
  });

  it('stops at once when destroyed while subscribing, and gives up quietly when it cannot listen', async () => {
    const b = new MockStartupBackend();
    const d = destroyRef();
    const pending = answerPings(b, d.ref);
    d.destroy();
    await pending;
    b.ping(1);
    expect(b.pongs).toEqual([]);
    const broken = new MockStartupBackend();
    broken.onPing = () => Promise.reject(new Error('no events'));
    await expect(answerPings(broken, destroyRef().ref)).resolves.toBeUndefined();
    const failing = new MockStartupBackend();
    failing.pong = () => Promise.reject(new Error('gone'));
    await answerPings(failing, destroyRef().ref);
    expect(() => failing.ping(1)).not.toThrow();
  });
});

describe('provideStartupContract', () => {
  it('reports ready after the first render, not before, hands on the reply and then answers pings', async () => {
    let answer!: (r: ReadyReply) => void;
    const backend = new MockStartupBackend();
    const reportReadySpy = vi.spyOn(backend, 'reportReady').mockImplementation(() => new Promise<ReadyReply>((resolve) => (answer = resolve)));
    const after = vi.fn();
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideStartupBackend(() => backend), provideStartupContract({ retry: fast }), { provide: AFTER_READY, useValue: after }],
    });
    const fixture = TestBed.createComponent(Host);
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('frame');
    expect(reportReadySpy).toHaveBeenCalledTimes(1);
    backend.ping(1);
    expect(backend.pongs, 'no answers before ready').toEqual([]);
    answer({ recovered: 'unresponsive', ui: null });
    await vi.waitFor(() => expect(after).toHaveBeenCalledWith({ recovered: 'unresponsive', ui: null }));
    await vi.waitFor(() => {
      backend.ping(2);
      expect(backend.pongs).toContain(2);
    });
  });

  it('shows the recovery notice through the shell by default', async () => {
    const notices: ShellNotice[] = [];
    const backend = new MockStartupBackend();
    vi.spyOn(backend, 'reportReady').mockResolvedValue({ recovered: 'terminated', ui: null });
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideStartupBackend(() => backend),
        provideStartupContract(),
        { provide: SHELL_PORTS, useValue: { showPage: vi.fn(), openDialog: vi.fn(), notify: (n: ShellNotice) => notices.push(n) } },
      ],
    });
    const fixture = TestBed.createComponent(Host);
    await fixture.whenStable();
    await vi.waitFor(() => expect(notices.map((n) => n.message)).toEqual([RECOVERY_NOTICES.terminated.text]));
    expect(TestBed.inject(StartupBackend)).toBe(backend);
  });
});
