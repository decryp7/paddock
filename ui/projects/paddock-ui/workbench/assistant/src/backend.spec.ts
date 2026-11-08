import { describe, expect, it } from 'vitest';
import { ASSISTANT_COMMANDS } from '../generated/assistant-commands';
import { ASSISTANT_EVENT, type AssistantInfo, type Attachment, type Envelope } from '../generated/assistant';
import { FIXTURE_CONVERSATION, FIXTURE_EVENTS } from '../generated/assistant-fixtures';
import { DEMO_ANSWER, MemoryAssistantBackend, TauriAssistantBackend, type AssistantIpc } from './backend';

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
  it('are the eight the backend calls', () => {
    expect(Object.keys(ASSISTANT_COMMANDS).sort()).toEqual(['assistant_approve_tool', 'assistant_context', 'assistant_conversations', 'assistant_delete', 'assistant_info', 'assistant_load', 'assistant_send', 'assistant_stop']);
  });
});

describe('TauriAssistantBackend', () => {
  const info: AssistantInfo = { provider: 'Pitwall', tools: [{ name: 'a', description: 'd', write: false, destructive: false }], hasWrite: false };

  it('calls the engine with the generated command names and parses what comes back', async () => {
    const { ipc, calls } = fakeIpc({
      assistant_info: info,
      assistant_conversations: [{ id: 'c', title: 't', updatedMs: 1, messageCount: 2 }, 'junk'],
      assistant_load: FIXTURE_CONVERSATION,
      assistant_send: { conversationId: 'c1' },
      assistant_stop: true,
      assistant_delete: null,
      assistant_approve_tool: null,
      assistant_context: { usedTokens: 900, limitTokens: 3072, reserveTokens: 1024, exact: true, level: 'ok' },
    });
    const b = new TauriAssistantBackend(ipc);
    expect(await b.info()).toEqual(info);
    expect(await b.conversations()).toHaveLength(1);
    expect((await b.load('x')).id).toBe(FIXTURE_CONVERSATION.id);
    expect(await b.send({ text: 'q', context: [], allowWrite: false })).toEqual({ conversationId: 'c1' });
    expect(await b.stop('c1')).toBe(true);
    await b.delete('c1');
    await b.approve('c1', 'call_1', 'alwaysAllow');
    expect(await b.context(null)).toEqual({ usedTokens: 900, limitTokens: 3072, reserveTokens: 1024, exact: true, level: 'ok' });
    expect(calls.map((c) => c[0])).toEqual(['assistant_info', 'assistant_conversations', 'assistant_load', 'assistant_send', 'assistant_stop', 'assistant_delete', 'assistant_approve_tool', 'assistant_context']);
    expect(calls[7]?.[1]).toEqual({ conversationId: null, attachments: [] });
    expect(calls[3]?.[1]).toEqual({ request: { text: 'q', context: [], allowWrite: false } });
    expect(calls[6]?.[1]).toEqual({ conversationId: 'c1', callId: 'call_1', choice: 'alwaysAllow' });
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
  it('answers in markdown: a paragraph, a list, inline code and a code block', () => {
    expect(DEMO_ANSWER).toMatch(/^[A-Z].*\.\n\n/);
    expect(DEMO_ANSWER).toMatch(/\n- .+\n- /);
    expect(DEMO_ANSWER).toMatch(/`handle_request`/);
    expect(DEMO_ANSWER).toMatch(/```rust\n[\s\S]+\n```/);
  });

  async function ask(b: MemoryAssistantBackend, text: string, allowWrite = false, conversationId?: string) {
    const events: Envelope[] = [];
    await b.listen((e) => events.push(e));
    const ack = await b.send({ ...(conversationId ? { conversationId } : {}), text, context: [], allowWrite });
    return { events, ack };
  }

  it("measures like the engine: the window less a 1024-token answer reserve, files at about four bytes a token (review 2 P2-F)", async () => {
    const file = (sha: string, name: string): Attachment => ({ kind: 'file', sha256: sha.repeat(64), name });
    const bytes = new Map([['a'.repeat(64), 4000]]);
    const b = new MemoryAssistantBackend({ attachmentTokens: (a) => Math.ceil((bytes.get(a.sha256) ?? 0) / 4) });
    // A new chat: about 40 tokens of system prompt and tools, in 8192 less the reserve.
    expect(await b.context(null)).toEqual({ usedTokens: 40, limitTokens: 7168, reserveTokens: 1024, exact: false, level: 'ok' });
    expect((await b.context(null, [file('a', 'notes.txt')]))?.usedTokens).toBe(1040);
    // Without a size hook a file counts a plausible 250 tokens; a window of 0 is unknown and keeps no room.
    expect((await new MemoryAssistantBackend().context(null, [file('b', 'x.txt')]))?.usedTokens).toBe(290);
    expect(await new MemoryAssistantBackend({ contextTokens: 0 }).context(null)).toMatchObject({ limitTokens: 0, level: 'unknown' });
    // A small window still keeps at least one token of room (as the engine does) and is full past it.
    expect(await new MemoryAssistantBackend({ contextTokens: 1074 }).context(null)).toMatchObject({ limitTokens: 50, level: 'high' });
    expect(await new MemoryAssistantBackend({ contextTokens: 1000 }).context(null)).toMatchObject({ limitTokens: 1, level: 'full' });
  });

  it('streams a canned answer word by word and keeps the conversation', async () => {
    const b = new MemoryAssistantBackend();
    const { events, ack } = await ask(b, 'hello there');
    await settle(300);
    expect(events[0]?.type).toBe('started');
    expect(events.slice(1, -1).every((e) => e.type === 'delta')).toBe(true);
    expect(events.at(-1)).toMatchObject({ type: 'done', text: DEMO_ANSWER, end: 'answered' });
    expect((await b.load(ack.conversationId)).messages.map((m) => m.role)).toEqual(['user', 'assistant']);
    expect((await b.conversations())[0]).toMatchObject({ id: ack.conversationId, messageCount: 2 });
    // A second question continues it.
    await b.send({ conversationId: ack.conversationId, text: 'again', context: [], allowWrite: false });
    await settle(300);
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
    await expect(b.approve(write.ack.conversationId, 'wrong', 'allowOnce')).rejects.toThrow();
    // The demo's write asks every time, like the engine's default policy: Always allow is refused.
    await expect(b.approve(write.ack.conversationId, 'call_2', 'alwaysAllow')).rejects.toThrow('cannot be always allowed');
    await b.approve(write.ack.conversationId, 'call_2', 'allowOnce');
    await settle();
    expect(b.erased).toBe(1);
    expect(write.events.at(-1)).toMatchObject({ type: 'done', text: 'Done. I erased everything.' });
    const declined = await ask(b, '/write', true);
    await settle(5);
    await b.approve(declined.ack.conversationId, 'call_2', 'deny');
    await settle();
    expect(b.erased).toBe(1);
    expect(declined.events.at(-1)).toMatchObject({ type: 'done', text: 'I did not erase anything.' });
    expect(b.approvals).toEqual([
      ['call_2', 'allowOnce'],
      ['call_2', 'deny'],
    ]);
    // Without "allow actions" the write is not offered at all.
    const plain = await ask(new MemoryAssistantBackend(), '/write');
    await settle();
    expect(plain.events.some((e) => e.type === 'approvalNeeded')).toBe(false);
  });

  it('asks a read-only call first with Always allow, remembers it, and reports a cold start (ADR 0023 WP-4a)', async () => {
    const b = new MemoryAssistantBackend();
    const denied = await ask(b, '/look');
    await settle(5);
    expect(denied.events.find((e) => e.type === 'approvalNeeded')).toMatchObject({ remember: true, callId: 'call_1' });
    await b.approve(denied.ack.conversationId, 'call_1', 'deny');
    await settle();
    expect(denied.events.find((e) => e.type === 'toolFinished')).toMatchObject({ ok: false, outcome: 'declined' });
    const once = await ask(b, '/look');
    await settle(5);
    await b.approve(once.ack.conversationId, 'call_1', 'allowOnce');
    await settle();
    expect(once.events.find((e) => e.type === 'toolFinished')).toMatchObject({ ok: true, approval: 'once' });
    const always = await ask(b, '/look');
    await settle(5);
    await b.approve(always.ack.conversationId, 'call_1', 'alwaysAllow');
    await settle();
    expect(always.events.find((e) => e.type === 'toolFinished')).toMatchObject({ approval: 'always' });
    const later = await ask(b, '/look');
    await settle();
    expect(later.events.some((e) => e.type === 'approvalNeeded')).toBe(false);
    expect(later.events.find((e) => e.type === 'toolFinished')).toMatchObject({ approval: 'remembered' });
    expect((await b.load(later.ack.conversationId)).messages.find((m) => m.role === 'tool')?.approval).toBe('remembered');
    const cold = await ask(b, '/cold');
    await settle(300);
    expect(cold.events.filter((e) => e.type === 'phase').map((e) => (e.type === 'phase' ? e.phase : null))).toEqual([
      { kind: 'queued' },
      { kind: 'loading', elapsedMs: 0 },
      { kind: 'loading', elapsedMs: 12_000 },
      { kind: 'generating' },
    ]);
    expect(cold.events.at(-1)).toMatchObject({ type: 'done', end: 'answered' });
  });

  it("answers plain questions with the host's reply and keeps the scripts of the slash commands", async () => {
    const b = new MemoryAssistantBackend({ reply: (q) => `about ${q}` });
    const plain = await ask(b, 'disks');
    await settle();
    expect((await b.load(plain.ack.conversationId)).messages.at(-1)?.text).toBe('about disks');
    const tool = await ask(b, '/tool things');
    await settle();
    expect((await b.load(tool.ack.conversationId)).messages.at(-1)?.text).toBe('Found item 7 for you.');
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

  it('plays /fail (words, then an error of kind failed) and /slow (words, then waits for Stop)', async () => {
    const b = new MemoryAssistantBackend();
    const failed = await ask(b, '/fail now');
    await settle(300);
    expect(failed.events.some((e) => e.type === 'delta')).toBe(true);
    expect(failed.events.filter((e) => e.type === 'delta').map((e) => (e as { text: string }).text).join('')).not.toBe(DEMO_ANSWER);
    expect(failed.events.at(-1)).toMatchObject({ type: 'error', kind: 'failed', message: 'The model stopped answering.' });
    expect(await b.stop(failed.ack.conversationId)).toBe(false);

    const slow = await ask(b, '/slow please');
    await settle(300);
    expect(slow.events.some((e) => e.type === 'delta')).toBe(true);
    expect(slow.events.some((e) => e.type === 'done')).toBe(false);
    expect(await b.stop(slow.ack.conversationId)).toBe(true);
    await settle(5);
    expect(slow.events.at(-1)).toMatchObject({ type: 'done', end: 'cancelled' });
  });

  it('plays /tool-slow (a tool card that stays running until Stop) and /tool-fail (a failed card with a reason)', async () => {
    const b = new MemoryAssistantBackend();
    const slow = await ask(b, '/tool-slow lookup');
    await settle(30);
    expect(slow.events.map((e) => e.type)).toEqual(['started', 'toolStarted']);
    expect(slow.events[1]).toMatchObject({ summary: 'lookup', write: false, destructive: false });
    expect(await b.stop(slow.ack.conversationId)).toBe(true);
    await settle(30);
    // As the engine does: the stopped call reports itself as stopped by the user (not as a failure of the tool), and nothing is said.
    expect(slow.events.some((e) => e.type === 'delta')).toBe(false);
    expect(slow.events.find((e) => e.type === 'toolFinished')).toMatchObject({ ok: false, outcome: 'stopped' });
    expect(slow.events.at(-1)).toMatchObject({ type: 'done', end: 'cancelled' });

    const fail = await ask(b, '/tool-fail');
    await settle(300);
    expect(fail.events.slice(0, 3).map((e) => e.type)).toEqual(['started', 'toolStarted', 'toolFinished']);
    expect(fail.events[2]).toMatchObject({ ok: false, summary: 'The index is locked by another process.' });
    expect(fail.events.at(-1)).toMatchObject({ type: 'done', end: 'answered', text: expect.stringContaining('index is locked') });
  });

  it('keeps /tool giving a citation and only the first word selects the behaviour', async () => {
    const b = new MemoryAssistantBackend();
    const tool = await ask(b, '/tool');
    await settle(30);
    expect(tool.events[2]).toMatchObject({ type: 'toolFinished', ok: true, data: { citations: [{ target: 'record:7' }] } });
    expect(tool.events.at(-1)).toMatchObject({ type: 'done', text: 'Found item 7 for you.' });
    const plain = await ask(b, 'what does /tool-fail do');
    await settle(300);
    expect(plain.events.some((e) => e.type === 'toolStarted')).toBe(false);
  });
});
