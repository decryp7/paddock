import { ChangeDetectionStrategy, Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, onTestFailed, vi } from 'vitest';
import { installLeakDetector, LeakDetector, registerInFlight, registerSingleton, timerOwner, type LeakFinding } from './leaks';

/** The smallest TestBed component, for the tests of what the detector does around TestBed's own clean-up. */
@Component({ selector: 'pd-leak-probe', template: '<p>probe</p>', changeDetection: ChangeDetectionStrategy.OnPush })
class LeakProbe {}

const kinds = (found: LeakFinding[]): string[] => found.map((f) => f.kind);

describe('timerOwner', () => {
  const head = 'Error\n    at wrapper (leaks.ts:1:1)\n';
  it('names the first frame of the code under test', () => {
    expect(timerOwner(`${head}    at Store.schedule (/repo/app/store.ts:12:5)\n    at run (/repo/app/spec.ts:3:1)`)).toBe('Store.schedule (/repo/app/store.ts:12:5)');
  });
  it('does not blame a test for the framework’s own timers', () => {
    expect(timerOwner(`${head}    at MatFormField.ngAfterViewInit (file:///repo/node_modules/@angular/material/fesm2022/form-field.mjs:741:9)`)).toBeNull();
    expect(timerOwner(`${head}    at waitFor (file:///repo/node_modules/vitest/dist/chunks/utils.js:1:1)`)).toBeNull();
  });
  it('looks through rxjs, which only forwards the call', () => {
    expect(timerOwner(`${head}    at AsyncAction.requestAsyncId (/repo/node_modules/rxjs/dist/esm/internal/scheduler/AsyncAction.js:1:1)\n    at Poller.start (/repo/app/poller.ts:9:3)`)).toBe('Poller.start (/repo/app/poller.ts:9:3)');
  });
  it('looks through a spy on the timer global to the code that called it', () => {
    const spied =
      `${head}    at /repo/app/spec.ts:227:88\n    at wrapper (file:///repo/node_modules/vitest/dist/chunks/spy.DDMXghXv.js:353:40)\n` +
      '    at file:///repo/node_modules/@angular/material/fesm2022/form-field.mjs:741:9';
    expect(timerOwner(spied)).toBeNull();
    expect(timerOwner(spied.replace(/\n {4}at file:.*material.*$/, '\n    at Store.schedule (/repo/app/store.ts:12:5)'))).toBe('Store.schedule (/repo/app/store.ts:12:5)');
  });
  it('blames no test when the stack has no frame to read', () => {
    expect(timerOwner(undefined)).toBeNull();
    expect(timerOwner('Error\n    at wrapper (leaks.ts:1:1)')).toBeNull();
  });
});

