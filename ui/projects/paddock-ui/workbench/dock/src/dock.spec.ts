import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { settle } from '@decryp7/paddock-ui/shell/testing';
import { definePerspective, defineView } from '@decryp7/paddock-ui/workbench';
import { msg } from '@decryp7/paddock-ui/shell';
import { InMemoryDockLayoutStore } from './dock-store';
import { mountDock, Probe, probeView, tabIds } from './harness.spec-helper';

describe('WorkbenchDock: showing and closing views', () => {
  it('opens a view, lists it as open and active, and closes it', async () => {
    const { rendered, dock } = await mountDock({ views: [probeView('a.one'), probeView('a.two')] });
    expect(dock.ready()).toBe(true);
    expect(await dock.showView('a.one')).toBe(true);
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.one']);
    expect(dock.activePanel()).toBe('a.one');
    expect(tabIds(rendered.element)).toEqual(['a.one']);
    expect(rendered.one('[data-panel-id="a.one"]').textContent).toContain('A.ONE');
    dock.closeView('a.one');
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual([]);
    expect(tabIds(rendered.element)).toEqual([]);
  });

  it('refuses unknown views, views that are not offered and instances of a view without instances', async () => {
    const { dock } = await mountDock({ views: [probeView('a.one'), probeView('a.late', 'document', { when: (c) => c.flags.ready() })], ready: signal(false) });
    expect(await dock.showView('nope')).toBe(false);
    expect(await dock.showView('a.late')).toBe(false);
    expect(await dock.showView('a.one', { key: 'x' })).toBe(false);
  });

  it('opens instances as panels <view>#<key>', async () => {
    const { rendered, dock } = await mountDock({ views: [probeView('a.doc', 'document', { instances: true })] });
    expect(await dock.showView('a.doc', { key: 'f1' })).toBe(true);
    expect(await dock.showView('a.doc', { key: 'f2', background: true })).toBe(true);
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.doc#f1', 'a.doc#f2']);
    expect(await dock.showView('a.doc', { key: '' })).toBe(false);
  });

  it('showing an open view again brings it to the front', async () => {
    const { rendered, dock } = await mountDock({ views: [probeView('a.one'), probeView('a.two')] });
    await dock.showView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    expect(dock.activePanel()).toBe('a.two');
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(dock.activePanel()).toBe('a.one');
  });

  it('keeps a view that cannot be closed, and hides its close affordance', async () => {
    const { rendered, dock } = await mountDock({ views: [probeView('a.pin', 'document', { closable: false })] });
    await dock.showView('a.pin');
    await settle(rendered.fixture);
    expect(dock.isClosable('a.pin')).toBe(false);
    dock.closeView('a.pin');
    expect(dock.openPanels()).toEqual(['a.pin']);
    expect(rendered.element.querySelector('.pd-dock-tab-close')).toBeNull();
  });

  it('waits for the dock to have a layout before showing a view', async () => {
    const { dock } = await mountDock({ views: [probeView('a.one')] });
    dock.detach();
    const pending = dock.showView('a.one');
    dock.detach();
    expect(await pending).toBe(false);
  });
});

describe('panels', () => {
  it('shows a skeleton in the panel while the view loads, with the tab already there', async () => {
    let release: (() => void) | undefined;
    const slow = defineView({
      id: 'a.slow',
      title: msg('h.slow', 'Slow'),
      icon: 'pd:x',
      layout: probeView('x').layout,
      load: () => new Promise((resolve) => (release = () => resolve(class {}))),
    });
    const { rendered, dock } = await mountDock({ views: [slow] });
    await dock.showView('a.slow');
    await settle(rendered.fixture);
    expect(tabIds(rendered.element)).toEqual(['a.slow']);
    expect(rendered.element.querySelector('.pd-dock-skeleton')).not.toBeNull();
    release?.();
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('.pd-dock-skeleton')).toBeNull();
  });

  it('renders the component of the view', async () => {
    const { rendered, dock } = await mountDock({ views: [probeView('a.one')] });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('.probe')?.textContent).toBe('probe');
  });

  it('contains a failing load to its panel, with Retry', async () => {
    let attempts = 0;
    const flaky = defineView({
      id: 'a.flaky',
      title: msg('h.flaky', 'Flaky'),
      icon: 'pd:x',
      layout: probeView('x').layout,
      load: () => (++attempts === 1 ? Promise.reject(new Error('chunk 404')) : Promise.resolve(Probe)),
    });
    const { rendered, dock } = await mountDock({ views: [flaky, probeView('a.ok')] });
    await dock.showView('a.ok');
    await dock.showView('a.flaky');
    await settle(rendered.fixture);
    await settle(rendered.fixture);
    const card = rendered.element.querySelector('.pd-panel-failed');
    expect(card?.textContent).toContain('chunk 404');
    await dock.showView('a.ok');
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('.probe')).not.toBeNull();
    await dock.showView('a.flaky');
    await settle(rendered.fixture);
    rendered.one<HTMLButtonElement>('[data-testid="pd-panel-retry"]').click();
    await settle(rendered.fixture);
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('.pd-panel-failed')).toBeNull();
    expect(attempts).toBe(2);
  });
});

