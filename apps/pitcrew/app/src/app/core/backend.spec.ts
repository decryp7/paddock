import { isTauri } from './backend';
import { toCommandError } from './command-error';
import { DEMO_RUNNERS, MockBackend } from './mock-backend';
import { TauriBackend } from './tauri-backend';

describe('backends', () => {
  it('detects the browser build', () => {
    expect(isTauri()).toBe(false);
  });

  it('the mock returns copies of its demo data', async () => {
    const mock = new MockBackend();
    const a = await mock.status();
    a[0]!.name = 'changed';
    expect((await mock.status())[0]!.name).toBe(DEMO_RUNNERS[0]!.name);
    expect((await mock.appUpdateStatus()).available).toBe(false);
  });

  it('the Tauri backend calls the commands and types their errors', async () => {
    const calls: string[] = [];
    const ok = new TauriBackend(async <T>(cmd: string) => {
      calls.push(cmd);
      return [] as T;
    });
    expect(await ok.status()).toEqual([]);
    await ok.appUpdateStatus();
    expect(calls).toEqual(['runner_status', 'app_update_status']);

    const failing = new TauriBackend(() => Promise.reject({ code: 'notImplemented', message: 'not yet' }));
    await expect(failing.status()).rejects.toEqual({ code: 'notImplemented', message: 'not yet' });
  });

  it('turns any rejection into a typed error', () => {
    expect(toCommandError({ code: 'busy', message: 'later' })).toEqual({ code: 'busy', message: 'later' });
    expect(toCommandError({ code: 'weird', message: 'x' })).toEqual({ code: 'internal', message: 'Something went wrong.' });
    expect(toCommandError(new Error('boom'))).toEqual({ code: 'internal', message: 'boom' });
    expect(toCommandError('plain')).toEqual({ code: 'internal', message: 'plain' });
    expect(toCommandError(null)).toEqual({ code: 'internal', message: 'Something went wrong.' });
  });
});
