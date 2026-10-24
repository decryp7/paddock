import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  MemoryPacksBackend,
  memoryKind,
  PACKS_COMMANDS,
  PACKS_EVENT,
  PACKS_SETTINGS_COMMANDS,
  PacksBackend,
  PacksStore,
  parsePacksView,
  provideShellPacks,
  TauriPacksBackend,
  type PacksIpc,
  type PacksView,
} from '@decryp7/paddock-ui/shell/packs';

async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

const BAD = [{ file: 'broken.toml', message: 'not valid TOML: expected `=` (line 2, column 3)' }];
const PACKS = [
  { name: 'builtin.toml', origin: 'builtin' as const },
  { name: 'mine.toml', origin: 'user' as const },
];

function store(backend: PacksBackend) {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellPacks({ backend: () => backend })] });
  return TestBed.inject(PacksStore);
}

describe('the packs store', () => {
  it('asks the engine on init and lists the kinds', async () => {
    const s = store(new MemoryPacksBackend());
    expect(s.view()).toBeNull();
    await settle();
    expect(s.requestStatus()).toBe('loaded');
    expect(s.kinds().map((k) => k.id)).toEqual(['cleanup']);
    expect(s.problemCount()).toBe(0);
  });

  it('folds a change the engine found by itself into the view', async () => {
    const memory = new MemoryPacksBackend();
    const s = store(memory);
    await settle();
    memory.emit({ kinds: [memoryKind('cleanup', 'Cleanup packs', PACKS, BAD)] });
    expect(s.problemCount()).toBe(1);
    expect(s.kinds()[0]?.health.level).toBe('degraded');
  });

  it('reload asks the engine and shows the result; the fix clears the problem', async () => {
    const memory = new MemoryPacksBackend({ kinds: [memoryKind('cleanup', 'Cleanup packs', PACKS, BAD)], onReload: 'fix' });
    const s = store(memory);
    await settle();
    expect(s.problemCount()).toBe(1);
    const done = s.reload();
    expect(s.reloading()).toBe(true);
    await done;
    expect(memory.reloads).toBe(1);
    expect(s.reloading()).toBe(false);
    expect(s.problemCount()).toBe(0);
  });

  it('a reload that is already running is not started twice', async () => {
    const memory = new MemoryPacksBackend();
    const s = store(memory);
    await settle();
    const first = s.reload();
    void s.reload();
    await first;
    expect(memory.reloads).toBe(1);
  });

  it('a failed ask is an error with Retry, and the retry loads', async () => {
    const s = store(new MemoryPacksBackend({ statusFailures: 1 }));
    await settle();
    expect(s.requestError()).toBe('The packs could not be read.');
    await s.load();
    expect(s.requestStatus()).toBe('loaded');
  });

  it('a failed reload keeps the view and says why', async () => {
    class Failing extends MemoryPacksBackend {
      override async reload(): Promise<PacksView> {
        throw 'the engine is busy';
      }
    }
    const s = store(new Failing());
    await settle();
    await s.reload();
    expect(s.reloadError()).toBe('the engine is busy');
    expect(s.kinds().length).toBe(1);
    await s.reload();
    expect(s.reloading()).toBe(false);
  });

  it('stops listening when it is destroyed, also when the listener arrives late', async () => {
    let off = 0;
    class Late extends MemoryPacksBackend {
      override async listen(): Promise<() => void> {
        await settle(3);
        return () => void off++;
      }
    }
    store(new Late());
    TestBed.resetTestingModule();
    await settle();
    expect(off).toBe(1);
  });

  it('a listener that cannot start is logged, not fatal', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    class NoListen extends MemoryPacksBackend {
      override listen(): Promise<() => void> {
        return Promise.reject(new Error('no events'));
      }
    }
    const s = store(new NoListen());
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    expect(s.kinds().length).toBe(1);
    warn.mockRestore();
  });
});

describe('the wire', () => {
  const kind = memoryKind('k', 'K', PACKS, BAD, [{ file: 'a.toml', message: 'unknown key "x" is ignored' }]);

  it('parses a view and rebuilds it from known fields only', () => {
    const parsed = parsePacksView({ kinds: [{ ...kind, extra: 1 }], extra: 2 });
    expect(parsed).toEqual({ kinds: [kind] });
  });

  it.each([null, 'x', [], {}, { kinds: 'x' }, { kinds: [null] }, { kinds: [{ ...kind, id: 5 }] }, { kinds: [{ ...kind, packs: [{ name: 'a', origin: 'other' }] }] }, { kinds: [{ ...kind, problems: [{ file: 1 }] }] }, { kinds: [{ ...kind, health: { level: 'degraded' } }] }, { kinds: [{ ...kind, health: 3 }] }, { kinds: [{ ...kind, warnings: 'x' }] }])('refuses %j', (payload) => {
    expect(parsePacksView(payload)).toBeNull();
  });

  it('knows the health levels', () => {
    const levels = (h: unknown) => parsePacksView({ kinds: [{ ...kind, health: h }] })?.kinds[0]?.health;
    expect(levels({ level: 'ok' })).toEqual({ level: 'ok' });
    expect(levels({ level: 'notes', message: 'm' })).toEqual({ level: 'notes', message: 'm' });
  });

  it('the fake engine words its health like the engine', () => {
    expect(kind.health).toEqual({ level: 'degraded', message: 'broken.toml: not valid TOML: expected `=` (line 2, column 3) (and 1 more)' });
    expect(memoryKind('k', 'K', [], [], [{ file: 'a.toml', message: 'm' }]).health.level).toBe('notes');
    expect(memoryKind('k', 'K', []).health).toEqual({ level: 'ok' });
  });
});

describe('the Tauri backend', () => {
  function ipc(answers: Record<string, unknown>) {
    const calls: string[] = [];
    let handler: ((p: unknown) => void) | undefined;
    const port: PacksIpc = {
      invoke: async <T>(c: string) => {
        calls.push(c);
        return answers[c] as T;
      },
      listen: async (event, h) => {
        expect(event).toBe(PACKS_EVENT);
        handler = h;
        return () => void (handler = undefined);
      },
    };
    return { port, calls, send: (p: unknown) => handler?.(p) };
  }

  it('asks the two commands and parses the answers', async () => {
    const view = { kinds: [memoryKind('k', 'K', PACKS)] };
    const i = ipc({ packs_status: view, packs_reload: view });
    const b = new TauriPacksBackend(i.port);
    expect(await b.status()).toEqual(view);
    expect(await b.reload()).toEqual(view);
    expect(i.calls).toEqual(['packs_status', 'packs_reload']);
  });

  it('refuses an answer that is not a pack list', async () => {
    const b = new TauriPacksBackend(ipc({ packs_status: 'nope' }).port);
    await expect(b.status()).rejects.toThrow('no valid pack list');
  });

  it('hands on valid events and drops the others', async () => {
    const i = ipc({});
    const seen: PacksView[] = [];
    const off = await new TauriPacksBackend(i.port).listen((v) => seen.push(v));
    i.send({ kinds: [] });
    i.send('garbage');
    expect(seen).toEqual([{ kinds: [] }]);
    off();
    i.send({ kinds: [] });
    expect(seen.length).toBe(1);
  });

  it('the settings window may call exactly the page commands, and they exist', () => {
    expect([...PACKS_SETTINGS_COMMANDS].sort()).toEqual(Object.keys(PACKS_COMMANDS).sort());
  });
});
