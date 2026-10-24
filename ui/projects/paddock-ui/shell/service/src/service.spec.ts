import { Injector, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideContributions } from '@decryp7/paddock-ui/shell';
import { assertContributionsValid } from '@decryp7/paddock-ui/shell/testing';
import type { Condition, ServiceStatus, ServiceUpdate } from '../generated/service';
import { SERVICE_EVENT } from '../generated/service';
import { SERVICE_COMMANDS, type ServiceCommand } from '../generated/service-commands';
import { MemoryServiceBackend, ServiceBackend, TauriServiceBackend, memoryStatus, type MemoryServiceOptions, type ServiceIpc } from './backend';
import { conditionView } from './condition-view';
import { provideShellService } from './provide-service';
import { SERVICE_SETTINGS_COMMANDS, SERVICE_SETTINGS_PAGE, defineServiceSettingsPage, serviceContributions } from './service.contributions';
import { ServiceStore } from './service.store';
import { asServiceError, parseServiceStatus, parseServiceUpdate, serviceMessage } from './wire';

const CONDITIONS: readonly Condition[] = ['running', 'repairing', 'notInstalled', 'notRunning', 'refused', 'programMissing', 'moved', 'switchedOff', 'needsApproval', 'unavailable', 'newerOwner'];

/** Lets the simulated engine and the stores settle (microtasks only). */
async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

function text(el: Element): string {
  return (el.textContent ?? '').replace(/\s+/g, ' ').trim();
}

/** The engine's steps wait until `flush()`/`flushAll()`: a test sees each phase. */
function held(options: MemoryServiceOptions = {}) {
  const queue: (() => void)[] = [];
  const memory = new MemoryServiceBackend({ schedule: (step) => queue.push(step), ...options });
  return {
    memory,
    /** Runs the engine's next step (one phase). */
    async next(): Promise<void> {
      queue.shift()?.();
      await settle();
    },
    async all(): Promise<void> {
      while (queue.length) await this.next();
      await settle();
    },
    pending: () => queue.length,
  };
}

function storeBed(backend: ServiceBackend) {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellService({ backend: () => backend })] });
  return TestBed.inject(ServiceStore);
}

describe('the wire', () => {
  const status = memoryStatus('notRunning');

  it('accepts every status the fake engine reports and rebuilds it from known fields', () => {
    for (const c of CONDITIONS) expect(parseServiceStatus({ ...memoryStatus(c), extra: 1 })).toEqual(memoryStatus(c));
    const repair = { state: 'working', message: 'Repairing', step: 'Registering' };
    expect(parseServiceStatus({ ...status, repair })?.repair).toEqual(repair);
    expect(parseServiceStatus({ ...status, repair: { state: 'failed', message: 'x' } })?.repair).toEqual({ state: 'failed', message: 'x', step: null });
    const { message: _m, repair: _r, ...bare } = status;
    expect(parseServiceStatus(bare)).toMatchObject({ message: null, repair: null });
  });

  it('drops anything that is not a status', () => {
    const bad: unknown[] = [
      null,
      [],
      'running',
      {},
      { ...status, condition: 'exploded' },
      { ...status, mechanism: 'cron' },
      { ...status, mechanism: 1 },
      { ...status, installed: 'yes' },
      { ...status, running: 1 },
      { ...status, canRepair: null },
      { ...status, canInstall: 'x' },
      { ...status, canOpenLoginItems: undefined },
      { ...status, message: 3 },
      { ...status, repair: 'working' },
      { ...status, repair: { state: 'sleeping', message: 'x', step: null } },
      { ...status, repair: { state: 'working', message: 3, step: null } },
      { ...status, repair: { state: 'working', message: 'x', step: 4 } },
    ];
    for (const b of bad) expect(parseServiceStatus(b)).toBeNull();
  });

  it('parses updates and refusals', () => {
    expect(parseServiceUpdate({ status, error: null })).toEqual({ status, error: null });
    expect(parseServiceUpdate({ status })).toEqual({ status, error: null });
    const error = { code: 'busy', message: 'Wait.', retryable: true };
    expect(parseServiceUpdate({ status, error })).toEqual({ status, error });
    for (const bad of [null, [], {}, { status: 1 }, { status, error: 'x' }, { status, error: { code: 'nope', message: 'x', retryable: true } }]) expect(parseServiceUpdate(bad)).toBeNull();
    expect(asServiceError({ code: 'failed', message: 'x' })).toEqual({ code: 'failed', message: 'x', retryable: false });
    expect(asServiceError({ code: 'failed' })).toBeNull();
    expect(asServiceError({ code: 7, message: 'x' })).toBeNull();
    expect(asServiceError(new Error('x'))).toBeNull();
    expect(serviceMessage(error, 'f')).toBe('Wait.');
    expect(serviceMessage(new Error('boom'), 'f')).toBe('boom');
    expect(serviceMessage('plain', 'f')).toBe('plain');
    expect(serviceMessage('', 'f')).toBe('f');
    expect(serviceMessage(undefined, 'f')).toBe('f');
  });
});

