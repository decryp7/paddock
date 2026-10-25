import { Component, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { PITWALL_COMMANDS, PITWALL_EVENT, type AiSnapshot, type AiState, type NextStep } from '../generated/pitwall';
import { MemoryPitwallBackend, PitwallBackend, TauriPitwallBackend, type PitwallIpc } from './backend';
import { PdNeedsAi, stepLabel } from './needs-ai';
import { PitwallStore } from './pitwall.store';
import { providePitwall } from './provide-pitwall';
import { groupCode, parseSnapshot, pitwallMessage } from './wire';
import { assertStatusGlyphs } from '@decryp7/paddock-ui/testing';

async function settle(rounds = 8): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

const snapshot = (state: AiState, over: Partial<AiSnapshot> = {}): AiSnapshot => ({
  state,
  app: 'Debrief',
  message: 'Debrief is not connected to Pitwall yet.',
  actions: ['pair'],
  pairing: null,
  builtInAvailable: true,
  builtInForSession: false,
  ...over,
});

describe('the wire', () => {
  it('accepts what the engine sends, every state, and refuses anything else', () => {
    const states: AiState[] = [
      { kind: 'disabled' },
      { kind: 'notInstalled' },
      { kind: 'notRunning' },
      { kind: 'incompatible', message: 'm', updateApp: true },
      { kind: 'needsPairing' },
      { kind: 'paused', stopped: false },
      { kind: 'noSuitableModel', needs: { chat: true, vision: true, tools: false, json: false, embeddings: false, contextTokens: 8192, fast: false } },
      { kind: 'unavailable', message: 'm' },
      { kind: 'ready', model: 'q', vision: false, contextTokens: 4096 },
    ];
    for (const s of states) expect(parseSnapshot(snapshot(s))).toEqual(snapshot(s));
    const withCode = snapshot({ kind: 'needsPairing' }, { pairing: { code: '482913', expiresAtMs: 5 } });
    expect(parseSnapshot({ ...withCode, future: 1 })).toEqual(withCode);
    const bad: unknown[] = [
      null,
      [],
      {},
      { ...snapshot({ kind: 'ready', model: 'q', vision: false, contextTokens: 1 }), state: { kind: 'ready', model: 1 } },
      { ...snapshot({ kind: 'disabled' }), state: { kind: 'teleport' } },
      { ...snapshot({ kind: 'disabled' }), state: { kind: 'paused' } },
      { ...snapshot({ kind: 'disabled' }), state: { kind: 'incompatible', message: 1, updateApp: true } },
      { ...snapshot({ kind: 'disabled' }), state: { kind: 'unavailable' } },
      { ...snapshot({ kind: 'disabled' }), state: { kind: 'noSuitableModel', needs: { chat: 1 } } },
      { ...snapshot({ kind: 'disabled' }), state: { kind: 'noSuitableModel', needs: { chat: true, vision: true, tools: true, json: true, embeddings: true, fast: true, contextTokens: 'x' } } },
      { ...snapshot({ kind: 'disabled' }), app: 7 },
      { ...snapshot({ kind: 'disabled' }), message: 7 },
      { ...snapshot({ kind: 'disabled' }), actions: ['dance'] },
      { ...snapshot({ kind: 'disabled' }), actions: 'pair' },
      { ...snapshot({ kind: 'disabled' }), pairing: { code: 1 } },
      { ...snapshot({ kind: 'disabled' }), builtInAvailable: 'yes' },
    ];
    for (const b of bad) expect(parseSnapshot(b)).toBeNull();
  });

  it('groups a six-digit code and finds the engine’s words in a refusal', () => {
    expect(groupCode('482913')).toBe('482 913');
    expect(groupCode('12')).toBe('12');
    expect(pitwallMessage('no', 'f')).toBe('no');
    expect(pitwallMessage(new Error('boom'), 'f')).toBe('boom');
    expect(pitwallMessage({ message: 'refused' }, 'f')).toBe('refused');
    expect(pitwallMessage({}, 'f')).toBe('f');
    expect(pitwallMessage(null, 'f')).toBe('f');
  });
});

describe('the Tauri backend', () => {
  function ipc(replies: Record<string, unknown>): PitwallIpc & { calls: string[]; emit(payload: unknown): void; unlistened: number } {
    let handler: ((p: unknown) => void) | null = null;
    const o = {
      calls: [] as string[],
      unlistened: 0,
      invoke: (command: string) => {
        o.calls.push(command);
        return Promise.resolve(replies[command]) as Promise<never>;
      },
      listen: (event: string, h: (p: unknown) => void) => {
        expect(event).toBe(PITWALL_EVENT);
        handler = h;
        return Promise.resolve(() => void o.unlistened++);
      },
      emit: (p: unknown) => handler?.(p),
    };
    return o;
  }

  it('calls the four commands of the contract and parses what comes back', async () => {
    const snap = snapshot({ kind: 'needsPairing' });
    const o = ipc({ pitwall_state: snap, pitwall_connect: snap, pitwall_use_builtin: snap, pitwall_cancel_pairing: true });
    const b = new TauriPitwallBackend(o);
    expect(await b.snapshot()).toEqual(snap);
    expect(await b.connect()).toEqual(snap);
    expect(await b.useBuiltIn()).toEqual(snap);
    expect(await b.cancelPairing()).toBe(true);
    expect(o.calls).toEqual(['pitwall_state', 'pitwall_connect', 'pitwall_use_builtin', 'pitwall_cancel_pairing']);
    for (const c of o.calls) expect(PITWALL_COMMANDS as readonly string[]).toContain(c);
  });

  it('refuses a reply that is not a snapshot, and hears only valid events', async () => {
    const o = ipc({ pitwall_state: { nonsense: true }, pitwall_cancel_pairing: false });
    const b = new TauriPitwallBackend(o);
    await expect(b.snapshot()).rejects.toThrow('no valid Pitwall state');
    expect(await b.cancelPairing()).toBe(false);
    const heard: AiSnapshot[] = [];
    const off = await b.listen((s) => heard.push(s));
    o.emit({ junk: 1 });
    o.emit(snapshot({ kind: 'notRunning' }, { actions: ['startPitwall'] }));
    expect(heard.map((s) => s.state.kind)).toEqual(['notRunning']);
    off();
    expect(o.unlistened).toBe(1);
  });
});

describe('the memory backend plays the engine', () => {
  it('walks pairing from the code to ready, and the repair works without Pitwall', async () => {
    const m = new MemoryPitwallBackend({ state: { kind: 'needsPairing' }, app: 'Debrief' });
    const heard: AiSnapshot[] = [];
    const off = await m.listen((s) => heard.push(s));
    expect((await m.snapshot()).actions).toEqual(['pair']);
    const paired = await m.connect();
    expect(paired.pairing?.code).toBe('482913');
    expect(await m.cancelPairing()).toBe(true);
    expect(await m.cancelPairing()).toBe(false);
    await m.connect();
    m.approve();
    expect((await m.snapshot()).state.kind).toBe('ready');
    expect((await m.snapshot()).pairing).toBeNull();
    m.approve();
    const own = await m.useBuiltIn();
    expect(own.builtInForSession).toBe(true);
    off();
    const count = heard.length;
    m.setState({ kind: 'notRunning' });
    m.setBuiltInAvailable(false);
    expect(heard.length).toBe(count);
    expect((await m.snapshot()).builtInAvailable).toBe(false);
    expect(m.connects).toBe(2);
  });

  it('says every state in the engine’s words with its buttons', async () => {
    const cases: [AiState, NextStep[], string][] = [
      [{ kind: 'disabled' }, [], 'turned off'],
      [{ kind: 'notInstalled' }, ['installPitwall'], 'not installed'],
      [{ kind: 'notRunning' }, ['startPitwall'], 'not running'],
      [{ kind: 'incompatible', message: 'Update it.', updateApp: true }, ['updateApp'], 'Update it.'],
      [{ kind: 'incompatible', message: 'Update it.', updateApp: false }, ['updatePitwall'], 'Update it.'],
      [{ kind: 'paused', stopped: true }, ['startPitwall'], 'stopped'],
      [{ kind: 'paused', stopped: false }, ['startPitwall'], 'paused'],
      [{ kind: 'noSuitableModel', needs: { chat: true, vision: true, tools: false, json: false, embeddings: false, contextTokens: null, fast: false } }, ['installModel'], 'with vision'],
      [{ kind: 'noSuitableModel', needs: { chat: true, vision: false, tools: false, json: false, embeddings: false, contextTokens: null, fast: false } }, ['installModel'], 'no model that'],
      [{ kind: 'unavailable', message: 'Odd.' }, ['wait'], 'Odd.'],
      [{ kind: 'ready', model: 'q', vision: false, contextTokens: 1 }, [], 'answers with q'],
    ];
    for (const [state, actions, words] of cases) {
      const s = await new MemoryPitwallBackend({ state, app: 'Demo' }).snapshot();
      expect(s.actions).toEqual(actions);
      expect(s.message).toContain(words);
    }
  });

  it('refuses what the engine would refuse', async () => {
    await expect(new MemoryPitwallBackend({ state: { kind: 'notRunning' } }).connect()).rejects.toThrow('not waiting');
    await expect(new MemoryPitwallBackend({ state: { kind: 'needsPairing' }, connectError: 'Pitwall quit.' }).connect()).rejects.toThrow('Pitwall quit.');
    await expect(new MemoryPitwallBackend({ builtInAvailable: false }).useBuiltIn()).rejects.toThrow('no engine of its own');
  });
});

@Component({ selector: 'pd-test-host', imports: [PdNeedsAi], template: `<pd-needs-ai feature="Ask about this log" [settingsLink]="settings" (step)="steps.push($event)" (openSettings)="opened = opened + 1" />` })
class Host {
  settings = true;
  steps: NextStep[] = [];
  opened = 0;
}

function bed(memory: MemoryPitwallBackend) {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), providePitwall({ backend: () => memory })] });
  return memory;
}