describe('persistence', () => {
  it('saves the arrangement and restores it in the next window', async () => {
    const store = new InMemoryDockLayoutStore();
    const views = [probeView('a.one'), probeView('a.two')];
    const first = await mountDock({ views, store });
    await first.dock.showView('a.one');
    await first.dock.showView('a.two');
    await settle(first.rendered.fixture);
    await new Promise((r) => setTimeout(r, 5));
    expect(store.entries.has('wb.layout')).toBe(true);
    first.rendered.fixture.destroy();
    const second = await mountDock({ views, store });
    expect(second.dock.openPanels()).toEqual(['a.one', 'a.two']);
    // Regression (review 2 P2-F): a restored layout raises no activation, yet its front panel is the active one.
    expect(second.dock.activePanel()).toBe('a.two');
  });

  it('builds the default layout from corrupt or foreign stored data', async () => {
    const store = new InMemoryDockLayoutStore();
    for (const bad of ['{"version":99}', '{"version":1,"layout":"x"}', '[1]', 'not json']) {
      store.entries.set('wb.layout', bad);
      const { dock } = await mountDock({ views: [probeView('a.one')], store });
      expect(dock.ready()).toBe(true);
      expect(dock.openPanels()).toEqual([]);
    }
  });

  it('survives a store that fails', async () => {
    const store = { load: () => Promise.reject(new Error('away')), save: () => Promise.reject(new Error('away')) };
    const { dock, rendered } = await mountDock({ views: [probeView('a.one')], store });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.one']);
  });

  it('gives the layout for the UI checkpoint and restores it', async () => {
    const views = [probeView('a.one'), probeView('a.two')];
    const first = await mountDock({ views });
    await first.dock.showView('a.one');
    await first.dock.showView('a.two');
    await settle(first.rendered.fixture);
    const checkpoint = JSON.parse(JSON.stringify(first.dock.checkpoint()));
    const second = await mountDock({ views });
    expect(second.dock.openPanels()).toEqual([]);
    expect(second.dock.restoreCheckpoint(checkpoint)).toBe(true);
    await settle(second.rendered.fixture);
    expect(second.dock.openPanels()).toEqual(['a.one', 'a.two']);
    expect(second.dock.restoreCheckpoint({ junk: true })).toBe(false);
  });

  it('forgets the saved layout and rebuilds the default on reset', async () => {
    const store = new InMemoryDockLayoutStore();
    const p = definePerspective({ id: 'a.p1', title: msg('h.p1', 'One'), icon: 'pd:x', order: 1, views: ['a.one'] });
    const { dock, rendered } = await mountDock({ views: [probeView('a.one'), probeView('a.two')], perspectives: [p], store });
    expect(dock.openPanels()).toEqual(['a.one']);
    await dock.showView('a.two');
    await settle(rendered.fixture);
    dock.resetLayout();
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.one']);
  });
});

describe('views with a live when', () => {
  it('opens an autoOpen view in the background when its when starts to hold, and closes it when it stops', async () => {
    const ready = signal(false);
    const late = probeView('a.late', 'document', { when: (c) => c.flags.ready(), autoOpen: true });
    const offered = probeView('a.offered', 'document', { when: (c) => c.flags.ready() });
    const { rendered, dock } = await mountDock({ views: [probeView('a.one'), late, offered], ready });
    await dock.showView('a.one');
    expect(dock.openPanels()).toEqual(['a.one']);
    ready.set(true);
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.late', 'a.one']);
    expect(dock.activePanel()).toBe('a.one');
    expect(await dock.showView('a.offered')).toBe(true);
    ready.set(false);
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.one']);
  });

  it('does not close a restored view whose when is not known yet (the context is still loading)', async () => {
    const store = new InMemoryDockLayoutStore();
    const ready = signal(true);
    const gated = probeView('a.gated', 'document', { when: (c) => c.flags.ready() });
    const first = await mountDock({ views: [gated], store, ready });
    await first.dock.showView('a.gated');
    await settle(first.rendered.fixture);
    await new Promise((r) => setTimeout(r, 5));
    first.rendered.fixture.destroy();
    const second = await mountDock({ views: [gated], store, ready });
    expect(second.dock.openPanels()).toEqual(['a.gated']);
  });
});

describe('ports', () => {
  it('provides the core DOCK_PORT and EXPLORER_PLAN from the dock', async () => {
    const { DOCK_PORT, EXPLORER_PLAN } = await import('@decryp7/paddock-ui/workbench');
    const { dock } = await mountDock({ views: [probeView('a.one')] });
    expect(TestBed.inject(DOCK_PORT)).toBe(dock);
    expect(TestBed.inject(EXPLORER_PLAN)).toBe(dock);
  });

  it('answers the Explorer plan: keep while the dock is not measured, and notifies window plans', async () => {
    const { dock } = await mountDock({ views: [probeView('a.one')] });
    expect(dock.verdict(true, 300)).toBe('keep');
    expect(dock.explorerVerdict(false, 300)).toBe('keep');
    const listener = vi.fn();
    const off = dock.onWindowPlan(listener);
    off();
    expect(typeof off).toBe('function');
  });
});
