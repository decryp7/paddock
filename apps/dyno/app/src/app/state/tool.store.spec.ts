import { TestBed } from '@angular/core/testing';
import { jobsProviders, mockWithJobs } from '../../testing/job-test-support';
import { BACKEND } from '../core/backend';
import { ToolStore } from './tool.store';

function setup() {
  const { mock, jobs } = mockWithJobs({ os: 'windows', helper: 'ready' });
  TestBed.configureTestingModule({ providers: [jobsProviders(jobs), { provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(ToolStore) };
}

afterEach(() => TestBed.resetTestingModule());

describe('ToolStore', () => {
  it('says so when the install job is done but the reloaded offer does not show the tool (never a silent "Not installed")', async () => {
    const { mock, store } = setup();
    await store.load();
    const available = await mock.toolOffer();
    vi.spyOn(mock, 'toolOffer').mockResolvedValue(available);
    await store.install();
    expect(store.running()).toBe(false);
    expect(store.actionError()).toContain('cannot see smartmontools');
    expect(store.actionError()).toContain('Check again');
  });

  it('has no message when the install is done and the tool shows', async () => {
    const { store } = setup();
    await store.load();
    await store.install();
    expect(store.actionError()).toBeNull();
    expect(store.offer()?.state).toEqual({ installed: '7.5' });
    expect(store.offer()?.source).toBe('helper');
  });
});