describe('LeakDetector (the checks, driven by hand)', () => {
  let detector: LeakDetector;
  beforeEach(() => {
    detector = new LeakDetector();
    detector.begin();
  });

  it('does not flag a clean test', () => {
    detector.snapshot(vi);
    expect(detector.finish(false)).toEqual([]);
  });

  it('does not flag a timer that fired, a timer that was cleared, or a frame of a test that used fake timers and removed them', async () => {
    let fired = false;
    setTimeout(() => void (fired = true), 1);
    await vi.waitFor(() => expect(fired).toBe(true));
    clearTimeout(setTimeout(() => undefined, 60_000));
    clearInterval(setInterval(() => undefined, 60_000));
    vi.useFakeTimers();
    setTimeout(() => undefined, 60_000);
    vi.useRealTimers();
    detector.snapshot(vi);
    expect(detector.finish(false)).toEqual([]);
  });

  it('names a live interval and a live timeout the test left, with where they were started', () => {
    const interval = setInterval(() => undefined, 60_000);
    const timeout = setTimeout(() => undefined, 60_001);
    detector.snapshot(vi);
    const found = detector.finish(false);
    clearInterval(interval);
    clearTimeout(timeout);
    expect(kinds(found)).toEqual(['timer', 'timer']);
    expect(found[0]?.message).toMatch(/live setInterval\(60000\) started at .*leaks\.spec\.ts:\d+/);
    expect(found[1]?.message).toMatch(/live setTimeout\(60001\) started at .*leaks\.spec\.ts:\d+/);
  });

  it('blames a leaked timer once and clears it when asked, so no later test sees it', () => {
    let fired = false;
    setTimeout(() => void (fired = true), 5);
    detector.snapshot(vi);
    expect(kinds(detector.finish(true))).toEqual(['timer']);
    expect(detector.finish(true)).toEqual([]);
    expect(fired).toBe(false);
  });

  it('does not blame a test for a timer that already existed when it began', () => {
    const old = setInterval(() => undefined, 60_000);
    detector.begin();
    detector.snapshot(vi);
    const found = detector.finish(false);
    clearInterval(old);
    expect(found).toEqual([]);
  });

  it('names fake timers left installed', () => {
    vi.useFakeTimers();
    detector.snapshot(vi);
    vi.useRealTimers();
    const found = detector.finish(false);
    expect(kinds(found)).toEqual(['fake-timers']);
    expect(found[0]?.message).toContain('fake timers');
  });

  it('names a node left in the body, and leaves TestBed roots and the CDK containers alone', () => {
    const stray = Object.assign(document.createElement('div'), { id: 'pd-stray', className: 'dock active' });
    const root = document.createElement('div');
    root.setAttribute('ng-version', '0');
    const cdk = Object.assign(document.createElement('div'), { className: 'cdk-overlay-container' });
    document.body.append(stray, root, cdk);
    detector.snapshot(vi);
    const found = detector.finish(false);
    stray.remove();
    root.remove();
    cdk.remove();
    expect(kinds(found)).toEqual(['dom']);
    expect(found[0]?.message).toContain('<div#pd-stray.dock.active>');
  });

  it('names a registered singleton that holds state, and resets it', () => {
    let held: string | null = null;
    const remove = registerSingleton({ name: 'probe singleton', leftover: () => held, reset: () => void (held = null) });
    try {
      detector.begin();
      held = '2 running jobs';
      detector.snapshot(vi);
      const found = detector.finish(false);
      expect(kinds(found)).toEqual(['singleton']);
      expect(found[0]?.message).toBe('left "probe singleton" holding state: 2 running jobs');
      expect(held).toBeNull();
      detector.begin();
      detector.snapshot(vi);
      expect(detector.finish(false)).toEqual([]);
    } finally {
      remove();
    }
  });

  it('resets a registered singleton before the test, so an earlier leak is not blamed on this one', () => {
    let held: string | null = 'left over from before';
    const remove = registerSingleton({ name: 'probe singleton', leftover: () => held, reset: () => void (held = null) });
    try {
      detector.begin();
      detector.snapshot(vi);
      expect(detector.finish(false)).toEqual([]);
    } finally {
      remove();
    }
  });

  it('names tracked in-flight work that is still pending', () => {
    let pending = 0;
    const remove = registerInFlight({ name: 'probe loads', pending: () => pending });
    try {
      detector.snapshot(vi);
      expect(detector.finish(false)).toEqual([]);
      pending = 2;
      const found = detector.finish(false);
      expect(kinds(found)).toEqual(['in-flight']);
      expect(found[0]?.message).toBe('left 2 item(s) of "probe loads" still in flight');
    } finally {
      remove();
    }
  });

  it('skips the kinds it is told to skip', () => {
    const skipping = new LeakDetector(['timer', 'fake-timers']);
    skipping.begin();
    const timeout = setTimeout(() => undefined, 60_000);
    vi.useFakeTimers();
    skipping.snapshot(vi);
    vi.useRealTimers();
    const found = skipping.finish(false);
    clearTimeout(timeout);
    expect(found).toEqual([]);
  });

  it('never throws while it snapshots, whatever the state it reads', () => {
    expect(() =>
      detector.snapshot({
        isFakeTimers: () => {
          throw new Error('boom');
        },
      }),
    ).not.toThrow();
  });

  it('reports a failing check as a finding instead of throwing', () => {
    const remove = registerSingleton({
      name: 'broken singleton',
      leftover: () => {
        throw new Error('cannot read');
      },
      reset: () => undefined,
    });
    try {
      expect(detector.finish(false).map((f) => f.message)).toEqual([expect.stringContaining('the detector failed while checking: Error: cannot read')]);
    } finally {
      remove();
    }
  });
});

