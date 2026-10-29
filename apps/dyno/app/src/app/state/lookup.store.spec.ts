import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { jobsProviders, mockWithJobs, pump } from '../../testing/job-test-support';
import { LookupStore } from './lookup.store';

function setup() {
  const { mock, jobs } = mockWithJobs();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, jobsProviders(jobs)] });
  return { mock, store: TestBed.inject(LookupStore) };
}

describe('LookupStore', () => {
  it('loads the redacted starting text of a search', async () => {
    const { store } = setup();
    await store.loadSeed('Slack for alex at 10.0.0.5');
    expect(store.seed()).toEqual({ text: 'Slack for at', removed: ['1 user name', '1 IP address'] });
    expect(store.busy()).toBe(false);
    expect(store.error()).toBeNull();
  });

  it('opens a search for exactly the confirmed text, and reports a refusal', async () => {
    const { store, mock } = setup();
    expect(await store.search('slack high cpu')).toBe(true);
    expect(mock.opened).toEqual(['https://duckduckgo.com/?q=slack+high+cpu']);
    expect(await store.search('   ')).toBe(false);
    expect(store.error()?.code).toBe('badRequest');
    expect(mock.opened).toHaveLength(1);
  });

  it('computes a hash and opens VirusTotal with only the hash', async () => {
    const { store, mock } = setup();
    await store.loadHash({ path: '/Applications/Slack.app/slack' });
    const h = store.hash()!;
    expect(h.sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(await store.openVirusTotal(h.sha256)).toBe(true);
    expect(mock.opened).toEqual([`https://www.virustotal.com/gui/file/${h.sha256}`]);
    expect(await store.openVirusTotal('nope')).toBe(false);
    expect(store.error()?.message).toContain('64 hexadecimal');
  });

  it('a failed hash keeps no stale result, and reset clears everything', async () => {
    const { store } = setup();
    await store.loadHash({ path: '/a/b' });
    expect(store.hash()).not.toBeNull();
    await store.loadHash({ path: 'relative' });
    expect(store.hash()).toBeNull();
    expect(store.error()?.message).toBe('That is not a full path to a file.');
    store.reset();
    expect(store.error()).toBeNull();
    expect(store.seed()).toBeNull();
  });

  describe('reading a file as a job', () => {
    afterEach(() => vi.useRealTimers());

    it('shows the bytes read so far from the shell\'s job status', async () => {
      vi.useFakeTimers();
      const { store, mock } = setup();
      mock.jobSteps = 4;
      mock.jobStepMs = 500;
      const done = store.loadHash({ path: '/a/big.bin' });
      await pump(500);
      expect(store.busy()).toBe(true);
      expect(store.hashJob()?.snapshot).toMatchObject({ title: 'Hashing a file', state: 'running', phase: { id: 'hash', unit: 'bytes' } });
      await pump(3000);
      await done;
      expect(store.hash()?.sha256).toMatch(/^[0-9a-f]{64}$/);
      expect(store.hashJob()).toBeNull();
    });

    it('never times out a big file: "still working" after the threshold, no error', async () => {
      vi.useFakeTimers();
      const { store, mock } = setup();
      mock.jobsStalled = true;
      const done = store.loadHash({ path: '/a/big.bin' });
      await pump(0);
      await pump(5 * 60_000);
      expect(store.error()).toBeNull();
      expect(store.hashJob()?.stalled).toBe(true);
      void done;
    });

    it('Cancel stops the read at the engine and leaves a message the dialog can retry from', async () => {
      vi.useFakeTimers();
      const { store, mock } = setup();
      mock.jobSteps = 10;
      mock.jobStepMs = 500;
      const cancel = vi.spyOn(mock, 'cancelJob');
      const done = store.loadHash({ path: '/a/big.bin' });
      await pump(500);
      await store.cancelHash();
      expect(cancel).toHaveBeenCalledTimes(1);
      await pump(0);
      await done;
      expect(store.hash()).toBeNull();
      expect(store.busy()).toBe(false);
      expect(store.hashJob()).toBeNull();
      expect(store.error()?.message).toContain('stopped');
    });

    it('closing the dialog (reset) stops a read in progress and the late answer is ignored', async () => {
      vi.useFakeTimers();
      const { store, mock } = setup();
      mock.jobSteps = 10;
      mock.jobStepMs = 500;
      const cancel = vi.spyOn(mock, 'cancelJob');
      const done = store.loadHash({ path: '/a/big.bin' });
      await pump(500);
      store.reset();
      await pump(1000);
      await done;
      expect(cancel).toHaveBeenCalledTimes(1);
      expect(store.error()).toBeNull();
      expect(store.hash()).toBeNull();
    });
  });
});
