import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { bed, settle } from './spec-helpers';
import { MERGE_WINDOW_MS, UNDO_LIMIT, UNDO_TOAST_MS } from './undo-config';
import { UNDO_NOTICE_KINDS, UndoStore, type UndoableAction } from './undo.store';
import { provideShellUndo } from './provide-undo';

/** A counter whose edits can be undone: `set` records through the store. */
function counter() {
  const state = { value: 0, log: [] as string[] };
  const edit = (to: number, extra: Partial<UndoableAction> = {}): UndoableAction => {
    const from = state.value;
    return {
      label: `set to ${to}`,
      do: () => void (state.value = to, state.log.push(`do ${to}`)),
      undo: () => void (state.value = from, state.log.push(`undo ${from}`)),
      ...extra,
    };
  };
  return { state, edit };
}

function setup() {
  const b = bed([provideShellUndo()]);
  return { ...b, store: TestBed.inject(UndoStore) };
}

describe('UndoStore', () => {
  it('records, undoes and redoes in order, with labels', async () => {
    const { store } = setup();
    const c = counter();
    expect(store.canUndo()).toBe(false);
    expect(store.undoLabel()).toBeNull();
    expect(await store.undo()).toBe(false);
    expect(await store.redo()).toBe(false);
    for (const n of [1, 2]) {
      expect(await store.execute(c.edit(n))).toBe(true);
    }
    expect(c.state.value).toBe(2);
    expect([store.canUndo(), store.canRedo(), store.undoLabel(), store.size()]).toEqual([true, false, 'set to 2', 2]);
    expect(await store.undo()).toBe(true);
    expect([c.state.value, store.undoLabel(), store.redoLabel(), store.canRedo()]).toEqual([1, 'set to 1', 'set to 2', true]);
    expect(await store.redo()).toBe(true);
    expect(c.state.value).toBe(2);
    expect(await store.undo()).toBe(true);
    expect(await store.undo()).toBe(true);
    expect(c.state.value).toBe(0);
    // A new edit drops the redo history.
    await store.execute(c.edit(9));
    expect(store.canRedo()).toBe(false);
  });

  it('records an edit that was already made and does not run it', () => {
    const { store } = setup();
    const c = counter();
    c.state.value = 5;
    store.record({ label: 'edit', do: () => void c.state.log.push('do'), undo: () => void c.state.log.push('undo') });
    expect(c.state.log).toEqual([]);
    expect(store.canUndo()).toBe(true);
  });

  it('merges consecutive edits with the same key inside the window: the first undo, the last do', async () => {
    const { store, clock } = setup();
    const c = counter();
    store.record(c.edit(1, { mergeKey: 'q' }));
    c.state.value = 1;
    clock.advance(MERGE_WINDOW_MS);
    store.record({ ...c.edit(2, { mergeKey: 'q' }) });
    expect(store.size()).toBe(1);
    clock.advance(MERGE_WINDOW_MS + 1);
    store.record(c.edit(3, { mergeKey: 'q' }));
    expect(store.size()).toBe(2);
    store.record(c.edit(4, { mergeKey: 'other' }));
    expect(store.size()).toBe(3);
    // After an undo and redo an entry no longer merges.
    await store.undo();
    await store.redo();
    store.record(c.edit(5, { mergeKey: 'other' }));
    expect(store.size()).toBe(4);
  });

  it('keeps at most UNDO_LIMIT entries, dropping the oldest', () => {
    const { store } = setup();
    for (let i = 0; i < UNDO_LIMIT + 5; i++) store.record({ label: `e${i}`, do: () => undefined, undo: () => undefined });
    expect(store.size()).toBe(UNDO_LIMIT);
    expect(store.undoLabel()).toBe(`e${UNDO_LIMIT + 4}`);
  });

  it('reports a failed edit as a notice, records nothing, and keeps the history where it was', async () => {
    const { store, notices } = setup();
    const ok = await store.execute({ label: 'rename', do: () => undefined, undo: () => undefined });
    expect(ok).toBe(true);
    const failed = await store.execute({
      label: 'move file',
      do: () => {
        throw new Error('the disk is full');
      },
      undo: () => undefined,
    });
    expect(failed).toBe(false);
    expect(store.size()).toBe(1);
    expect(notices).toEqual([{ level: 'error', kind: UNDO_NOTICE_KINDS.failed, message: 'Move file failed: the disk is full' }]);
  });

  it('keeps the entry where it was when an undo or a redo fails, and says why', async () => {
    const { store, notices } = setup();
    let allowed = false;
    await store.execute({
      label: 'profile edit',
      do: () => {
        if (!allowed) throw new Error('refused');
      },
      undo: () => Promise.reject(new Error('the engine refused the old profile')),
    });
    // do() failed at first, so nothing was recorded; make it succeed.
    allowed = true;
    await store.execute({ label: 'profile edit', do: () => undefined, undo: () => Promise.reject('the engine refused the old profile') });
    expect(await store.undo()).toBe(false);
    expect(store.canUndo()).toBe(true);
    expect(store.canRedo()).toBe(false);
    expect(notices.at(-1)?.message).toBe('Could not undo profile edit: the engine refused the old profile');
    let failRedo = false;
    const c = counter();
    await store.execute(
      c.edit(1, {
        do: () => {
          if (failRedo) throw new Error('nope');
          c.state.value = 1;
        },
      }),
    );
    await store.undo();
    failRedo = true;
    expect(await store.redo()).toBe(false);
    expect(store.canRedo()).toBe(true);
    expect(notices.at(-1)?.message).toBe('Could not redo set to 1: nope');
    expect(await store.redo()).toBe(false);
    failRedo = false;
    expect(await store.redo()).toBe(true);
    // An error that is not an Error uses a generic cause.
    store.record({ label: 'odd', do: () => undefined, undo: () => Promise.reject({}) });
    await store.undo();
    expect(notices.at(-1)?.message).toBe('Could not undo odd: something went wrong');
  });

  it('is busy while an undo runs: observers do not record its changes, and a second undo waits', async () => {
    const { store } = setup();
    let release: () => void = () => undefined;
    const observed: boolean[] = [];
    store.record({
      label: 'slow',
      do: () => undefined,
      undo: () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    });
    const first = store.undo();
    expect(store.applying()).toBe(true);
    expect(store.canUndo()).toBe(false);
    store.record({ label: 'echo', do: () => void observed.push(true), undo: () => undefined });
    expect(await store.undo()).toBe(false);
    expect(await store.redo()).toBe(false);
    release();
    expect(await first).toBe(true);
    expect(store.applying()).toBe(false);
    expect(store.size()).toBe(0);
    expect(store.redoLabel()).toBe('slow');
  });

  it('forgets everything on clear and one group on discard', async () => {
    const { store } = setup();
    const noop = { do: () => undefined, undo: () => undefined };
    store.record({ label: 'a', group: 'draft', ...noop });
    store.record({ label: 'b', ...noop });
    store.record({ label: 'c', group: 'draft', ...noop });
    await store.undo();
    store.discard('draft');
    expect([store.size(), store.undoLabel(), store.canRedo()]).toEqual([1, 'b', false]);
    store.clear();
    expect([store.size(), store.canUndo(), store.canRedo()]).toEqual([0, false, false]);
  });

  it('announces an action with a toast that undoes it, and the toast goes by itself', async () => {
    const { store, clock } = setup();
    const c = counter();
    await store.execute(c.edit(1));
    expect(store.toast()).toBeNull();
    await store.execute(c.edit(2, { announce: true }));
    expect(store.toast()?.label).toBe('set to 2');
    expect(clock.pending).toBe(1);
    clock.advance(UNDO_TOAST_MS - 1);
    expect(store.toast()).not.toBeNull();
    clock.advance(1);
    expect(store.toast()).toBeNull();
    expect(clock.pending).toBe(0);
    // A newer toast replaces the older one, and the older timer does not hide it.
    store.record(c.edit(3, { announce: true }));
    clock.advance(UNDO_TOAST_MS - 100);
    store.record(c.edit(4, { announce: true }));
    expect(store.toast()?.label).toBe('set to 4');
    clock.advance(200);
    expect(store.toast()?.label).toBe('set to 4');
    // Undoing hides the toast; so does the close button and clear.
    await store.undo();
    expect(store.toast()).toBeNull();
    store.record(c.edit(5, { announce: true }));
    store.dismissToast();
    expect(store.toast()).toBeNull();
    store.dismissToast();
    store.record(c.edit(6, { announce: true }));
    store.clear();
    expect(store.toast()).toBeNull();
    expect(clock.pending).toBe(0);
    await settle();
  });
});
