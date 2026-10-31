import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { WidgetChoicesStore } from './widget-choices';

function setup(mock = new MockBackend({ latencyMs: 0 })) {
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(WidgetChoicesStore) };
}

describe('WidgetChoicesStore', () => {
  it('reads the engine choices once and answers defaults, accepted metrics and titles', async () => {
    const { mock, store } = setup();
    const spy = vi.spyOn(mock, 'widgetChoices');
    expect(store.ready()).toBe(false);
    await Promise.all([store.load(), store.load()]);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(store.ready()).toBe(true);
    expect(store.dashboardDefault('heatmap').metric).toBe('cpuPerCore');
    expect(store.dashboardMetrics('heatmap').map((m) => m.title)).toEqual(['CPU per core']);
    expect(store.dashboardMetrics('ring').map((m) => m.metric)).not.toContain('fan');
    expect(store.title('networkDown')).toBe('Network down');
    expect(store.title({ process: 'chrome' })).toBe('Process chrome');
  });

  it('asks again after a failed read, and refuses a default for a kind the engine does not offer', async () => {
    const { store } = setup(new MockBackend({ latencyMs: 0, fail: new Set(['widgetChoices']) }));
    await store.load();
    expect(store.ready()).toBe(false);
    expect(() => store.dashboardDefault('graph')).toThrow(/offers no dashboard widget/);
    expect(() => store.menuBarDefault('ring')).toThrow(/offers no menu-bar item/);
    expect(store.dashboardMetrics('graph')).toEqual([]);
  });
});
