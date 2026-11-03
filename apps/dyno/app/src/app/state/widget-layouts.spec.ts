import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { WidgetLayouts } from './widget-layouts';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, widgetStorage: false });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, layouts: TestBed.inject(WidgetLayouts) };
}

describe('WidgetLayouts', () => {
  it('sends every edit with the revision of the view it was computed on', async () => {
    const { mock, layouts } = setup();
    await layouts.use('dashboard');
    const spy = vi.spyOn(mock, 'editWidgetLayout');
    const revision = layouts.dashboard()!.revision;
    expect(await layouts.edit('dashboard', { arrange: { gesture: { nudge: { id: 'health', dir: 'down' } } } })).toBeNull();
    expect(spy).toHaveBeenCalledWith({ dashboard: { arrange: { gesture: { nudge: { id: 'health', dir: 'down' } } } } }, revision);
    expect(layouts.dashboard()!.revision).toBe(revision + 1);
  });

  it('reads the view again and sends the edit once more when the layout changed meanwhile', async () => {
    const { mock, layouts } = setup();
    await layouts.use('dashboard');
    // Another window edits: the store's copy is now one revision behind.
    await mock.editWidgetLayout({ dashboard: { arrange: { gesture: { toLast: { id: 'cpu' } } } } });
    const spy = vi.spyOn(mock, 'editWidgetLayout');
    expect(await layouts.edit('dashboard', { arrange: { gesture: { nudge: { id: 'health', dir: 'down' } } } })).toBeNull();
    expect(spy).toHaveBeenCalledTimes(2);
    expect(layouts.dashboard()!.layout.placements.some((p) => p.id === 'cpu')).toBe(true);
  });

  it('gives up after one retry and says why, and reads the view again for any other refusal', async () => {
    const { mock, layouts } = setup();
    await layouts.use('dashboard');
    vi.spyOn(mock, 'editWidgetLayout').mockRejectedValue({ code: 'stale', message: 'The layout changed in another window; it was reloaded.' });
    expect(await layouts.edit('dashboard', 'undo')).toMatchObject({ code: 'stale' });
    vi.spyOn(mock, 'editWidgetLayout').mockRejectedValue({ code: 'badRequest', message: 'There is nothing to undo.' });
    const read = vi.spyOn(mock, 'widgetLayout');
    expect(await layouts.edit('dashboard', 'undo')).toMatchObject({ code: 'badRequest' });
    expect(read).toHaveBeenCalled();
  });
});

describe('WidgetLayouts, for the grid', () => {
  it('accepts a view an edit answered with and reads a surface again on request', async () => {
    const { mock, layouts } = setup();
    await layouts.use('dashboard');
    const answered = await mock.editWidgetLayout({ dashboard: { arrange: { gesture: { nudge: { id: 'health', dir: 'down' } } } } });
    layouts.accept(answered);
    expect(layouts.dashboard()).toBe('dashboard' in answered ? answered.dashboard : null);
    const again = await layouts.reload('dashboard');
    expect('dashboard' in again && again.dashboard.revision).toBe(layouts.dashboard()!.revision);
    expect(layouts.error()).toBeNull();
  });

  it('rejects when the surface cannot be read, and keeps the last view', async () => {
    const { mock, layouts } = setup();
    await layouts.use('dashboard');
    const before = layouts.dashboard();
    vi.spyOn(mock, 'widgetLayout').mockRejectedValue({ code: 'agentUnavailable', message: 'No agent' });
    await expect(layouts.reload('dashboard')).rejects.toMatchObject({ code: 'agentUnavailable' });
    expect(layouts.dashboard()).toBe(before);
  });
});
