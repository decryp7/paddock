import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { WorkspaceStore } from '../core/workspace.store';
import { provideTinyBackend } from '../../testing/helpers';
import { DockService } from '../dock/dock.service';
import { EXPLORER_WIDTH, UiPrefsStore } from '../core/workspace/ui-prefs.store';
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

  it('closes an Explorer the user opens by hand while there is no room (there is no overlay mode), and reopens it when there is', () => {
    const { narrow, store, set } = setup();
    set('hide');
    narrow.evaluate();
    store.toggleSidebar();
    narrow.evaluate();
    expect(store.sidebarOpen()).toBe(false);
    set('show');
    narrow.evaluate();
    expect(store.sidebarOpen()).toBe(true);
  });

  it('tells the dock how much the Explorer can give up (its width less its minimum, nothing when closed)', () => {
    const { narrow, store } = setup();
    const spy = vi.spyOn(TestBed.inject(DockService), 'setExplorerYield');
    narrow.evaluate();
    expect(spy).toHaveBeenLastCalledWith(narrow.width() - EXPLORER_WIDTH.min);
    store.toggleSidebar();
    narrow.evaluate();
    expect(spy).toHaveBeenLastCalledWith(0);
  });

  it('a width the user chose is shown within the whole room and takes nothing from the dock\'s yield; the default width yields to the other columns', () => {
    const { narrow } = setup();
    const dock = TestBed.inject(DockService);
    const spy = vi.spyOn(dock, 'setExplorerYield');
    vi.spyOn(dock, 'explorerRoom').mockReturnValue(600);
    vi.spyOn(dock, 'explorerColumnRoom').mockReturnValue(250);
    narrow.evaluate();
    narrow.evaluate(); // the rooms found by the first call are the ones the second shows
    expect(narrow.width()).toBe(250);
    expect(spy).toHaveBeenLastCalledWith(250 - EXPLORER_WIDTH.min);
    TestBed.inject(UiPrefsStore).setExplorerWidth(500);
    narrow.evaluate();
    expect(narrow.width()).toBe(500);
    expect(spy).toHaveBeenLastCalledWith(0);
    // Dragged back to the default width it is still chosen; the reset makes it yield again.
    TestBed.inject(UiPrefsStore).setExplorerWidth(EXPLORER_WIDTH.default);
    narrow.evaluate();
    expect(spy).toHaveBeenLastCalledWith(0);
    TestBed.inject(UiPrefsStore).resetExplorerWidth();
    narrow.evaluate();
    narrow.evaluate();
    expect(narrow.width()).toBe(250);
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

  it('asks the plan about the width the user chose, open or closed (a closed Explorer must not be judged at the default width)', () => {
    const { narrow, store, verdicts } = setup();
    TestBed.inject(UiPrefsStore).setExplorerWidth(640);
    narrow.evaluate();
    expect(verdicts).toHaveBeenLastCalledWith(true, 640);
    store.toggleSidebar();
    narrow.evaluate();
    expect(verdicts).toHaveBeenLastCalledWith(false, 640);
  });

  it('does not judge the Explorer in the middle of a drag, only once on release', () => {
    const { narrow, store, set, verdicts } = setup();
    set('hide');
    narrow.hold();
    narrow.evaluate();
    narrow.evaluate();
    expect(verdicts).not.toHaveBeenCalled();
    expect(store.sidebarOpen()).toBe(true);
    narrow.release();
    expect(verdicts).toHaveBeenCalledTimes(1);
    expect(store.sidebarOpen()).toBe(false);
  });
});
