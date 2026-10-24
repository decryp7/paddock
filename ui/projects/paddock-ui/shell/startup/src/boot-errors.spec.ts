import { describeError, MAX_BOOT_ERRORS, reportBootstrapFailure, startBootErrorForwarding, stopBootErrorForwarding } from './boot-errors';

afterEach(() => stopBootErrorForwarding());

describe('boot error forwarding', () => {
  it('forwards uncaught errors and unhandled rejections until the window is ready', () => {
    const target = new EventTarget();
    const send = vi.fn();
    startBootErrorForwarding(send, target as unknown as Window);
    startBootErrorForwarding(vi.fn(), target as unknown as Window); // idempotent
    const error = Object.assign(new Event('error'), { error: new TypeError('x is undefined') });
    target.dispatchEvent(error);
    target.dispatchEvent(Object.assign(new Event('unhandledrejection'), { reason: 'ipc refused' }));
    expect(send.mock.calls.map((c) => c[0])).toEqual(['error', 'rejection']);
    expect(send.mock.calls[0]![1]).toMatch(/^TypeError: x is undefined/);
    expect(send.mock.calls[1]![1]).toBe('ipc refused');
    stopBootErrorForwarding();
    stopBootErrorForwarding();
    target.dispatchEvent(error);
    expect(send).toHaveBeenCalledTimes(2);
  });

  it('reports a failed bootstrap and caps the number of reports', () => {
    const target = new EventTarget();
    const send = vi.fn();
    startBootErrorForwarding(send, target as unknown as Window);
    reportBootstrapFailure(new Error('no providers'));
    expect(send).toHaveBeenCalledWith('bootstrap', expect.stringMatching(/^Error: no providers/));
    for (let i = 0; i < 50; i++) target.dispatchEvent(Object.assign(new Event('error'), { message: `e${i}` }));
    expect(send).toHaveBeenCalledTimes(MAX_BOOT_ERRORS);
  });

  it('sends nothing before it is started, and a sender that throws never breaks the page', () => {
    expect(() => reportBootstrapFailure('before')).not.toThrow();
    startBootErrorForwarding(() => {
      throw new Error('ipc gone');
    }, new EventTarget() as unknown as Window);
    expect(() => reportBootstrapFailure('x')).not.toThrow();
  });

  it('listens on the window by default', () => {
    const send = vi.fn();
    startBootErrorForwarding(send);
    window.dispatchEvent(Object.assign(new Event('error'), { message: 'from the window' }));
    expect(send).toHaveBeenCalledWith('error', 'from the window');
  });

  it('describes errors as one bounded line: message and first frame only', () => {
    const e = new Error('boom');
    e.stack = 'Error: boom\n    at start (main.js:1:2)\n    at more (main.js:3:4)';
    expect(describeError(e)).toBe('Error: boom @ at start (main.js:1:2)');
    const bare = new Error('bare');
    bare.stack = undefined;
    expect(describeError(bare)).toBe('Error: bare');
    expect(describeError('y'.repeat(1000))).toHaveLength(400);
    expect(describeError({ a: 1 })).toBe('[object Object]');
  });
});
