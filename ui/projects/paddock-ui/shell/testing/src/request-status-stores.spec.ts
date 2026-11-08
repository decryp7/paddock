import { provideZonelessChangeDetection, type ProviderToken, type Signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { MemoryUpdatesBackend, UpdatesBackend, UpdatesStore } from '@decryp7/paddock-ui/shell/about';
import { ChannelsBackend, ChannelsStore, MemoryChannelsBackend } from '@decryp7/paddock-ui/shell/channels';
import { DiagnosticsBackend, DiagnosticsStore, MemoryDiagnosticsBackend } from '@decryp7/paddock-ui/shell/diagnostics';
import { FilesBackend, FilesStore, MemoryFilesBackend } from '@decryp7/paddock-ui/shell/files';
import { JOBS_CLOCK, JobsBackend, JobsStore, MemoryJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { McpBackend, McpStore, MemoryMcpBackend } from '@decryp7/paddock-ui/shell/mcp';
import {
  MemoryNotificationCentre,
  MemoryServiceHealth,
  NotificationsBackend,
  NotificationsStore,
  ServiceHealthBackend,
  ServiceHealthStore,
} from '@decryp7/paddock-ui/shell/notify';
import { MemoryPacksBackend, PacksBackend, PacksStore } from '@decryp7/paddock-ui/shell/packs';
import { MemoryPitwallBackend, PitwallBackend, PitwallStore } from '@decryp7/paddock-ui/shell/pitwall';
import { MemoryServiceBackend, ServiceBackend, ServiceStore } from '@decryp7/paddock-ui/shell/service';
import { MemorySettingsBackend, SettingsBackend, SettingsStore } from '@decryp7/paddock-ui/shell/settings';
import { MemorySetupBackend, SetupBackend, SetupStore } from '@decryp7/paddock-ui/shell/setup';
import { MemoryStorageBackend, StorageBackend, StorageStore } from '@decryp7/paddock-ui/shell/storage';
import { READ_RETRY_CLOCK, type RequestStatus } from '@decryp7/paddock-ui/shell/store';
import { ActivityBackend, ActivityStore, MemoryActivityBackend } from '@decryp7/paddock-ui/shell/undo';
import { FakeClock, RecordingShellPorts } from './shell-testing';

// ADR 0058: every store that uses `withRequestStatus` declares what happens after a failed read, and this spec holds each
// declaration to the store's behaviour. A store that reads engine state (`retry: true`) leaves an error on its own: its
// self-retry reads again after the first backoff delay, and an event that arrives while in error ends the error (an event
// with the whole state) or starts a re-read (an event with a part of it). The source scan below makes a store added later
// without a row here fail, so none can miss the check.

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  readdirSync(path: string): string[];
  statSync(path: string): { isDirectory(): boolean };
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;

const ROOT = 'projects/paddock-ui';

/** Every non-spec TypeScript file under `dir`. */
function sources(fs: NodeFs, dir: string): string[] {
  return fs.readdirSync(dir).flatMap((name) => {
    const path = `${dir}/${name}`;
    if (fs.statSync(path).isDirectory()) return name === 'node_modules' ? [] : sources(fs, path);
    return path.endsWith('.ts') && !path.endsWith('.spec.ts') ? [path] : [];
  });
}

/** The part of a store this spec reads. */
interface StatusOf {
  readonly requestStatus: Signal<RequestStatus>;
}

/** A backend whose methods can be replaced by name. */
type AnyBackend = Record<string, (...args: unknown[]) => unknown>;

/** Calls the backend's method `name` with no arguments. */
const call = (b: AnyBackend, name: string): Promise<unknown> => (b[name] as () => Promise<unknown>).call(b);

/** How an event of the store reaches it, and what it carries. */
interface EventCase {
  /** The backend method the store listens with. */
  readonly listen: string;
  /** `snapshot`: the event carries the whole state (ends the error at once); `change`: a part (starts a re-read). */
  readonly carries: 'snapshot' | 'change';
  /** The event's payload; built before the read is made to fail, so a snapshot comes from the backend itself. */
  readonly payload: (backend: AnyBackend) => unknown;
}

/** A store that reads engine state (`retry: true`). */
interface RetryCase {
  readonly retry: true;
  readonly store: ProviderToken<StatusOf>;
  readonly port: ProviderToken<unknown>;
  readonly backend: () => object;
  /** The backend method of the store's read. */
  readonly read: string;
  readonly event?: EventCase;
}

/** A store whose status is not a read of engine state (`retry: false`): the reason is in its source. */
interface PlainCase {
  readonly retry: false;
}

const cases: Readonly<Record<string, RetryCase | PlainCase>> = {
  'shell/jobs/src/jobs.store.ts': {
    retry: true,
    store: JobsStore,
    port: JobsBackend,
    backend: () => new MemoryJobsBackend(),
    read: 'snapshot',
    event: {
      listen: 'listen',
      carries: 'change',
      payload: () => ({ id: 1, kind: 'scan', title: 'Job 1', subject: null, seq: 1, elapsedMs: 0, etaMs: null, cancellable: true, notCancellableReason: null, state: 'running', phase: null }),
    },
  },
  'shell/undo/src/activity.store.ts': {
    retry: true,
    store: ActivityStore,
    port: ActivityBackend,
    backend: () => new MemoryActivityBackend(),
    read: 'list',
    event: { listen: 'listen', carries: 'change', payload: () => ({ id: 1, title: 'Renamed the file', undo: 'none' }) },
  },
  'shell/settings/src/settings.store.ts': {
    retry: true,
    store: SettingsStore,
    port: SettingsBackend,
    backend: () => new MemorySettingsBackend(),
    read: 'overview',
    event: { listen: 'listen', carries: 'change', payload: () => ({ section: 'general', values: {}, restartPending: false }) },
  },
  'shell/notify/src/service-health.store.ts': {
    retry: true,
    store: ServiceHealthStore,
    port: ServiceHealthBackend,
    backend: () => new MemoryServiceHealth([{ id: 'engine', label: 'The engine' }]),
    read: 'snapshot',
    event: { listen: 'listen', carries: 'snapshot', payload: (b) => call(b, 'snapshot') },
  },
  'shell/notify/src/notifications.store.ts': {
    retry: true,
    store: NotificationsStore,
    port: NotificationsBackend,
    backend: () => new MemoryNotificationCentre(() => 1_000),
    read: 'list',
    event: { listen: 'listen', carries: 'snapshot', payload: async (b) => ({ ...((await call(b, 'list')) as object), posted: null }) },
  },
  'shell/channels/src/channels.store.ts': {
    retry: true,
    store: ChannelsStore,
    port: ChannelsBackend,
    backend: () => new MemoryChannelsBackend(),
    read: 'get',
    event: { listen: 'listen', carries: 'change', payload: () => ({ kind: 'testProgress', channelId: 'none', testId: 1, attempt: 1, of: 1, phase: 'sending', waitSecs: null }) },
  },
  'shell/mcp/src/mcp.store.ts': {
    retry: true,
    store: McpStore,
    port: McpBackend,
    backend: () => new MemoryMcpBackend(),
    read: 'get',
    event: { listen: 'listen', carries: 'change', payload: () => ({ kind: 'auditChanged' }) },
  },
  'shell/diagnostics/src/diagnostics.store.ts': {
    retry: true,
    store: DiagnosticsStore,
    port: DiagnosticsBackend,
    backend: () => new MemoryDiagnosticsBackend({ files: [] }),
    read: 'preview',
    event: { listen: 'listen', carries: 'change', payload: () => ({ state: 'cancelled' }) },
  },
  'shell/about/src/updates.store.ts': {
    retry: true,
    store: UpdatesStore,
    port: UpdatesBackend,
    backend: () => new MemoryUpdatesBackend({ currentVersion: '1.0.0', server: 'none' }),
    read: 'status',
    event: { listen: 'listen', carries: 'snapshot', payload: (b) => call(b, 'status') },
  },
  'shell/setup/src/setup.store.ts': {
    retry: true,
    store: SetupStore,
    port: SetupBackend,
    backend: () => new MemorySetupBackend(),
    read: 'state',
    event: { listen: 'listen', carries: 'snapshot', payload: (b) => call(b, 'state') },
  },
  'shell/storage/src/storage.store.ts': {
    retry: true,
    store: StorageStore,
    port: StorageBackend,
    backend: () => new MemoryStorageBackend({ measured: true }),
    read: 'last',
    event: { listen: 'listen', carries: 'snapshot', payload: async (b) => ({ state: 'usage', usage: await call(b, 'last') }) },
  },
  'shell/packs/src/packs.store.ts': {
    retry: true,
    store: PacksStore,
    port: PacksBackend,
    backend: () => new MemoryPacksBackend(),
    read: 'status',
    event: { listen: 'listen', carries: 'snapshot', payload: (b) => call(b, 'status') },
  },
  'shell/pitwall/src/pitwall.store.ts': {
    retry: true,
    store: PitwallStore,
    port: PitwallBackend,
    backend: () => new MemoryPitwallBackend(),
    read: 'snapshot',
    event: { listen: 'listen', carries: 'snapshot', payload: (b) => call(b, 'snapshot') },
  },
  'shell/files/src/files.store.ts': {
    retry: true,
    store: FilesStore,
    port: FilesBackend,
    backend: () => new MemoryFilesBackend(),
    read: 'recent',
    event: { listen: 'listenRecent', carries: 'snapshot', payload: (b) => call(b, 'recent') },
  },
  'shell/service/src/service.store.ts': {
    retry: true,
    store: ServiceStore,
    port: ServiceBackend,
    backend: () => new MemoryServiceBackend(),
    read: 'status',
    event: { listen: 'listen', carries: 'snapshot', payload: async (b) => ({ status: await call(b, 'status'), error: null }) },
  },
  'shell/startup/src/recovery.store.ts': { retry: false },
  'workbench/assistant/src/assistant.store.ts': { retry: false },
  'workbench/palette/src/palette.store.ts': { retry: false },
  'live/src/live-feed.store.ts': { retry: false },
  'view/src/view.store.ts': { retry: false },
};

/** The store of `c` over its memory backend, whose read fails once; the clock, the ports and the event handler. */
async function failedFirst(c: RetryCase) {
  const backend = c.backend() as AnyBackend;
  const payload = c.event ? await c.event.payload(backend) : undefined;
  let handler: ((payload: unknown) => void) | undefined;
  if (c.event) {
    vi.spyOn(backend, c.event.listen).mockImplementation((h: unknown) => {
      handler = h as (payload: unknown) => void;
      return Promise.resolve(() => undefined);
    });
  }
  const read = vi.spyOn(backend, c.read).mockRejectedValueOnce(new Error('The engine is restarting.'));
  const clock = new FakeClock(1_000);
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      c.store,
      { provide: c.port, useValue: backend },
      { provide: READ_RETRY_CLOCK, useValue: clock },
      { provide: JOBS_CLOCK, useValue: clock },
      { provide: SHELL_PORTS, useValue: new RecordingShellPorts() },
    ],
  });
  const store = TestBed.inject(c.store);
  await vi.waitFor(() => expect(store.requestStatus()).toEqual({ error: 'The engine is restarting.' }));
  if (c.event) await vi.waitFor(() => expect(handler).toBeDefined());
  return { store, clock, read, emit: () => handler?.(payload) };
}

