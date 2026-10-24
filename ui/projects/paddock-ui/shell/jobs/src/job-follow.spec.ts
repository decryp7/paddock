import { DestroyRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { JobFollow } from './job-follow';
import { jobsBed, settle, snap } from './spec-helpers';

/** A DestroyRef the test destroys. */
function owner() {
  const callbacks = new Set<() => void>();
  let destroyed = false;
  const ref = {
    onDestroy(cb: () => void) {
      if (destroyed) throw new Error('NG0911: View has already been destroyed.');
      callbacks.add(cb);
      return () => void callbacks.delete(cb);
    },
  } as unknown as DestroyRef;
  return {
    ref,
    destroy() {
      destroyed = true;
      for (const cb of [...callbacks]) cb();
    },
    count: () => callbacks.size,
  };
}

describe('JobFollow', () => {
  it('resolves with the end of the job it started', async () => {
    const { backend, notices } = jobsBed();
    const follow = TestBed.inject(JobFollow);
    await settle();
    const o = owner();
    const started = vi.fn();
    const result = follow.run(() => Promise.resolve(4), o.ref, started);
    await settle();
    expect(started).toHaveBeenCalledWith(4);
    backend.emit(snap(4, 1));
    backend.emit(snap(4, 2, 'done'));
    expect((await result)?.state).toBe('done');
    expect(o.count()).toBe(0);
    expect(notices).toEqual([]);
  });

  it('resolves null when the owner goes first; the job then announces its own end', async () => {
    const { backend, notices } = jobsBed();
    const follow = TestBed.inject(JobFollow);
    await settle();
    backend.emit(snap(1, 1));
    const o = owner();
    const result = follow.follow(1, o.ref);
    o.destroy();
    expect(await result).toBeNull();
    backend.emit(snap(1, 2, 'done'));
    expect(notices.map((n) => n.level)).toEqual(['success']);
  });

  it('resolves null for an owner that is already destroyed', async () => {
    jobsBed();
    const follow = TestBed.inject(JobFollow);
    const o = owner();
    o.destroy();
    expect(await follow.follow(1, o.ref)).toBeNull();
  });

  it('rejects when the work did not start', async () => {
    jobsBed();
    const follow = TestBed.inject(JobFollow);
    await expect(follow.run(() => Promise.reject(new Error('queue full')), owner().ref)).rejects.toThrow('queue full');
  });
});
