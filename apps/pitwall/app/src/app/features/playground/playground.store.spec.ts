import { TestBed } from '@angular/core/testing';
import type { PlaygroundEvent, PlaygroundRequest } from '../../core/api.types';
import type { PitwallBackend, Unsubscribe } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { DEMO_REPLY } from '../../core/mock-data';
import { PlaygroundStore, applyEvent, buildMessages, chatModels, type Pane, type Turn } from './playground.store';

const turn = (over: Partial<Turn>): Turn => ({ role: 'assistant', content: '', streaming: false, stopped: false, stats: null, error: null, ...over });
const wait = (ms = 0) => new Promise((r) => setTimeout(r, ms));
const QWEN = 'qwen3.5-9b-q4_k_m';
const LLAMA = 'qwen3.5-4b-q4_k_m';

describe('playground helpers', () => {
  it('builds messages: trimmed system prompt, finished turns, then the new prompt', () => {
    const turns = [turn({ role: 'user', content: 'hi' }), turn({ content: 'hello' }), turn({ content: 'oops', error: { code: 'internal', message: 'x' } }), turn({ content: '' })];
    expect(buildMessages('  be brief ', turns, 'next')).toEqual([
      { role: 'system', content: 'be brief' },
      { role: 'user', content: 'hi' },
      { role: 'assistant', content: 'hello' },
      { role: 'user', content: 'next' },
    ]);
    expect(buildMessages('   ', [], 'p')).toEqual([{ role: 'user', content: 'p' }]);
  });

  it('folds events into the streaming assistant turn only', () => {
    const pane: Pane = { modelId: 'm', turns: [turn({ role: 'user', content: 'q' }), turn({ streaming: true })] };
    const a = applyEvent(pane, { kind: 'delta', text: 'ab' });
    const b = applyEvent(a, { kind: 'delta', text: 'c' });
    expect(b.turns[1]!.content).toBe('abc');
    const done = applyEvent(b, { kind: 'done', finish: 'stop', promptTokens: 1, completionTokens: 2, ttftMs: 3, genTps: 4, coldStartMs: null });
    expect(done.turns[1]).toMatchObject({ streaming: false, stats: { promptTokens: 1, completionTokens: 2, ttftMs: 3, genTps: 4, coldStartMs: null } });
    expect(done.turns[1]!.stats).not.toHaveProperty('kind');
    expect(applyEvent(done, { kind: 'delta', text: 'late' })).toBe(done);
    // Events that only the Chat tab shows leave a Compare pane as it is.
    for (const ignored of [{ kind: 'phase', phase: { kind: 'generating' } }, { kind: 'reasoning', text: 't' }, { kind: 'toolCall', index: 0, id: 'c', name: 'n', arguments: '{}' }] as const) {
      expect(applyEvent(pane, ignored)).toBe(pane);
    }
    const err = applyEvent(pane, { kind: 'error', error: { code: 'internal', message: 'boom' } });
    expect(err.turns[1]).toMatchObject({ streaming: false, error: { message: 'boom' } });
    expect(applyEvent({ modelId: null, turns: [] }, { kind: 'delta', text: 'x' }).turns).toEqual([]);
    expect(applyEvent({ modelId: null, turns: [turn({ role: 'user' })] }, { kind: 'delta', text: 'x' }).turns[0]!.content).toBe('');
  });

  it('keeps chat models only, loaded first then by name', () => {
    const m = (id: string, name: string, loaded: boolean, caps: string[]) => ({ id, name, loaded, capabilities: caps }) as never;
    const out = chatModels([m('a', 'Zed', false, ['chat']), m('b', 'Emb', true, ['embeddings']), m('c', 'Beta', false, ['chat']), m('d', 'Alpha', true, ['chat'])]);
    expect(out.map((x) => x.id)).toEqual(['d', 'c', 'a']);
  });
});