describe('every store with a request status (ADR 0058)', () => {
  it('has a row here, and declares the policy its row says', async () => {
    const fs = await nodeFs();
    const users = sources(fs, ROOT)
      .filter((f) => !f.endsWith('/shell/store/src/request-status.ts') && fs.readFileSync(f, 'utf8').includes('withRequestStatus('))
      .map((f) => f.slice(ROOT.length + 1))
      .sort();
    expect(users).toEqual(Object.keys(cases).sort());
    for (const file of users) {
      // Every call in the file, not only the first: each must declare the policy of the row.
      const text = fs.readFileSync(`${ROOT}/${file}`, 'utf8');
      const calls = text.split('withRequestStatus(').length - 1;
      const declared = [...text.matchAll(/withRequestStatus\(\{\s*retry:\s*(true|false)\b/g)].map((m) => m[1]);
      expect(`${file}: ${calls} call(s), retry ${declared.join(', ')}`).toBe(`${file}: ${calls} call(s), retry ${Array(calls).fill(String(cases[file]?.retry)).join(', ')}`);
    }
  });

  it('keeps a retrying store off setLoading and setLoaded: its read changes the status only through the declared handle', async () => {
    const fs = await nodeFs();
    for (const [file, c] of Object.entries(cases)) {
      if (!c.retry) continue;
      const imports = [...fs.readFileSync(`${ROOT}/${file}`, 'utf8').matchAll(/import\s*\{([^}]*)\}\s*from\s*'@decryp7\/paddock-ui\/shell\/store'/g)].map((m) => m[1] ?? '').join(',');
      const used = imports.split(',').map((s) => s.trim()).filter((s) => s === 'setLoading' || s === 'setLoaded');
      expect(`${file}: ${used.join(', ') || 'none'}`).toBe(`${file}: none`);
    }
  });

  it('gives a temporary opt-out ("not adopted yet") a removal date that has not passed', async () => {
    const fs = await nodeFs();
    const today = new Date().toISOString().slice(0, 10);
    for (const [file, c] of Object.entries(cases)) {
      if (c.retry) continue;
      const text = fs.readFileSync(`${ROOT}/${file}`, 'utf8');
      if (!/why:\s*(['`])not adopted yet/.test(text)) continue;
      const until = /until:\s*'(\d{4}-\d{2}-\d{2})'/.exec(text)?.[1];
      const verdict = until === undefined ? 'no until date' : until < today ? `until ${until} has passed: adopt the retry or move the date with a reason` : 'dated';
      expect(`${file}: ${verdict}`).toBe(`${file}: dated`);
    }
  });

  it('gives every opt-out a reason', async () => {
    const fs = await nodeFs();
    for (const [file, c] of Object.entries(cases)) {
      if (c.retry) continue;
      const why = /withRequestStatus\(\{\s*retry:\s*false,\s*why:\s*(['`])(.*?)\1/s.exec(fs.readFileSync(`${ROOT}/${file}`, 'utf8'))?.[2] ?? '';
      expect(`${file}: ${why.length >= 20 ? 'has a reason' : `reason too short: "${why}"`}`).toBe(`${file}: has a reason`);
    }
  });

  const retrying = Object.entries(cases).filter((e): e is [string, RetryCase] => e[1].retry);

  describe.each(retrying)('%s', (_file, c) => {
    it('reads again on its own after a failed read, and the error ends when that read succeeds', async () => {
      const { store, clock, read } = await failedFirst(c);
      expect(read).toHaveBeenCalledTimes(1);
      clock.advance(2_000);
      await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
      expect(read).toHaveBeenCalledTimes(2);
    });

    if (c.event?.carries === 'snapshot') {
      it('ends the error when an event brings the whole state', async () => {
        const { store, emit, read } = await failedFirst(c);
        emit();
        await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
        expect(read).toHaveBeenCalledTimes(1);
      });
    }

    if (c.event?.carries === 'change') {
      it('reads again when an event arrives while in error, and the error ends when that read succeeds', async () => {
        const { store, emit, read } = await failedFirst(c);
        emit();
        await vi.waitFor(() => expect(read).toHaveBeenCalledTimes(2));
        await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
      });
    }

    it('leaves no timer after the store is destroyed', async () => {
      const { clock, read } = await failedFirst(c);
      TestBed.resetTestingModule();
      expect(clock.ticking).toBe(0);
      clock.advance(120_000);
      expect(read).toHaveBeenCalledTimes(1);
    });
  });
});