describe('the conditions', () => {
  it('give every condition a word and a role, and no two the same word', () => {
    const views = CONDITIONS.map((c) => conditionView(c));
    expect(new Set(views.map((v) => v.label)).size).toBe(CONDITIONS.length);
    expect(views.every((v) => v.label.length > 0)).toBe(true);
    expect(conditionView('running').status).toBe('success');
    expect(conditionView('refused').status).toBe('danger');
  });

  it('offer buttons that fit the condition in the fake engine', () => {
    expect(memoryStatus('notInstalled')).toMatchObject({ canInstall: true, canRepair: false });
    expect(memoryStatus('notRunning')).toMatchObject({ canInstall: false, canRepair: true });
    expect(memoryStatus('switchedOff')).toMatchObject({ canOpenLoginItems: true, canRepair: false });
    expect(memoryStatus('running')).toMatchObject({ canRepair: false, running: true });
    expect(memoryStatus('unavailable')).toMatchObject({ canRepair: false, canInstall: false, canOpenLoginItems: false });
  });
});

describe('the Tauri backend', () => {
  function ipc(answers: Partial<Record<ServiceCommand, unknown>>) {
    const calls: ServiceCommand[] = [];
    let handler: ((p: unknown) => void) | undefined;
    const port: ServiceIpc = {
      invoke: <T>(command: ServiceCommand) => {
        calls.push(command);
        const a = answers[command];
        return a instanceof Error || (typeof a === 'object' && a !== null && 'code' in a && command === 'service_repair') ? Promise.reject(a) : Promise.resolve(a as T);
      },
      listen: (event, h) => {
        expect(event).toBe(SERVICE_EVENT);
        handler = h;
        return Promise.resolve(() => (handler = undefined));
      },
    };
    return { port, calls, emit: (p: unknown) => handler?.(p), listening: () => handler !== undefined };
  }

  it('calls the four commands of the table and nothing else', async () => {
    const status = memoryStatus('running');
    const t = ipc({ service_status: status, service_repair: null, service_install: true, open_login_items: true });
    const backend = new TauriServiceBackend(t.port);
    expect(await backend.status()).toEqual(status);
    await backend.repair();
    await backend.install();
    expect(await backend.openLoginItems()).toBe(true);
    expect(t.calls).toEqual(['service_status', 'service_repair', 'service_install', 'open_login_items']);
    for (const c of t.calls) expect(SERVICE_COMMANDS[c]).toBeDefined();
  });

  it('refuses a status that is not one, and reports a missing Login Items page', async () => {
    const t = ipc({ service_status: { nonsense: true }, open_login_items: false });
    const backend = new TauriServiceBackend(t.port);
    await expect(backend.status()).rejects.toThrow('no valid service status');
    expect(await backend.openLoginItems()).toBe(false);
  });

  it('lets the engine’s refusal through and passes parsed updates only', async () => {
    const refusal = { code: 'busy', message: 'Running.', retryable: true };
    const t = ipc({ service_repair: refusal });
    const backend = new TauriServiceBackend(t.port);
    await expect(backend.repair()).rejects.toBe(refusal);
    const seen: ServiceUpdate[] = [];
    const off = await backend.listen((u) => seen.push(u));
    t.emit({ nonsense: 1 });
    t.emit({ status: memoryStatus('running'), error: null });
    expect(seen).toHaveLength(1);
    off();
    expect(t.listening()).toBe(false);
  });
});

