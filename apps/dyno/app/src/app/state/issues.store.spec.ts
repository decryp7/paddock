import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { JOB_POLL_MS } from '../core/job-runner';
import { freezeJobs } from '../../testing/job-test-support';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { IssuesStore } from './issues.store';
import { UnusualStore } from './unusual.store';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, issues: TestBed.inject(IssuesStore), unusual: TestBed.inject(UnusualStore), notifier: TestBed.inject(Notifier) };
}

describe('IssuesStore', () => {
  it('loads and switches the window', async () => {
    const { issues, mock } = setup();
    await issues.load();
    expect(issues.overview()?.days).toBe(14);
    const spy = vi.spyOn(mock, 'issues');
    await issues.setDays(14);
    expect(spy).not.toHaveBeenCalled();
    await issues.setDays(7);
    expect(issues.overview()?.days).toBe(7);
  });

  it('keeps the engine message and recovers on retry', async () => {
    const { issues, mock } = setup();
    const spy = vi.spyOn(mock, 'issues').mockRejectedValueOnce({ code: 'busy', message: 'later' });
    await issues.load();
    expect(issues.error()).toEqual({ code: 'busy', message: 'later' });
    spy.mockRestore();
    await issues.load();
    expect(issues.error()).toBeNull();
  });

  it('ignores a slow answer for a window the person left', async () => {
    const { issues, mock } = setup();
    let release!: () => void;
    const real = mock.issues.bind(mock);
    vi.spyOn(mock, 'issues').mockImplementationOnce((q) => new Promise((res) => (release = () => res(real(q)))));
    const slow = issues.load();
    await issues.setDays(30);
    release();
    await slow;
    expect(issues.overview()?.days).toBe(30);
  });

  describe('rescan as a job', () => {
    afterEach(() => vi.useRealTimers());

    it('rescans once at a time and tells the person', async () => {
      const { issues, mock, notifier } = setup();
      const spy = vi.spyOn(mock, 'startJob');
      await Promise.all([issues.rescan(), issues.rescan()]);
      expect(spy).toHaveBeenCalledTimes(1);
      expect(issues.rescanning()).toBe(false);
      expect(issues.rescanJob()).toBeNull();
      expect(notifier.recent().at(-1)?.text).toContain('Read the system logs');
      spy.mockRejectedValueOnce({ code: 'busy', message: 'Reading system logs is already running; wait for it or cancel it.' });
      await issues.rescan();
      expect(issues.error()?.message).toContain('already running');
      expect(issues.rescanning()).toBe(false);
    });

    it('shows the progress, never a timeout error, and keeps the page while it reads', async () => {
      vi.useFakeTimers();
      const { issues, mock } = setup();
      await issues.load();
      mock.jobPolls = 4;
      const before = issues.overview();
      const done = issues.rescan();
      await vi.advanceTimersByTimeAsync(0);
      expect(issues.rescanning()).toBe(true);
      expect(issues.rescanJob()?.snapshot).toMatchObject({ title: 'Reading system logs', state: 'running' });
      await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
      expect(issues.rescanJob()?.snapshot).toMatchObject({ state: 'running', phase: { id: 'read', total: 4 } });
      expect(issues.overview()).toBe(before);
      // Far longer than the old 160 s limit: still working, no error.
      freezeJobs(mock);
      await vi.advanceTimersByTimeAsync(5 * 60_000);
      expect(issues.error()).toBeNull();
      expect(issues.rescanning()).toBe(true);
      expect(issues.rescanJob()?.stalled).toBe(true);
      void done;
    });

    it('Cancel stops the read at the engine, keeps what the page had and says so', async () => {
      vi.useFakeTimers();
      const { issues, mock, notifier } = setup();
      await issues.load();
      mock.jobPolls = 10;
      const before = issues.overview();
      const cancel = vi.spyOn(mock, 'cancelJob');
      const done = issues.rescan();
      await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
      await issues.cancelRescan();
      expect(cancel).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
      await done;
      expect(issues.rescanning()).toBe(false);
      expect(issues.rescanJob()).toBeNull();
      expect(issues.overview()).toBe(before);
      expect(issues.error()).toBeNull();
      expect(notifier.recent().at(-1)?.text).toBe('Stopped reading the system logs');
    });
  });

  it('exports for Debrief, and a cancelled or failed dialog is handled', async () => {
    const { issues, mock, notifier } = setup();
    await issues.exportForDebrief();
    expect(issues.saved()?.path).toMatch(/\.jsonl$/);
    expect(notifier.recent().at(-1)?.text).toContain('dyno-issues.jsonl');
    const spy = vi.spyOn(mock, 'saveIssues').mockResolvedValueOnce(null);
    const before = issues.saved();
    await issues.exportForDebrief();
    expect(issues.saved()).toBe(before);
    spy.mockRejectedValueOnce({ code: 'accessDenied', message: 'no' });
    await issues.exportForDebrief();
    expect(issues.saveError()?.message).toBe('no');
    expect(issues.exporting()).toBe(false);
  });
});

describe('UnusualStore', () => {
  it('lists the new programs and reports a failure without breaking the list', async () => {
    const { unusual, mock } = setup();
    await unusual.load();
    expect(unusual.overview()!.items.length).toBeGreaterThan(0);
    vi.spyOn(mock, 'unusual').mockRejectedValueOnce({ code: 'busy', message: 'later' });
    await unusual.load();
    expect(unusual.error()?.message).toBe('later');
  });

  it('forgetting starts learning again; a failure is shown', async () => {
    const { unusual, mock, notifier } = setup();
    await unusual.forget();
    expect(unusual.overview()).toMatchObject({ known: 0 });
    vi.spyOn(mock, 'forgetBaseline').mockRejectedValueOnce({ code: 'internal', message: 'bad' });
    await unusual.forget();
    expect(notifier.recent().at(-1)?.text).toBe('bad');
  });

});
