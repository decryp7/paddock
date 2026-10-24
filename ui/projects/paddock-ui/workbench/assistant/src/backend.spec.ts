import { describe, expect, it } from 'vitest';
import { ASSISTANT_COMMANDS } from '../generated/assistant-commands';
import { ASSISTANT_EVENT, type AssistantInfo, type Envelope } from '../generated/assistant';
import { FIXTURE_CONVERSATION, FIXTURE_EVENTS } from '../generated/assistant-fixtures';
import { MemoryAssistantBackend, TauriAssistantBackend, type AssistantIpc } from './backend';

const settle = async (n = 20): Promise<void> => {
  for (let i = 0; i < n; i++) await Promise.resolve();
};

function fakeIpc(answers: Record<string, unknown>) {
  const calls: [string, unknown][] = [];
  const handlers: ((p: unknown) => void)[] = [];
  const ipc: AssistantIpc = {
    invoke: <T>(command: string, args?: Record<string, unknown>) => {
      calls.push([command, args]);
      const a = answers[command];
      return a instanceof Error ? Promise.reject(a) : Promise.resolve(a as T);
    },
    listen: (event, handler) => {
      expect(event).toBe(ASSISTANT_EVENT);
      handlers.push(handler);
      return Promise.resolve(() => void handlers.splice(handlers.indexOf(handler), 1));
    },
  };
  return { ipc, calls, handlers };
}

describe('the generated commands', () => {
  it('are the seven the backend calls', () => {
    expect(Object.keys(ASSISTANT_COMMANDS).sort()).toEqual(['assistant_approve_tool', 'assistant_conversations', 'assistant_delete', 'assistant_info', 'assistant_load', 'assistant_send', 'assistant_stop']);
  });
});

describe('TauriAssistantBackend', () => {
  const info: AssistantInfo = { provider: 'Pitwall', tools: [{ name: 'a', description: 'd', write: false }], hasWrite: false };

  it('calls the engine with the generated command names and parses what comes back', async () => {
    const { ipc, calls } = fakeIpc({
      assistant_info: info,
      assistant_conversations: [{ id: 'c', title: 't', updatedMs: 1, messageCount: 2 }, 'junk'],
      assistant_load: FIXTURE_CONVERSATION,
      assistant_send: { conversationId: 'c1' },
      assistant_stop: true,
      assistant_delete: null,
      assistant_approve_tool: null,
    });
    const b = new TauriAssistantBackend(ipc);
    expect(await b.info()).toEqual(info);
    expect(await b.conversations()).toHaveLength(1);
    expect((await b.load('x')).id).toBe(FIXTURE_CONVERSATION.id);
    expect(await b.send({ text: 'q', context: [], allowWrite: false })).toEqual({ conversationId: 'c1' });
    expect(await b.stop('c1')).toBe(true);
    await b.delete('c1');
    await b.approve('c1', 'call_1', true);
    expect(calls.map((c) => c[0])).toEqual(['assistant_info', 'assistant_conversations', 'assistant_load', 'assistant_send', 'assistant_stop', 'assistant_delete', 'assistant_approve_tool']);
    expect(calls[3]?.[1]).toEqual({ request: { text: 'q', context: [], allowWrite: false } });
    expect(calls[6]?.[1]).toEqual({ conversationId: 'c1', callId: 'call_1', approved: true });
    for (const [command] of calls) expect(Object.keys(ASSISTANT_COMMANDS)).toContain(command);
  });

  it('refuses answers that are not what the contract says', async () => {
    const b = new TauriAssistantBackend(fakeIpc({ assistant_info: {}, assistant_load: {}, assistant_send: {}, assistant_stop: 'no' }).ipc);
    await expect(b.info()).rejects.toThrow('no valid assistant information');
    await expect(b.load('x')).rejects.toThrow('no valid conversation');
    await expect(b.send({ text: 'q', context: [], allowWrite: false })).rejects.toThrow('did not start');
    expect(await b.stop('x')).toBe(false);
  });

  it('parses events at the edge and stops listening', async () => {
    const { ipc, handlers } = fakeIpc({});
    const got: Envelope[] = [];
    const stop = await new TauriAssistantBackend(ipc).listen((e) => got.push(e));
    handlers[0]?.(FIXTURE_EVENTS[0]);
    handlers[0]?.({ nonsense: true });
    expect(got).toHaveLength(1);
    stop();
    expect(handlers).toHaveLength(0);
  });
});

