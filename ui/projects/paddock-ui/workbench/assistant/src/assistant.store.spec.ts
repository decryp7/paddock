import { TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { msg } from '@decryp7/paddock-ui/shell';
import { provideShellTesting, settle } from '@decryp7/paddock-ui/shell/testing';
import { defineAssistantContext, provideWorkbenchContributions } from '@decryp7/paddock-ui/workbench';
import { FIXTURE_CONVERSATION } from '../generated/assistant-fixtures';
import { ASSISTANT_LIMITS } from '../generated/assistant';
import { AssistantStore, UNDO_MS } from './assistant.store';
import { AssistantBackend, MemoryAssistantBackend, type MemoryAssistantOptions } from './backend';

interface Ctx {
  readonly selection: string;
}

function setup(options: MemoryAssistantOptions = {}, selection = 'main.rs:12') {
  const backend = new MemoryAssistantBackend(options);
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      ...provideShellTesting<Ctx>({
        context: () => Promise.resolve(() => ({ selection })),
        providers: [
          { provide: AssistantBackend, useValue: backend },
          provideWorkbenchContributions<Ctx>({
            assistantContext: [
              defineAssistantContext<Ctx>({ id: 'wd.selection', label: msg('wd.ctx.selection', 'Selection'), icon: 'pd:info', order: 100, value: (c) => ({ text: c.selection, detail: 'lines' }) }),
              defineAssistantContext<Ctx>({ id: 'wd.optional', label: msg('wd.ctx.optional', 'Open file'), order: 200, defaultOn: false, value: () => ({ text: 'file text' }) }),
              defineAssistantContext<Ctx>({ id: 'wd.empty', label: msg('wd.ctx.empty', 'Empty'), order: 300, value: () => null }),
              defineAssistantContext<Ctx>({
                id: 'wd.throws',
                label: msg('wd.ctx.throws', 'Throws'),
                order: 400,
                value: () => {
                  throw new Error('boom');
                },
              }),
              defineAssistantContext<Ctx>({ id: 'wd.long', label: msg('wd.ctx.long', 'Long'), order: 500, value: () => ({ text: 'x'.repeat(ASSISTANT_LIMITS.contextChars + 100) }) }),
            ],
          }),
        ],
      }),
      AssistantStore,
    ],
  });
  return { backend, store: TestBed.inject(AssistantStore) };
}

/** Lets the store's hooks, the backend's microtasks and the stream finish. */
const quiet = async (n = 4): Promise<void> => {
  for (let i = 0; i < n; i++) await settle();
};