async function render() {
  const fixture = TestBed.createComponent(Host);
  fixture.detectChanges();
  await settle();
  await fixture.whenStable();
  fixture.detectChanges();
  return fixture;
}

const text = (el: HTMLElement): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();
const q = (f: { nativeElement: HTMLElement }, id: string): HTMLElement | null => f.nativeElement.querySelector(`[data-testid="${id}"]`);

describe('the store', () => {
  it('loads the engine’s snapshot, follows its events and keeps the failure of a command', async () => {
    const memory = bed(new MemoryPitwallBackend({ state: { kind: 'needsPairing' }, app: 'Debrief' }));
    const store = TestBed.inject(PitwallStore);
    await settle();
    expect(store.isLoaded() && store.kind()).toBe('needsPairing');
    expect(store.needsAttention()).toBe(true);
    expect(store.canUseBuiltIn()).toBe(true);
    await store.connect();
    expect(store.pairingCode()).toBe('482 913');
    memory.approve();
    expect(store.ready()).toBe(true);
    expect(store.needsAttention()).toBe(false);
    await store.useBuiltIn();
    expect(store.onBuiltIn() && !store.canUseBuiltIn()).toBe(true);
    memory.setState({ kind: 'notRunning' });
    expect(store.message()).toContain('not running');
    await store.connect();
    expect(store.actionError()).toContain('not waiting');
    expect(store.busy()).toBe(false);
    store.dismissError();
    expect(store.actionError()).toBeNull();
    await store.cancelPairing();
    expect(store.actionError()).toBeNull();
  });

  it('shows a failed first read as an error with a way to read again, and stops listening on destroy', async () => {
    let fail = true;
    let listening = 0;
    const flaky = new (class extends PitwallBackend {
      snapshot() {
        return fail ? Promise.reject(new Error('The engine is busy.')) : Promise.resolve(snapshot({ kind: 'notRunning' }, { actions: ['startPitwall'] }));
      }
      connect() { return this.snapshot(); }
      useBuiltIn() { return this.snapshot(); }
      cancelPairing() { return Promise.resolve(false); }
      listen() {
        listening++;
        return Promise.resolve(() => void listening--);
      }
    })();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), providePitwall({ backend: () => flaky })] });
    const store = TestBed.inject(PitwallStore);
    await settle();
    expect(store.requestError()).toBe('The engine is busy.');
    expect(store.needsAttention()).toBe(false);
    fail = false;
    await store.load();
    expect(store.isLoaded() && store.kind()).toBe('notRunning');
    expect(listening).toBe(1);
    TestBed.resetTestingModule();
    await settle();
    expect(listening).toBe(0);
  });

  it('survives a backend that cannot listen', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const memory = new MemoryPitwallBackend({ state: { kind: 'notRunning' } });
    vi.spyOn(memory, 'listen').mockRejectedValue(new Error('no events'));
    bed(memory);
    TestBed.inject(PitwallStore);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    warn.mockRestore();
  });
});

