import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { flush, makeMock } from '../../testing/test-helpers';
import { JobsStore } from './jobs.store';
import { ToastStore } from './toast.store';

function setup(options = {}) {
  const backend = makeMock(options);
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
  return { backend, store: TestBed.inject(JobsStore), toasts: TestBed.inject(ToastStore) };
}

describe('JobsStore', () => {
  it('shows a job from the moment it begins, before the engine has said anything', async () => {
    const { store } = setup({ jobStepMs: 5 });
    let seen: unknown = 'unset';
    const done = store.run('download', 'Downloading Lumen 1.0', async (id) => {
      seen = store.jobs().find((j) => j.id === id);
      await flush();
      return 'ok';
    });
    await flush();
    expect(store.jobs()).toHaveLength(1);
    expect(store.jobs()[0]).toMatchObject({ kind: 'download', title: 'Downloading Lumen 1.0', progress: null, cancelling: false });
    await expect(done).resolves.toBe('ok');
    expect(seen).toMatchObject({ kind: 'download' });
    expect(store.jobs()).toHaveLength(0);
  });

  it('follows the phases of a job and forgets it when it ends', async () => {
    const { backend, store } = setup();
    const phases: (string | undefined)[] = [];
    await store.run('installRenderer', 'Installing the browser', async (id) => {
      // Subscribed after the store, so it sees the store's state after each event.
      await backend.onJob((e) => {
        if (e.state === 'running') phases.push(store.jobs()[0]?.progress?.label);
      });
      return backend.installRenderer(id);
    });
    expect(phases).toContain('Downloading');
    expect(phases).toContain('Unpacking');
    expect(phases).toContain('Testing');
    expect(store.jobs()).toHaveLength(0);
  });

  it('ends a job when its command fails', async () => {
    const { store } = setup();
    await expect(store.run('download', 'x', () => Promise.reject(new Error('no')))).rejects.toThrow('no');
    expect(store.jobs()).toHaveLength(0);
  });

  it('cancels a running job through the backend and marks it cancelling', async () => {
    const { backend, store } = setup({ jobStepMs: 20 });
    const version = (await backend.getWatch('w-1')).history[0]!.version;
    const run = store.run('download', 'Downloading', async (id) => backend.downloadRelease('w-1', version, id));
    await new Promise((r) => setTimeout(r, 60));
    const id = store.jobs()[0]?.id as string;
    expect(id).toBeTruthy();
    await store.cancel(id);
    expect(store.jobs()[0]?.cancelling).toBe(true);
    await expect(run).rejects.toMatchObject({ code: 'cancelled' });
    expect(store.jobs()).toHaveLength(0);
    expect(backend.calls).toContain('cancelJob');
  });

  it('keeps a detached job (check all) until the engine says it ended, and says when it was cancelled', async () => {
    const { backend, store, toasts } = setup({ jobStepMs: 15 });
    const id = await store.start('checkAll', 'Checking every watch', (jobId) => backend.checkNow(null, jobId));
    expect(store.jobs().map((j) => j.id)).toEqual([id]);
    await new Promise((r) => setTimeout(r, 40));
    expect(store.jobs()[0]?.progress).toMatchObject({ phase: 'checking', unit: 'count' });
    expect(store.jobs()[0]?.progress?.total).toBeGreaterThan(1);
    await store.cancel(id);
    await new Promise((r) => setTimeout(r, 60));
    expect(store.jobs()).toHaveLength(0);
    expect(toasts.toasts().at(-1)?.title).toBe('Check cancelled');
  });

  it('a detached job that finishes simply goes away', async () => {
    const { backend, store, toasts } = setup();
    await store.start('checkAll', 'Checking every watch', (jobId) => backend.checkNow(null, jobId));
    await flush();
    expect(store.jobs()).toHaveLength(0);
    expect(toasts.toasts()).toHaveLength(0);
  });

  it('removes a detached job whose command could not even start', async () => {
    const { store } = setup();
    await expect(store.start('checkAll', 'x', () => Promise.reject(new Error('refused')))).rejects.toThrow('refused');
    expect(store.jobs()).toHaveLength(0);
  });

  it('ignores events of jobs it does not know and survives a failed subscription', async () => {
    const { backend, store } = setup();
    vi.spyOn(backend, 'onJob').mockRejectedValue(new Error('no events'));
    await expect(store.run('check', 'Checking', async () => 7)).resolves.toBe(7);
  });

  it('gives every job its own id of letters, digits and dashes', async () => {
    const { store } = setup({ jobStepMs: 5 });
    const ids: string[] = [];
    await Promise.all([store.run('check', 'a', async (id) => ids.push(id)), store.run('check', 'b', async (id) => ids.push(id))]);
    expect(new Set(ids).size).toBe(2);
    expect(ids.every((id) => /^[A-Za-z0-9_-]{1,64}$/.test(id))).toBe(true);
  });
});
