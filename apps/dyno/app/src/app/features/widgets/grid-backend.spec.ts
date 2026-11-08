import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { WidgetLayouts } from '../../state/widget-layouts';
import { DynoGridBackend } from './grid-backend';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, widgetStorage: false });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, DynoGridBackend] });
  return { mock, layouts: TestBed.inject(WidgetLayouts), grid: TestBed.inject(DynoGridBackend) };
}

describe('DynoGridBackend', () => {
  it('asks the engine for a preview and stores nothing', async () => {
    const { mock, layouts, grid } = setup();
    await layouts.use('dashboard');
    const view = layouts.dashboard()!;
    const spy = vi.spyOn(mock, 'previewWidgetLayout');
    const request = { surface: 'dashboard', revision: view.revision, gesture: { nudge: { id: 'health', dir: 'down' as const } }, seq: 7 };
    const reply = await grid.preview(request);
    expect(spy).toHaveBeenCalledWith(request);
    expect(reply.seq).toBe(7);
    expect(layouts.dashboard()!.revision).toBe(view.revision);
  });

  it('sends an edit on the revision it was computed on and hands the answer to the store', async () => {
    const { mock, layouts, grid } = setup();
    await layouts.use('dashboard');
    const revision = layouts.dashboard()!.revision;
    const spy = vi.spyOn(mock, 'editWidgetLayout');
    const view = await grid.edit({ arrange: { gesture: { nudge: { id: 'health', dir: 'down' } } } }, revision);
    expect(spy).toHaveBeenCalledWith({ dashboard: { arrange: { gesture: { nudge: { id: 'health', dir: 'down' } } } } }, revision);
    expect(view.revision).toBe(revision + 1);
    expect(layouts.dashboard()).toEqual(view);
  });

  it('rejects with the engine\'s error (the grid reloads and retries a stale one itself)', async () => {
    const { mock, layouts, grid } = setup();
    await layouts.use('dashboard');
    vi.spyOn(mock, 'editWidgetLayout').mockRejectedValue({ code: 'stale', message: 'The layout changed in another window; it was reloaded.' });
    await expect(grid.edit('undo', 1)).rejects.toMatchObject({ code: 'stale' });
  });

  it('reloads the dashboard and keeps the store current', async () => {
    const { mock, layouts, grid } = setup();
    await layouts.use('dashboard');
    await mock.editWidgetLayout({ dashboard: { arrange: { gesture: { toLast: { id: 'cpu' } } } } });
    const view = await grid.reload();
    expect(layouts.dashboard()).toEqual(view);
    expect(view.revision).toBeGreaterThan(1);
  });

  it('refuses another surface\'s view', async () => {
    const { layouts, grid } = setup();
    vi.spyOn(layouts, 'reload').mockResolvedValue({ popover: {} as never });
    await expect(grid.reload()).rejects.toThrow(/another surface/);
  });
});