describe('MemoryAssistantBackend', () => {
  async function ask(b: MemoryAssistantBackend, text: string, allowWrite = false, conversationId?: string) {
    const events: Envelope[] = [];
    await b.listen((e) => events.push(e));
    const ack = await b.send({ ...(conversationId ? { conversationId } : {}), text, context: [], allowWrite });
    return { events, ack };
  }

  it('streams a canned answer word by word and keeps the conversation', async () => {
    const b = new MemoryAssistantBackend();
    const { events, ack } = await ask(b, 'hello there');
    await settle();
    expect(events.map((e) => e.type)).toEqual(['started', 'delta', 'delta', 'delta', 'delta', 'done']);
    expect(events.at(-1)).toMatchObject({ type: 'done', text: 'You asked: hello there', end: 'answered' });
    expect((await b.load(ack.conversationId)).messages.map((m) => m.role)).toEqual(['user', 'assistant']);
    expect((await b.conversations())[0]).toMatchObject({ id: ack.conversationId, messageCount: 2 });
    // A second question continues it.
    await b.send({ conversationId: ack.conversationId, text: 'again', context: [], allowWrite: false });
    await settle();
    expect((await b.load(ack.conversationId)).messages).toHaveLength(4);
    await b.delete(ack.conversationId);
    await expect(b.load(ack.conversationId)).rejects.toThrow('no longer exists');
    await expect(b.delete('nope')).rejects.toThrow();
    expect(b.sent).toHaveLength(2);
  });

  it('runs a read-only tool with citations, and a write only after the approval', async () => {
    const b = new MemoryAssistantBackend();
    const read = await ask(b, '/tool things');
    await settle();
    expect(read.events.map((e) => e.type).slice(0, 3)).toEqual(['started', 'toolStarted', 'toolFinished']);
    const write = await ask(b, '/write', true);
    await settle(5);
    expect(write.events.some((e) => e.type === 'approvalNeeded')).toBe(true);
    expect(b.erased).toBe(0);
    await expect(b.approve(write.ack.conversationId, 'wrong', true)).rejects.toThrow();
    await b.approve(write.ack.conversationId, 'call_2', true);
    await settle();
    expect(b.erased).toBe(1);
    expect(b.approvals).toEqual([['call_2', true]]);
    // Without "allow actions" the write is not offered at all.
    const plain = await ask(new MemoryAssistantBackend(), '/write');
    await settle();
    expect(plain.events.some((e) => e.type === 'approvalNeeded')).toBe(false);
  });

  it('can be held and stopped, and refuses a question on request', async () => {
    const b = new MemoryAssistantBackend({ reply: () => 'one two three' });
    b.hold();
    const { events, ack } = await ask(b, 'q');
    await settle();
    expect(events.map((e) => e.type)).toEqual(['started']);
    expect(await b.stop(ack.conversationId)).toBe(true);
    await settle();
    expect(events.at(-1)).toMatchObject({ type: 'done', end: 'cancelled' });
    expect(await b.stop(ack.conversationId)).toBe(false);
    await expect(new MemoryAssistantBackend({ sendError: 'No.' }).send({ text: 'q', context: [], allowWrite: false })).rejects.toThrow('No.');
    const info = await new MemoryAssistantBackend().info();
    expect(info.hasWrite).toBe(true);
    const seeded = new MemoryAssistantBackend({ conversations: [FIXTURE_CONVERSATION], info: { provider: 'x', tools: [], hasWrite: false } });
    expect((await seeded.conversations())[0]?.id).toBe(FIXTURE_CONVERSATION.id);
    seeded.emit('c', { type: 'notice', message: 'x' });
  });
});