describe('the memory backend', () => {
  it('walks the phases of a repair and ends running', async () => {
    const h = held({ condition: 'notRunning', steps: ['one', 'two'] });
    const seen: ServiceUpdate[] = [];
    await h.memory.listen((u) => seen.push(u));
    await h.memory.repair();
    await h.all();
    expect(seen.map((u) => u.status.repair?.step ?? u.status.repair?.state)).toEqual(['one', 'two', 'succeeded']);
    expect((await h.memory.status()).condition).toBe('running');
  });

  it('refuses a second request while one runs, and what does not apply', async () => {
    const h = held({ condition: 'notRunning', outcome: 'hang' });
    await h.memory.repair();
    await expect(h.memory.repair()).rejects.toMatchObject({ code: 'busy' });
    await expect(h.memory.install()).rejects.toMatchObject({ code: 'busy' });
    const r = held({ condition: 'running' });
    await expect(r.memory.repair()).rejects.toMatchObject({ code: 'unavailable' });
  });

  it('plays failure, switched-off and needs-approval outcomes with an error', async () => {
    for (const [outcome, code, condition] of [['fail', 'failed', 'notRunning'], ['switchedOff', 'switchedOff', 'switchedOff'], ['needsApproval', 'needsApproval', 'needsApproval']] as const) {
      const h = held({ outcome, steps: ['x'] });
      const seen: ServiceUpdate[] = [];
      await h.memory.listen((u) => seen.push(u));
      await h.memory.repair();
      await h.all();
      const last = seen.at(-1)!;
      expect(last.error?.code).toBe(code);
      expect(last.status.condition).toBe(condition);
      expect(last.status.repair?.state).toBe('failed');
    }
    const h = held({ condition: 'notInstalled', outcome: 'fail', steps: ['x'] });
    const seen: ServiceUpdate[] = [];
    await h.memory.listen((u) => seen.push(u));
    await h.memory.install();
    await h.all();
    expect(seen.at(-1)!.status.condition).toBe('notInstalled');
  });

  it('counts the Login Items opened, answers false where there is none, fails the first asks on request, and stops listening', async () => {
    const m = new MemoryServiceBackend({ statusFailures: 1 });
    await expect(m.status()).rejects.toThrow();
    expect((await m.status()).condition).toBe('notRunning');
    expect(await m.openLoginItems()).toBe(true);
    expect(m.loginItemsOpened).toBe(1);
    expect(await new MemoryServiceBackend({ loginItems: false }).openLoginItems()).toBe(false);
    const seen: ServiceUpdate[] = [];
    const off = await m.listen((u) => seen.push(u));
    off();
    await m.repair();
    await settle();
    expect(seen).toHaveLength(0);
  });

  it('runs steps on the next microtask by default and with no steps shows no phase', async () => {
    const m = new MemoryServiceBackend({ steps: [] });
    const seen: ServiceUpdate[] = [];
    await m.listen((u) => seen.push(u));
    await m.repair();
    await settle();
    expect(seen.at(-1)!.status.condition).toBe('running');
  });
});