describe('<pd-needs-ai>', () => {
  it('draws nothing while Pitwall answers or AI is off', async () => {
    bed(new MemoryPitwallBackend({ state: { kind: 'ready', model: 'q', vision: false, contextTokens: 8192 } }));
    const f = await render();
    expect(q(f, 'needs-ai')).toBeNull();
    TestBed.resetTestingModule();
    bed(new MemoryPitwallBackend({ state: { kind: 'disabled' } }));
    expect(q(await render(), 'needs-ai')).toBeNull();
  });

  it('offers connect when Pitwall runs, shows the code to compare, and lets the user cancel', async () => {
    const memory = bed(new MemoryPitwallBackend({ state: { kind: 'needsPairing' }, app: 'Debrief' }));
    const f = await render();
    const region = q(f, 'needs-ai')!;
    expect(region.getAttribute('aria-labelledby')).toBeTruthy();
    expect(text(region)).toContain('Ask about this log needs local AI');
    expect(text(q(f, 'needs-ai-message')!)).toBe('Debrief is not connected to Pitwall yet.');
    expect(q(f, 'needs-ai-code')).toBeNull();
    (q(f, 'needs-ai-pair') as HTMLButtonElement).click();
    await settle();
    f.detectChanges();
    expect(text(q(f, 'needs-ai-code')!)).toBe('482 913');
    expect(text(q(f, 'needs-ai-approve')!)).toContain('Approve Debrief in Pitwall');
    expect(q(f, 'needs-ai-pair')).toBeNull();
    (q(f, 'needs-ai-cancel') as HTMLButtonElement).click();
    await settle();
    f.detectChanges();
    expect(q(f, 'needs-ai-code')).toBeNull();
    expect(memory.connects).toBe(1);
    (q(f, 'needs-ai-pair') as HTMLButtonElement).click();
    await settle();
    memory.approve();
    f.detectChanges();
    expect(q(f, 'needs-ai')).toBeNull();
  });

  it('uses the built-in engine for the session, without Pitwall, and says so', async () => {
    bed(new MemoryPitwallBackend({ state: { kind: 'notInstalled' } }));
    const f = await render();
    (q(f, 'needs-ai-builtin') as HTMLButtonElement).click();
    await settle();
    f.detectChanges();
    expect(q(f, 'needs-ai-builtin')).toBeNull();
    expect(text(q(f, 'needs-ai-on-builtin')!)).toBe('This session uses the built-in engine.');
  });

  it('has no built-in button for an app without an engine of its own, and opens the settings when asked', async () => {
    bed(new MemoryPitwallBackend({ state: { kind: 'noSuitableModel', needs: { chat: true, vision: false, tools: false, json: false, embeddings: false, contextTokens: 8192, fast: false } }, builtInAvailable: false }));
    const f = await render();
    expect(q(f, 'needs-ai-builtin')).toBeNull();
    (q(f, 'needs-ai-settings') as HTMLButtonElement).click();
    (q(f, 'needs-ai-installModel') as HTMLButtonElement).click();
    expect(f.componentInstance.opened).toBe(1);
    expect(f.componentInstance.steps).toEqual(['installModel']);
    f.componentInstance.settings = false;
    f.changeDetectorRef.markForCheck();
    f.detectChanges();
    expect(q(f, 'needs-ai-settings')).toBeNull();
  });

  it('hands the app the steps only it can run, and reads again on "Try again"', async () => {
    const memory = bed(new MemoryPitwallBackend({ state: { kind: 'notRunning' } }));
    const f = await render();
    (q(f, 'needs-ai-startPitwall') as HTMLButtonElement).click();
    expect(f.componentInstance.steps).toEqual(['startPitwall']);
    memory.setState({ kind: 'unavailable', message: 'Odd.' });
    f.detectChanges();
    expect(q(f, 'needs-ai')!.getAttribute('data-kind')).toBe('unavailable');
    // A blocking state draws the warning shape from the suite's map (review 4 follow-up).
    assertStatusGlyphs(q(f, 'needs-ai')!);
    expect(q(f, 'needs-ai')!.querySelector('pd-status-glyph')?.getAttribute('data-status')).toBe('warning');
    const spy = vi.spyOn(memory, 'snapshot');
    (q(f, 'needs-ai-wait') as HTMLButtonElement).click();
    await settle();
    expect(spy).toHaveBeenCalled();
  });

  it('shows a refused connect inline, in an alert', async () => {
    bed(new MemoryPitwallBackend({ state: { kind: 'needsPairing' }, connectError: 'Pitwall quit while connecting.' }));
    const f = await render();
    (q(f, 'needs-ai-pair') as HTMLButtonElement).click();
    await settle();
    f.detectChanges();
    const alert = q(f, 'needs-ai-error')!;
    expect(alert.getAttribute('role')).toBe('alert');
    expect(text(alert)).toBe('Pitwall quit while connecting.');
  });

  it('shows a state that could not be read with a Retry', async () => {
    const memory = new MemoryPitwallBackend();
    vi.spyOn(memory, 'snapshot').mockRejectedValueOnce(new Error('The engine is busy.'));
    bed(memory);
    const f = await render();
    expect(text(q(f, 'needs-ai-failed')!)).toContain('The engine is busy.');
    (q(f, 'needs-ai-retry') as HTMLButtonElement).click();
    await settle();
    f.detectChanges();
    expect(q(f, 'needs-ai-failed')).toBeNull();
    expect(q(f, 'needs-ai')!.getAttribute('data-kind')).toBe('notInstalled');
  });

  it('labels every step the app runs and none that has nothing to press', () => {
    const steps: NextStep[] = ['startPitwall', 'installPitwall', 'installModel', 'updateApp', 'updatePitwall', 'changePermissions', 'wait'];
    for (const s of steps) expect(stepLabel(s)).toBeTruthy();
    expect(stepLabel('pair')).toBeNull();
    expect(stepLabel('none')).toBeNull();
  });
});
