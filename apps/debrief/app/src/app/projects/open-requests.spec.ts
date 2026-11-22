import { TestBed } from '@angular/core/testing';
import { Logger } from '../core/logger';
import { WorkspaceStore } from '../core/workspace.store';
import { MemoryLogger } from '../../testing/helpers';
import { ProjectBackend } from '../core/tools-backend';
import { ProjectService } from './project.service';
import { OpenRequests } from './open-requests';

// The real clock back after every test, so a test that times out cannot leave fake timers for the next one (ER-12, C154).
afterEach(() => vi.useRealTimers());

function setup(summary: unknown, unlisten: () => void = () => undefined) {
  const subscribe = vi.fn(async () => unlisten);
  TestBed.configureTestingModule({
    providers: [
      { provide: ProjectBackend, useValue: { onOpenRequested: subscribe, takePendingOpen: async () => [] } },
      { provide: ProjectService, useValue: {} },
      { provide: WorkspaceStore, useValue: { summary: () => summary } },
      { provide: Logger, useFactory: () => new MemoryLogger() },
    ],
  });
  return { requests: TestBed.inject(OpenRequests), subscribe, logger: TestBed.inject(Logger) };
}

describe('OpenRequests.start', () => {
  it('subscribes once when two callers start it together', async () => {
    const { requests, subscribe } = setup({});
    await Promise.all([requests.start(), requests.start()]);
    expect(subscribe).toHaveBeenCalledTimes(1);
  });

  it('ends its wait for the workspace with its injector: no timer is left', async () => {
    vi.useFakeTimers();
    try {
      const { requests } = setup(null);
      const started = requests.start();
      void requests.start();
      await vi.advanceTimersByTimeAsync(0);
      expect(vi.getTimerCount()).toBe(1);
      TestBed.resetTestingModule();
      expect(vi.getTimerCount()).toBe(0);
      await started;
    } finally {
      vi.useRealTimers();
    }
  });

  it('unsubscribes when its injector is destroyed', async () => {
    const unlisten = vi.fn();
    const { requests } = setup({}, unlisten);
    await requests.start();
    TestBed.resetTestingModule();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('subscribes to nothing when the injector goes while it waits for the workspace', async () => {
    vi.useFakeTimers();
    try {
      const { requests, subscribe } = setup(null);
      const started = requests.start();
      await vi.advanceTimersByTimeAsync(0);
      TestBed.resetTestingModule();
      await started;
      expect(subscribe).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('a failed subscription clears the shared start, so a later start() tries again', async () => {
    const { requests, subscribe } = setup({});
    subscribe.mockRejectedValueOnce(new Error('no shell'));
    await requests.start();
    await requests.start();
    expect(subscribe).toHaveBeenCalledTimes(2);
  });
});
