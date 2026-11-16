import { Injectable } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_IDLE } from '@decryp7/paddock-ui/shell';
import { FakeCommandContext } from './contributions';
import { provideFakeDock, provideTinyBackendBare } from './helpers';
import { COMMAND_CONTEXT, CommandRegistry } from '../app/extensions/command-registry';
import { lazyCommandFactory, lazyCommands } from '../app/extensions/lazy-registration';
import type { DebriefCommand } from '../app/extensions/tokens';
import { settleStartup } from './startup';

@Injectable({ providedIn: 'root' })
class SlowCommands {
  readonly commands: DebriefCommand[] = [{ id: 'slow.run', title: 'Slow', run: () => undefined }];
}

describe('settleStartup', () => {
  it('waits for a lazy command set whose chunk is still loading, however long that takes', async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => (release = resolve));
    let chunkLoaded = false;
    TestBed.configureTestingModule({
      providers: [
        provideTinyBackendBare(),
        provideFakeDock(),
        { provide: COMMAND_CONTEXT, useValue: new FakeCommandContext() },
        { provide: SHELL_IDLE, useValue: () => undefined },
        ...lazyCommands(async () => {
          await gate;
          chunkLoaded = true;
          return SlowCommands;
        }, [{ id: 'slow.run', title: 'Slow' }]),
      ],
    });
    let settled = false;
    const done = settleStartup().then(() => (settled = true));
    // Let every already-queued task and microtask run: a clock-based settle would have returned by now.
    await new Promise<void>((resolve) => {
      const channel = new MessageChannel();
      channel.port1.onmessage = () => {
        channel.port1.close();
        resolve();
      };
      channel.port2.postMessage(null);
    });
    expect(settled).toBe(false);
    release();
    await done;
    expect(chunkLoaded).toBe(true);
  });
});

/**
 * The shared `afterEach` of test-setup.ts: a test that only boots the app and ends (no settle of its own) must still
 * have its lazy chunks loaded, into a live injector, before TestBed tears the module down. The chunk here finishes
 * loading after the first test body has returned; the second test reads what happened.
 */
describe('the shared afterEach for booted start-up work', () => {
  const seen = { factoryRan: false };

  it('boots a lazy set whose chunk finishes after the test body, and does nothing else', () => {
    seen.factoryRan = false;
    TestBed.configureTestingModule({
      providers: [
        provideTinyBackendBare(),
        provideFakeDock(),
        { provide: COMMAND_CONTEXT, useValue: new FakeCommandContext() },
        { provide: SHELL_IDLE, useValue: (work: () => void) => work() },
        ...lazyCommandFactory(
          () =>
            new Promise((resolve) =>
              // The chunk arrives on a later task than the test body's end: what a slow disk or a loaded machine does.
              setTimeout(
                () =>
                  resolve(() => {
                    seen.factoryRan = true;
                    return [{ id: 'slow.run', title: 'Slow', run: () => undefined }];
                  }),
                0,
              ),
            ),
          [{ id: 'slow.run', title: 'Slow' }],
        ),
      ],
    });
    TestBed.inject(CommandRegistry).all();
  });

  it('had the chunk loaded into the live injector before the module was torn down', () => {
    expect(seen.factoryRan).toBe(true);
  });
});