// The hooks, in `fail` mode: a leaking test fails by name after every afterEach (Angular's clean-up included) has run, the
// test after it is clean, and a throw in the leaking test itself still reaches Angular's clean-up.
describe('installLeakDetector in fail mode', () => {
  installLeakDetector({ mode: 'fail' });
  const failure = { message: '' };
  let fired = false;
  const capture = (): void => onTestFailed(({ task }) => void (failure.message = task.result?.errors?.map((e) => e.message).join('\n') ?? ''));

  it.fails('fails the test that left a timer, and names it', () => {
    capture();
    setTimeout(() => void (fired = true), 20);
  });

  it('clears that timer, so the next test is clean and never sees it fire', async () => {
    expect(failure.message).toContain('This test leaked');
    expect(failure.message).toMatch(/timer: left a live setTimeout\(20\) started at .*leaks\.spec\.ts:\d+/);
    // A later timer fires after the cleared one would have (20 ms): the order of real timers is by deadline.
    let later = false;
    setTimeout(() => void (later = true), 40);
    await vi.waitFor(() => expect(later).toBe(true));
    expect(fired).toBe(false);
  });

  it.fails('fails the test that left a node in the page, and names it', () => {
    capture();
    document.body.append(Object.assign(document.createElement('section'), { id: 'pd-left-section' }));
  });

  it('has the page the leaker left in its report', () => {
    expect(failure.message).toContain('dom: left <section#pd-left-section> in document.body');
    document.getElementById('pd-left-section')?.remove();
  });

  it.fails('fails the test that left fake timers on, and names them', () => {
    capture();
    vi.useFakeTimers();
  });

  it('is back on real timers, having been told by name', () => {
    expect(failure.message).toContain('fake-timers: left fake timers installed');
    vi.useRealTimers();
    expect(vi.isFakeTimers()).toBe(false);
  });

  it('does not flag a clean test', () => {
    expect(1 + 1).toBe(2);
  });

  describe('Angular clean-up', () => {
    afterEach(() => void document.getElementById('pd-left-after-throw')?.remove());

    // The test body throws after it built a TestBed component and left a timer: the detector must not skip TestBed's own
    // clean-up (the next TestBed test would fail with "Cannot configure the test module when the test module has already
    // been instantiated"), and it must still blame the leak.
    it.fails('a throwing test that left a timer still gets its TestBed clean-up', () => {
      capture();
      TestBed.configureTestingModule({ imports: [LeakProbe] });
      TestBed.createComponent(LeakProbe).detectChanges();
      setTimeout(() => undefined, 30);
      throw new Error('this test fails on purpose');
    });

    it('configures the next TestBed test cleanly, and the leaked timer was named next to the failure', () => {
      expect(failure.message).toContain('this test fails on purpose');
      expect(failure.message).toContain('timer: left a live setTimeout(30)');
      TestBed.configureTestingModule({ imports: [LeakProbe] });
      const fixture = TestBed.createComponent(LeakProbe);
      fixture.detectChanges();
      expect((fixture.nativeElement as HTMLElement).textContent).toBe('probe');
    });
  });
});

describe('installLeakDetector in report mode', () => {
  const lines: string[] = [];
  // The runner's stderr, as `process` is not typed in the ui's spec config.
  const stderr = (globalThis as unknown as { process: { stderr: { write: (chunk: string) => boolean } } }).process.stderr;
  const realWrite = stderr.write;
  beforeAll(() => {
    stderr.write = (chunk: string): boolean => {
      lines.push(String(chunk));
      return true;
    };
  });
  afterAll(() => void (stderr.write = realWrite));

  installLeakDetector({ mode: 'report' });
  let timer: ReturnType<typeof setTimeout> | undefined;

  it('leaves a timer and still passes', () => {
    timer = setTimeout(() => undefined, 60_000);
    expect(true).toBe(true);
  });

  it('printed a warning that names the leaking test, and did not fail it', () => {
    clearTimeout(timer);
    expect(lines.join('')).toMatch(/\[leak-detector\] .*leaks\.spec\.ts > installLeakDetector in report mode > leaves a timer and still passes: timer: left a live setTimeout\(60000\)/);
  });
});