describe('the store', () => {
  it('loads the status of every condition', async () => {
    for (const c of CONDITIONS) {
      TestBed.resetTestingModule();
      const store = storeBed(new MemoryServiceBackend({ condition: c }));
      await settle();
      expect(store.isLoaded()).toBe(true);
      expect(store.status()?.condition).toBe(c);
      const s = memoryStatus(c);
      expect([store.canRepair(), store.canInstall(), store.canOpenLoginItems()]).toEqual([s.canRepair, s.canInstall, s.canOpenLoginItems]);
    }
  });

  it('shows loading first, then an error with Retry that loads again', async () => {
    const memory = new MemoryServiceBackend({ statusFailures: 1 });
    const store = storeBed(memory);
    expect(store.isLoading()).toBe(true);
    await settle();
    expect(store.requestError()).toBe('The service could not be asked. Try again.');
    expect(store.status()).toBeNull();
    await store.load();
    expect(store.isLoaded()).toBe(true);
    expect(store.status()?.condition).toBe('notRunning');
  });

  it('keeps the status on screen while it refreshes', async () => {
    const store = storeBed(new MemoryServiceBackend());
    await settle();
    const p = store.load();
    expect(store.isLoaded()).toBe(true);
    await p;
  });

  it('folds a repair’s phases from the event and is never timed out while it works', async () => {
    const h = held({ steps: ['one', 'two'] });
    const store = storeBed(h.memory);
    await settle();
    const done = store.repair();
    await done;
    // Asked, nothing heard yet: working, no phase.
    expect(store.working()).toBe(true);
    expect(store.step()).toBeNull();
    await h.next();
    expect(store.step()).toBe('one');
    expect(store.status()?.condition).toBe('repairing');
    await h.next();
    expect(store.step()).toBe('two');
    expect(store.working()).toBe(true);
    // A second Repair while it runs is ignored here.
    await store.repair();
    await h.all();
    expect(store.working()).toBe(false);
    expect(store.step()).toBeNull();
    expect(store.notice()).toMatchObject({ state: 'succeeded' });
    expect(store.status()?.condition).toBe('running');
    expect(store.failure()).toBeNull();
  });

  it('installs, and shows a failure with the request it belongs to; Retry runs it again', async () => {
    const h = held({ condition: 'notInstalled', outcome: 'fail', steps: ['x'] });
    const store = storeBed(h.memory);
    await settle();
    await store.install();
    await h.all();
    expect(store.failure()).toMatchObject({ action: 'install', error: { code: 'failed', retryable: true } });
    expect(store.retryable()).toBe('install');
    expect(store.working()).toBe(false);
    await store.retry();
    expect(store.failure()).toBeNull();
    expect(store.working()).toBe(true);
    await h.all();
    expect(store.failure()).toMatchObject({ action: 'install' });
    store.dismissFailure();
    expect(store.failure()).toBeNull();
    expect(store.retryable()).toBeNull();
    await store.retry();
    expect(store.working()).toBe(false);
  });

  it('retries a repair, and offers no Retry for a refusal that cannot help', async () => {
    const h = held({ outcome: 'fail', steps: ['x'] });
    const store = storeBed(h.memory);
    await settle();
    await store.repair();
    await h.all();
    expect(store.retryable()).toBe('repair');
    await store.retry();
    await h.all();
    expect(store.failure()?.action).toBe('repair');

    TestBed.resetTestingModule();
    const s = held({ outcome: 'switchedOff', steps: ['x'] });
    const store2 = storeBed(s.memory);
    await settle();
    await store2.repair();
    await s.all();
    expect(store2.failure()?.error.code).toBe('switchedOff');
    expect(store2.retryable()).toBeNull();
    expect(store2.canOpenLoginItems()).toBe(true);
  });

  it('says busy when the engine refuses and shows where the other repair is', async () => {
    const h = held({ outcome: 'hang', steps: ['one'] });
    const store = storeBed(h.memory);
    await settle();
    // Another window started a repair: the engine is busy but this store has not heard.
    await h.memory.repair();
    await store.repair();
    expect(store.failure()?.error.code).toBe('busy');
    expect(store.working()).toBe(false);
    await h.next();
    expect(store.step()).toBe('one');
    expect(store.working()).toBe(true);
  });

  it('shows a repair another window started and its outcome', async () => {
    const h = held({ steps: ['one'] });
    const store = storeBed(h.memory);
    await settle();
    await h.memory.repair();
    await h.next();
    expect(store.working()).toBe(true);
    expect(store.step()).toBe('one');
    await h.all();
    expect(store.working()).toBe(false);
  });

  it('wraps a rejection that is not the engine’s refusal', async () => {
    const backend = new (class extends MemoryServiceBackend {
      override repair(): Promise<void> {
        return Promise.reject(new Error('the pipe is closed'));
      }
    })();
    const store = storeBed(backend);
    await settle();
    await store.repair();
    expect(store.failure()).toMatchObject({ action: 'repair', error: { code: 'failed', message: 'the pipe is closed', retryable: true } });
    expect(store.working()).toBe(false);
  });

  it('opens Login Items, and says so when there is no page or the call fails', async () => {
    const memory = new MemoryServiceBackend({ condition: 'switchedOff' });
    const store = storeBed(memory);
    await settle();
    await store.openLoginItems();
    expect(memory.loginItemsOpened).toBe(1);
    expect(store.failure()).toBeNull();

    TestBed.resetTestingModule();
    const none = storeBed(new MemoryServiceBackend({ condition: 'switchedOff', loginItems: false }));
    await settle();
    await none.openLoginItems();
    expect(none.failure()).toMatchObject({ action: 'openLoginItems', error: { code: 'unavailable', retryable: false } });
    expect(none.retryable()).toBeNull();

    TestBed.resetTestingModule();
    const broken = storeBed(
      new (class extends MemoryServiceBackend {
        override openLoginItems(): Promise<boolean> {
          return Promise.reject({ code: 'unavailable', message: 'No such page.', retryable: true });
        }
      })({ condition: 'switchedOff' }),
    );
    await settle();
    await broken.openLoginItems();
    expect(broken.failure()?.error.message).toBe('No such page.');
    expect(broken.retryable()).toBeNull();

    TestBed.resetTestingModule();
    const odd = storeBed(
      new (class extends MemoryServiceBackend {
        override openLoginItems(): Promise<boolean> {
          return Promise.reject(new Error(''));
        }
      })(),
    );
    await settle();
    await odd.openLoginItems();
    expect(odd.failure()?.error.message).toContain('could not be opened');
  });

  it('stops listening when destroyed, also when the listen was still pending, and warns when listening fails', async () => {
    let off = 0;
    class Listening extends MemoryServiceBackend {
      override listen(): Promise<() => void> {
        return Promise.resolve(() => void off++);
      }
    }
    const store = storeBed(new Listening());
    expect(store).toBeTruthy();
    await settle();
    TestBed.resetTestingModule();
    expect(off).toBe(1);

    const quick = storeBed(new Listening());
    expect(quick).toBeTruthy();
    TestBed.resetTestingModule();
    await settle();
    expect(off).toBe(2);

    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    class Deaf extends MemoryServiceBackend {
      override listen(): Promise<() => void> {
        return Promise.reject(new Error('no events'));
      }
    }
    const deaf = storeBed(new Deaf());
    await settle();
    expect(deaf.isLoaded()).toBe(true);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    warn.mockRestore();
  });

  it('folds an update that arrives before any status', async () => {
    let push: ((u: ServiceUpdate) => void) | undefined;
    class Pushing extends MemoryServiceBackend {
      override listen(h: (u: ServiceUpdate) => void): Promise<() => void> {
        push = h;
        return Promise.resolve(() => undefined);
      }
    }
    const store = storeBed(new Pushing());
    const status: ServiceStatus = memoryStatus('notInstalled');
    push?.({ status, error: { code: 'failed', message: 'x', retryable: true } });
    expect(store.status()).toBe(status);
    expect(store.failure()?.action).toBe('install');
    await settle();
  });
});

describe('the contributions', () => {
  it('declare the page of the login section and the four commands of the table', () => {
    const page = defineServiceSettingsPage({ providers: () => Promise.resolve([]), load: () => Promise.resolve(class {}) });
    expect(page.id).toBe('pd.login');
    expect(SERVICE_SETTINGS_PAGE).toBe(page.id);
    expect(page.group).toBe('application');
    expect([...SERVICE_SETTINGS_COMMANDS].sort()).toEqual(Object.keys(SERVICE_COMMANDS).sort());
    const batch = serviceContributions(page);
    expect(batch.settingsPages).toEqual([page]);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions(batch)] });
    assertContributionsValid(TestBed.inject(Injector));
  });
});