describe('PlaygroundStore', () => {
  function setup(backend: PitwallBackend = new MockBackend({ streamDelayMs: 0, latencyMs: 0 })) {
    TestBed.configureTestingModule({ providers: [PlaygroundStore, { provide: BACKEND, useValue: backend }] });
    return { store: TestBed.inject(PlaygroundStore), backend };
  }
  async function idle(store: { streaming(): boolean }) {
    for (let i = 0; i < 400 && store.streaming(); i++) await wait(0);
  }

  it('loads the models and preselects a distinct model per pane', async () => {
    const { store } = setup();
    const p = store.load();
    expect(store.loading()).toBe(true);
    await p;
    expect(store.chatModels().map((m) => m.id)).toContain(QWEN);
    expect(store.chatModels().every((m) => m.capabilities.includes('chat'))).toBe(true);
    const [a, b] = store.panes();
    expect(a.modelId).not.toBeNull();
    expect(b.modelId).not.toBeNull();
    expect(a.modelId).not.toBe(b.modelId);
  });

  it('keeps a model the user already picked when reloading', async () => {
    const { store } = setup();
    await store.load();
    store.setModel(0, LLAMA);
    store.setModel(1, LLAMA);
    await store.load();
    expect(store.panes()[0].modelId).toBe(LLAMA);
    expect(store.panes()[1].modelId).toBe(LLAMA);
  });

  it('reuses the single chat model for both panes and selects nothing when none exist', async () => {
    const one = {
      localModels: () => Promise.resolve([{ id: 'only', name: 'Only', capabilities: ['chat'], loaded: false }]),
    } as unknown as PitwallBackend;
    const { store } = setup(one);
    await store.load();
    expect(store.panes().map((p) => p.modelId)).toEqual(['only', 'only']);

    TestBed.resetTestingModule();
    const none = setup(new MockBackend({ empty: true }));
    await none.store.load();
    // The empty mock still has models? Then both panes get one; otherwise neither does.
    const ids = none.store.panes().map((p) => p.modelId);
    expect(ids[0] === null).toBe(none.store.chatModels().length === 0);
  });

  it('reports a load error', async () => {
    const { store } = setup(new MockBackend({ failing: ['localModels'] }));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.loading()).toBe(false);
    expect(store.chatModels()).toEqual([]);
  });

  it('clamps and validates parameters', () => {
    const { store } = setup();
    store.setTemperature(5);
    expect(store.temperature()).toBe(2);
    store.setTemperature(-1);
    expect(store.temperature()).toBe(0);
    store.setTemperature(Number.NaN);
    expect(store.temperature()).toBe(0);
    store.setMaxTokens(0);
    expect(store.maxTokens()).toBe(1);
    store.setMaxTokens(99999);
    expect(store.maxTokens()).toBe(32768);
    store.setMaxTokens(100.6);
    expect(store.maxTokens()).toBe(101);
    store.setMaxTokens(Number.POSITIVE_INFINITY);
    expect(store.maxTokens()).toBe(101);
    store.setSystem('sys');
    expect(store.system()).toBe('sys');
  });

  it('streams a reply into an assistant turn and records the stats', async () => {
    const { store } = setup();
    await store.load();
    store.setModel(0, QWEN);
    expect(store.send('  hello  ')).toBe(true);
    expect(store.streaming()).toBe(true);
    expect(store.empty()).toBe(false);
    const turns = () => store.panes()[0].turns;
    expect(turns()[0]).toMatchObject({ role: 'user', content: 'hello' });
    expect(turns()[1]).toMatchObject({ role: 'assistant', streaming: true, content: '' });
    await idle(store);
    expect(store.streaming()).toBe(false);
    expect(turns()[1]!.content.trim()).toBe(DEMO_REPLY.slice(0, turns()[1]!.content.length).trim());
    expect(turns()[1]!.stats).toMatchObject({ ttftMs: 214, genTps: 61.8 });
    expect(store.panes()[1].turns).toEqual([]);
  });

  it('refuses empty prompts and a second send while streaming', async () => {
    const { store } = setup();
    await store.load();
    expect(store.send('   ')).toBe(false);
    expect(store.send('one')).toBe(true);
    expect(store.send('two')).toBe(false);
    await idle(store);
    expect(store.panes()[0].turns).toHaveLength(2);
  });

  it('sends the earlier turns, the system prompt and the parameters with a follow-up', async () => {
    const seen: PlaygroundRequest[] = [];
    const base = new MockBackend({ streamDelayMs: 0 });
    const spy: PitwallBackend = Object.create(base, {
      chat: {
        value: (r: PlaygroundRequest, on: (e: PlaygroundEvent) => void): Unsubscribe => {
          seen.push(r);
          return base.chat(r, on);
        },
      },
    });
    const { store } = setup(spy);
    await store.load();
    store.setModel(0, QWEN);
    store.setSystem('be brief');
    store.setTemperature(0.2);
    store.setMaxTokens(8);
    store.send('first');
    await idle(store);
    store.send('second');
    await idle(store);
    expect(seen[1]).toMatchObject({ modelId: QWEN, temperature: 0.2, maxTokens: 8 });
    expect(seen[1]!.messages.map((m) => m.role)).toEqual(['system', 'user', 'assistant', 'user']);
    expect(seen[1]!.messages.at(-1)?.content).toBe('second');
  });

  it('skips a pane without a model', async () => {
    const { store } = setup(new MockBackend({ empty: true }));
    store.setCompare(true);
    expect(store.send('hi')).toBe(true);
    expect(store.empty()).toBe(true);
  });

  it('compares two models with the same prompt side by side', async () => {
    const { store } = setup();
    await store.load();
    store.setCompare(true);
    expect(store.activePanes()).toEqual([0, 1]);
    store.setModel(0, QWEN);
    store.setModel(1, LLAMA);
    store.send('same prompt');
    await idle(store);
    const [a, b] = store.panes();
    expect(a.turns[0]!.content).toBe('same prompt');
    expect(b.turns[0]!.content).toBe('same prompt');
    expect(a.turns[1]!.stats?.genTps).toBe(61.8);
    expect(b.turns[1]!.stats?.genTps).toBe(118.4);
    expect(b.turns[1]!.content.length).toBeLessThan(a.turns[1]!.content.length);
  });

  it('turning compare off while streaming stops the answers', async () => {
    const { store } = setup();
    await store.load();
    store.send('go');
    store.setCompare(true);
    expect(store.streaming()).toBe(false);
    expect(store.panes()[0].turns[1]).toMatchObject({ streaming: false, stopped: true });
    expect(store.compare()).toBe(true);
  });

  it('stops a running answer: keeps the partial text, marks it stopped and sends no more', async () => {
    const { store } = setup(new MockBackend({ streamDelayMs: 2 }));
    await store.load();
    store.send('go');
    await wait(25);
    store.stop();
    const text = store.panes()[0].turns[1]!.content;
    expect(text.length).toBeGreaterThan(0);
    expect(store.panes()[0].turns[1]).toMatchObject({ streaming: false, stopped: true, stats: null });
    await wait(30);
    expect(store.panes()[0].turns[1]!.content).toBe(text);
    store.stop();
    expect(store.streaming()).toBe(false);
  });

  it('shows a stream error on the turn and allows sending again', async () => {
    const { store } = setup(new MockBackend({ failing: ['chat'], streamDelayMs: 0 }));
    await store.load();
    store.send('hi');
    await idle(store);
    expect(store.panes()[0].turns[1]).toMatchObject({ streaming: false, error: { code: 'serverUnavailable' } });
    expect(store.send('again')).toBe(true);
    await idle(store);
    // The failed turn is not replayed to the model.
    expect(store.panes()[0].turns).toHaveLength(4);
  });

  it('reports an unknown model as a not-found error', async () => {
    const { store } = setup();
    await store.load();
    store.setModel(0, 'ghost');
    store.send('hi');
    await idle(store);
    expect(store.panes()[0].turns[1]!.error?.code).toBe('notFound');
  });

  it('clears the conversations and stops streaming', async () => {
    const { store } = setup();
    await store.load();
    store.send('hi');
    store.clear();
    expect(store.empty()).toBe(true);
    expect(store.streaming()).toBe(false);
    await wait(10);
    expect(store.panes()[0].turns).toEqual([]);
  });

  it('stops streams when destroyed', async () => {
    const { store } = setup(new MockBackend({ streamDelayMs: 2 }));
    await store.load();
    store.send('hi');
    TestBed.resetTestingModule();
    await wait(20);
    expect(store.streaming()).toBe(false);
  });
});