describe('AssistantStore', () => {
  it('loads the info and the conversations, and lists the context chips (bounded, defensive, removable)', async () => {
    const { store } = setup({ conversations: [FIXTURE_CONVERSATION] });
    await quiet();
    expect(store.isLoaded()).toBe(true);
    expect(store.info()?.provider).toBe('Pitwall (demo)');
    expect(store.conversations().map((c) => c.id)).toEqual([FIXTURE_CONVERSATION.id]);
    expect(store.hasWriteTools()).toBe(true);
    // A chip that is empty or throws is left out; an optional one starts off; a long one is cut.
    const chips = store.chips();
    expect(chips.map((c) => [c.id, c.on])).toEqual([
      ['wd.selection', true],
      ['wd.optional', false],
      ['wd.long', true],
    ]);
    expect(chips[0]).toMatchObject({ label: 'Selection', detail: 'lines', icon: 'pd:info', text: 'main.rs:12' });
    expect(chips[2]?.text.length).toBe(ASSISTANT_LIMITS.contextChars);
    store.toggleChip('wd.selection');
    store.toggleChip('wd.optional');
    store.toggleChip('wd.nothing');
    expect(store.chips().map((c) => c.on)).toEqual([false, true, true]);
    store.toggleChip('wd.selection');
    store.toggleChip('wd.optional');
    expect(store.chips().map((c) => c.on)).toEqual([true, false, true]);
  });

  it('reports a failed load and loads again', async () => {
    const { backend, store } = setup();
    backend.info = () => Promise.reject(new Error('The engine is not there.'));
    await store.load();
    expect(store.requestError()).toBe('The engine is not there.');
    backend.info = MemoryAssistantBackend.prototype.info.bind(backend);
    await store.load();
    expect(store.isLoaded()).toBe(true);
  });

  it('sends the draft with the chips that are on and streams the answer into the last turn', async () => {
    const { backend, store } = setup();
    await quiet();
    store.toggleChip('wd.long');
    store.setDraft('  what is this?  ');
    expect(store.canSend()).toBe(true);
    const sending = store.send();
    expect(store.run()).toBe('sending');
    expect(store.draft()).toBe('');
    await sending;
    await quiet();
    expect(backend.sent[0]).toEqual({ text: 'what is this?', context: [{ id: 'wd.selection', label: 'Selection: lines', text: 'main.rs:12' }], allowWrite: false });
    expect(store.run()).toBe('idle');
    expect(store.activeId()).toMatch(/^m/);
    expect(store.answer()).toBe('You asked: what is this?');
    expect(store.lastTurn()).toMatchObject({ running: false, end: 'answered', user: 'what is this?' });
    // The chips come back for the next question; the list shows the new conversation.
    expect(store.chips().find((c) => c.id === 'wd.long')?.on).toBe(true);
    expect(store.conversations()).toHaveLength(1);
    // The next question continues the same conversation.
    store.setDraft('more');
    await store.send();
    await quiet();
    expect(backend.sent[1]?.conversationId).toBe(store.activeId());
    expect(store.turns()).toHaveLength(2);
  });

  it('does not send an empty draft or while busy, and gives the question back when the engine refuses it', async () => {
    const { backend, store } = setup({ sendError: 'The assistant is answering already.' });
    await quiet();
    await store.send();
    expect(backend.sent).toHaveLength(0);
    store.setDraft('q');
    await store.send();
    expect(store.draft()).toBe('q');
    expect(store.turns()).toHaveLength(0);
    expect(store.failure()).toBe('The assistant is answering already.');
    expect(store.run()).toBe('idle');
    store.dismissFailure();
    expect(store.failure()).toBeNull();
  });

  it('stops a running answer and keeps what was said', async () => {
    const { backend, store } = setup({ reply: () => 'one two three four' });
    await quiet();
    backend.hold();
    store.setDraft('go');
    await store.send();
    await quiet(2);
    expect(store.run()).toBe('running');
    expect(store.busy()).toBe(true);
    const stopping = store.stop();
    expect(store.run()).toBe('stopping');
    await stopping;
    await quiet();
    expect(store.run()).toBe('idle');
    expect(store.lastTurn()).toMatchObject({ end: 'cancelled', running: false });
    // Nothing runs: stopping again does nothing.
    await store.stop();
    expect(store.run()).toBe('idle');
  });

  it('shows the approval of a write and sends the decision', async () => {
    const { backend, store } = setup();
    await quiet();
    store.setAllowWrite(true);
    store.setDraft('/write');
    await store.send();
    await quiet(2);
    expect(store.approval()).toMatchObject({ callId: 'call_2', name: 'erase', summary: 'Erase everything' });
    expect(backend.sent[0]?.allowWrite).toBe(true);
    await store.approve('call_2', false);
    await quiet();
    expect(store.approval()).toBeNull();
    expect(backend.erased).toBe(0);
    expect(backend.approvals).toEqual([['call_2', false]]);
    // A decision nobody waits for is the engine's refusal, shown.
    await store.approve('call_2', true);
    expect(store.failure()).toBe('No action waits for a decision.');
  });

  it('does not offer writes when the app has none', async () => {
    const { backend, store } = setup({ info: { provider: 'p', tools: [], hasWrite: false } });
    await quiet();
    store.setAllowWrite(true);
    store.setDraft('q');
    await store.send();
    expect(backend.sent[0]?.allowWrite).toBe(false);
  });

  it('opens, starts and deletes conversations', async () => {
    const { store } = setup({ conversations: [FIXTURE_CONVERSATION] });
    await quiet();
    await store.open(FIXTURE_CONVERSATION.id);
    expect(store.activeId()).toBe(FIXTURE_CONVERSATION.id);
    expect(store.turns()).toHaveLength(1);
    store.toggleListOpen();
    expect(store.listOpen()).toBe(true);
    store.newConversation();
    expect(store.turns()).toHaveLength(0);
    expect(store.listOpen()).toBe(false);
    await store.open('nope');
    expect(store.failure()).toBe('That conversation no longer exists.');
    await store.open(FIXTURE_CONVERSATION.id);
    await store.remove(FIXTURE_CONVERSATION.id);
    expect(store.activeId()).toBeNull();
    expect(store.conversations()).toEqual([]);
    await store.remove('nope');
    expect(store.failure()).toBe('That conversation no longer exists.');
  });

  it('keeps the conversation while it answers, and hears events of other conversations only to refresh the list', async () => {
    const { backend, store } = setup({ conversations: [FIXTURE_CONVERSATION] });
    await quiet();
    // A conversation is busy: a new one cannot start.
    store.setDraft('first');
    await store.send();
    store.newConversation();
    expect(store.turns()).toHaveLength(1);
    await store.open('x');
    await quiet();
    store.setDraft('again');
    await store.send();
    backend.emit('someone-else', { type: 'delta', text: 'x' });
    backend.emit('someone-else', { type: 'done', text: 'x', usage: { inputTokens: 0, outputTokens: 0 }, end: 'answered' });
    await quiet();
    expect(store.answer()).toBe('You asked: again');
    expect(store.turns()).toHaveLength(2);
  });

  it('adopts the conversation named by the first event when it arrives before the ack', async () => {
    const { backend, store } = setup();
    await quiet();
    backend.send = (request) => {
      queueMicrotask(() => backend.emit('early', { type: 'started', title: 't' }));
      return Promise.resolve().then(() => Promise.resolve().then(() => Promise.resolve().then(() => ({ conversationId: request.conversationId ?? 'early' }))));
    };
    store.setDraft('q');
    await store.send();
    expect(store.activeId()).toBe('early');
    expect(store.run()).toBe('running');
    backend.emit('early', { type: 'error', kind: 'failed', message: 'Pitwall stopped.' });
    await quiet();
    expect(store.run()).toBe('idle');
    expect(store.lastTurn()?.error?.message).toBe('Pitwall stopped.');
  });

  it('reports commands that fail', async () => {
    const { backend, store } = setup({ conversations: [FIXTURE_CONVERSATION] });
    await quiet();
    backend.conversations = () => Promise.reject(new Error('no list'));
    backend.stop = () => Promise.reject(new Error('no stop'));
    backend.delete = () => Promise.reject(new Error('no delete'));
    await store.remove(FIXTURE_CONVERSATION.id);
    expect(store.failure()).toBe('no list');
    store.setDraft('q');
    await store.send();
    await store.stop();
    expect(store.failure()).toBe('no stop');
  });

  it('listens once and stops listening when destroyed', async () => {
    const { backend, store } = setup();
    await quiet();
    await store.listen();
    store.setDraft('q');
    await store.send();
    await quiet();
    expect(store.answer()).toBe('You asked: q');
    TestBed.resetTestingModule();
    backend.emit('x', { type: 'notice', message: 'late' });
  });

  describe('deleting with undo', () => {
    afterEach(() => vi.useRealTimers());

    it('hides the conversation at once, keeps it until the undo time passes, then deletes it', async () => {
      const { store, backend } = setup({ conversations: [FIXTURE_CONVERSATION] });
      await quiet();
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
      store.removeLater(FIXTURE_CONVERSATION.id);
      store.removeLater(FIXTURE_CONVERSATION.id);
      expect(store.visibleConversations()).toEqual([]);
      expect(store.conversations()).toHaveLength(1);
      expect(store.undoable()?.title).toBe(FIXTURE_CONVERSATION.title);
      await vi.advanceTimersByTimeAsync(UNDO_MS - 1);
      expect(store.conversations()).toHaveLength(1);
      expect((await backend.conversations()).length).toBe(1);
      await vi.advanceTimersByTimeAsync(1);
      vi.useRealTimers();
      await quiet();
      expect(store.conversations()).toEqual([]);
      expect(store.undoable()).toBeNull();
    });

    it('brings it back on undo and never deletes it', async () => {
      const { store, backend } = setup({ conversations: [FIXTURE_CONVERSATION] });
      await quiet();
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
      store.removeLater(FIXTURE_CONVERSATION.id);
      store.undoRemove(FIXTURE_CONVERSATION.id);
      store.undoRemove('nope');
      store.removeLater('nope');
      expect(store.visibleConversations()).toHaveLength(1);
      await vi.advanceTimersByTimeAsync(UNDO_MS * 2);
      expect((await backend.conversations()).length).toBe(1);
    });

    it('lets a pending delete go through when the pane closes', async () => {
      const { store, backend } = setup({ conversations: [FIXTURE_CONVERSATION] });
      await quiet();
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
      store.removeLater(FIXTURE_CONVERSATION.id);
      store.dispose();
      vi.useRealTimers();
      await quiet();
      expect(await backend.conversations()).toEqual([]);
      const { store: other, backend: b2 } = setup({ conversations: [FIXTURE_CONVERSATION] });
      await quiet();
      b2.delete = () => Promise.reject(new Error('no'));
      other.removeLater(FIXTURE_CONVERSATION.id);
      other.dispose();
      await quiet();
    });
  });

  it('titles the pane with the conversation, or "New conversation"', async () => {
    const { store } = setup({ conversations: [FIXTURE_CONVERSATION] });
    await quiet();
    expect(store.title()).toBe('New conversation');
    await store.open(FIXTURE_CONVERSATION.id);
    expect(store.title()).toBe(FIXTURE_CONVERSATION.title);
  });
});
