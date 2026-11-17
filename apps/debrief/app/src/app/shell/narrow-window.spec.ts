import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { WorkspaceStore } from '../core/workspace.store';
import { provideTinyBackend } from '../../testing/helpers';
import { DockService } from '../dock/dock.service';
import { NarrowWindow } from './narrow-window';

type Verdict = 'hide' | 'show' | 'keep';

function setup() {
  TestBed.configureTestingModule({ providers: [provideTinyBackend()] });
  const dock = TestBed.inject(DockService);
  let verdict: Verdict = 'keep';
  const verdicts = vi.spyOn(dock, 'explorerVerdict').mockImplementation(() => verdict);
  const listeners: (() => void)[] = [];
  vi.spyOn(dock, 'onWindowPlan').mockImplementation((l) => {
    listeners.push(l);
    return () => undefined;
  });
  return { narrow: TestBed.inject(NarrowWindow), store: TestBed.inject(WorkspaceStore), set: (v: Verdict) => (verdict = v), listeners, verdicts };
}

describe('NarrowWindow (the Explorer follows the layout plan)', () => {
  it('closes the Explorer when the plan has no room for it and brings it back when there is', () => {
    const { narrow, store, set } = setup();
    expect(store.sidebarOpen()).toBe(true);
    set('hide');
    narrow.evaluate();
    expect(store.sidebarOpen()).toBe(false);
    set('keep');
    narrow.evaluate();
    expect(store.sidebarOpen()).toBe(false);
    set('show');
    narrow.evaluate();
    expect(store.sidebarOpen()).toBe(true);
  });

  it('does not reopen an Explorer the user had closed', () => {
    const { narrow, store, set } = setup();
    store.toggleSidebar();
    set('hide');
    narrow.evaluate();
    set('show');
    narrow.evaluate();
    expect(store.sidebarOpen()).toBe(false);
  });

  it('leaves an Explorer the user opened by hand while there is no room, until it fits again', () => {
    const { narrow, store, set } = setup();
    set('hide');
    narrow.evaluate();
    store.toggleSidebar();
    narrow.evaluate();
    expect(store.sidebarOpen()).toBe(true);
    narrow.evaluate();
    expect(store.sidebarOpen()).toBe(true);
    set('keep');
    narrow.evaluate();
    set('hide');
    narrow.evaluate();
    expect(store.sidebarOpen()).toBe(false);
  });

  it('re-evaluates after every dock re-plan, and starts once', () => {
    const { narrow, store, set, listeners, verdicts } = setup();
    narrow.start();
    narrow.start();
    expect(listeners).toHaveLength(1);
    set('hide');
    listeners[0]?.();
    expect(store.sidebarOpen()).toBe(false);
    expect(verdicts).toHaveBeenCalled();
  });

  describe('the state when data first arrives (the layout plan decides, no constant of ours)', () => {
    const withVerdict = (v: Verdict) => {
      const base = setup();
      const follow = vi.spyOn(TestBed.inject(DockService), 'followContainer').mockImplementation(() => undefined);
      base.set(v);
      return { ...base, follow };
    };

    it('lays the dock out first, asks the plan with the Explorer open, and closes it when the plan has no room', () => {
      const { narrow, store, follow, verdicts } = withVerdict('hide');
      expect(narrow.decideAtStart()).toBe(true);
      expect(follow).toHaveBeenCalled();
      expect(verdicts).toHaveBeenCalledWith(true, expect.any(Number));
      expect(store.sidebarOpen()).toBe(false);
    });

    it('leaves the Explorer open when the plan keeps it', () => {
      const { narrow, store } = withVerdict('keep');
      expect(narrow.decideAtStart()).toBe(false);
      expect(store.sidebarOpen()).toBe(true);
    });

    it('brings the Explorer back later when the plan allows, as if the service had closed it', () => {
      const { narrow, store, set } = withVerdict('hide');
      narrow.decideAtStart();
      expect(store.sidebarOpen()).toBe(false);
      set('show');
      narrow.evaluate();
      expect(store.sidebarOpen()).toBe(true);
    });

    it('does nothing when the Explorer is closed already', () => {
      const { narrow, store, follow } = withVerdict('hide');
      store.toggleSidebar();
      expect(narrow.decideAtStart()).toBe(false);
      expect(follow).not.toHaveBeenCalled();
    });
  });
});
