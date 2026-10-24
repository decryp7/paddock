import { TestBed } from '@angular/core/testing';
import { settle } from '@decryp7/paddock-ui/shell/testing';
import { DockContext } from './dock-context';
import { GUIDE } from './dock-geometry';
import { dockview, forgetDockviewForTest } from './dockview-loader';
import { mountDock, probeView } from './harness.spec-helper';

// Spec files usually run in their own module registry, but a worker can be reused: the test starts from "not loaded" itself.
describe('attaching before dockview has loaded', () => {
  beforeEach(() => forgetDockviewForTest());

  it('attaches as soon as dockview is there, and only to the container asked for last', async () => {
    expect(dockview()).toBeNull();
    const { dock, rendered } = await mountDock({ views: [probeView('a.one')], preload: false });
    const ctx = TestBed.inject(DockContext);
    // The harness window asked while dockview was loading: it is attached once it is there.
    await vi.waitFor(() => expect(ctx.api).not.toBeNull());
    await settle(rendered.fixture);
    dock.detach();
    expect(ctx.api).toBeNull();
    const first = document.createElement('div');
    const second = document.createElement('div');
    document.body.append(first, second);
    // Loaded now: attaching is synchronous.
    dock.attach(first);
    expect(ctx.container).toBe(first);
    dock.detach();
    dock.attach(second);
    expect(ctx.container).toBe(second);
    dock.detach();
    first.remove();
    second.remove();
  });
});

describe('the drop position resolver', () => {
  type Resolve = (args: { x: number; y: number; width: number; height: number; zones: ReadonlySet<string>; event?: { clientX?: number; clientY?: number } }) => unknown;

  async function resolver() {
    const { dock } = await mountDock({ views: [probeView('a.one')] });
    const ctx = TestBed.inject(DockContext);
    const api = ctx.api as unknown as { component: { options: { dropPositionResolver: { resolve: Resolve } } } };
    const container = ctx.container!;
    container.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, right: 1000, bottom: 600, width: 1000, height: 600, toJSON: () => ({}) }) as DOMRect;
    return { resolve: api.component.options.dropPositionResolver.resolve, dock };
  }

  it('docks to the layout edge by the guides at the edges of the whole layout, and by the compass inside a group', async () => {
    const { resolve } = await resolver();
    const lx = GUIDE.edgeInset + GUIDE.cell / 2;
    const zones = new Set(['left', 'right', 'top', 'bottom', 'center']);
    // The target is the whole layout (1000 x 600): the edge guides.
    expect(resolve({ x: lx, y: 300, width: 1000, height: 600, zones, event: { clientX: lx, clientY: 300 } })).toEqual({ position: 'left' });
    // A group's content: the compass in its middle, and the layout's edge guide when the pointer is at the window's edge.
    expect(resolve({ x: 100, y: 100, width: 200, height: 200, zones, event: { clientX: 100, clientY: 100 } })).toEqual({ position: 'center' });
    expect(resolve({ x: 0, y: 300, width: 200, height: 200, zones: new Set<string>(), event: { clientX: lx, clientY: 300 } })).toEqual({ position: 'left', edge: true });
    // Without a pointer position there is no layout edge to dock to.
    expect(resolve({ x: 0, y: 0, width: 200, height: 200, zones: new Set<string>() })).toBeNull();
  });
});
