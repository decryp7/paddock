import { TestBed } from '@angular/core/testing';
import { MemoryAssistantBackend, TauriAssistantBackend, type SendRequest } from '@decryp7/paddock-ui/workbench/assistant';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { BACKEND } from '../../core/backend';
import { browserReply, ChatSaving, createChatBackend, createChatSpeech, resetChatBackend, SavingAssistantBackend } from './chat-backend';

const create = () => {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [ChatSaving] });
  const backend = TestBed.runInInjectionContext(() => createChatBackend()) as SavingAssistantBackend;
  return { backend, saving: TestBed.inject(ChatSaving) };
};

describe('createChatBackend', () => {
  afterEach(() => {
    resetChatBackend();
    vi.unstubAllGlobals();
  });

  it('is the memory engine in the browser, one per page so a tab switch keeps the conversations', () => {
    const a = create().backend;
    expect(a.inner).toBeInstanceOf(MemoryAssistantBackend);
    expect(create().backend.inner).toBe(a.inner);
    resetChatBackend();
    expect(create().backend.inner).not.toBe(a.inner);
  });

  it("is the app's assistant commands inside Tauri", async () => {
    const invoke = vi.fn(() => Promise.resolve({ provider: 'Pitwall', tools: [], hasWrite: false }));
    vi.stubGlobal('window', Object.assign(window, { __TAURI_INTERNALS__: { invoke, transformCallback: () => 1 } }));
    const b = create().backend;
    expect(b.inner).toBeInstanceOf(TauriAssistantBackend);
    await b.info();
    expect(invoke).toHaveBeenCalledWith('assistant_info', {});
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it('adds the "Don\'t save" choice to the question that starts a chat, and only to that one', async () => {
    const { backend, saving } = create();
    const sent: SendRequest[] = [];
    vi.spyOn(backend.inner, 'send').mockImplementation((r: SendRequest) => {
      sent.push(r);
      return Promise.resolve({ conversationId: r.conversationId ?? `c${sent.length}` });
    });
    saving.saveNext.set(false);
    const ack = await backend.send({ text: 'secret', context: [], allowWrite: false });
    expect(sent[0]!.save).toBe(false);
    expect(saving.started.get(ack.conversationId)).toBe(false);
    await backend.send({ conversationId: ack.conversationId, text: 'more', context: [], allowWrite: false });
    expect(sent[1]).not.toHaveProperty('save');
    saving.saveNext.set(true);
    const saved = await backend.send({ text: 'kept', context: [], allowWrite: false });
    expect(sent[2]!.save).toBe(true);
    expect(saving.started.get(saved.conversationId)).toBe(true);
  });

  it("measures the browser build's meter like the app: 8192 less a 1024 reserve, a file at about four bytes a token (review 2 P2-F)", async () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [ChatSaving, { provide: BACKEND, useValue: { chatAttachedBytes: (sha: string) => (sha === 'a'.repeat(64) ? 4000 : undefined) } }] });
    const backend = TestBed.runInInjectionContext(() => createChatBackend());
    const file = (c: string) => ({ kind: 'file' as const, sha256: c.repeat(64), name: `${c}.txt` });
    expect(await backend.context(null, [file('a')])).toEqual({ usedTokens: 1040, limitTokens: 7168, reserveTokens: 1024, exact: false, level: 'ok' });
    expect((await backend.context(null, [file('b')]))?.usedTokens).toBe(290);
  });

  it("passes the context meter's question to the engine (ADR 0023 WP-4b)", async () => {
    const { backend } = create();
    const used = { usedTokens: 10, limitTokens: 100, reserveTokens: 0, exact: true, level: 'ok' as const };
    const spy = vi.spyOn(backend.inner, 'context').mockResolvedValue(used);
    const file = { kind: 'file' as const, sha256: 'a'.repeat(64), name: 'a.txt' };
    expect(await backend.context('c1', [file])).toEqual(used);
    expect(spy).toHaveBeenCalledWith('c1', [file]);
  });

  it("reads the model's window again before measuring and before sending, so a per-slot change reaches both (review 3 P2-1)", async () => {
    const order: string[] = [];
    const inner = new MemoryAssistantBackend();
    vi.spyOn(inner, 'context').mockImplementation(() => (order.push('context'), Promise.resolve(null)));
    vi.spyOn(inner, 'send').mockImplementation(() => (order.push('send'), Promise.resolve({ conversationId: 'c1' })));
    let fail = false;
    const refresh = (): Promise<unknown> => (order.push('refresh'), fail ? Promise.reject(new Error('server down')) : Promise.resolve(4096));
    const backend = new SavingAssistantBackend(inner, new ChatSaving(), refresh);
    await backend.context('c1');
    await backend.send({ conversationId: 'c1', text: 'q', context: [], allowWrite: false });
    expect(order).toEqual(['refresh', 'context', 'refresh', 'send']);
    // A refresh that fails keeps the last window: the meter and the question still go on (the engine checks again).
    fail = true;
    await backend.context('c1');
    expect(order.slice(-2)).toEqual(['refresh', 'context']);
  });

  it("the browser build's stand-in answers on the question's subject (review P2-18)", () => {
    expect(browserReply('Why did the backups pause?')).toContain('data volume filled up');
    expect(browserReply('How busy is this computer?')).toContain('CPU');
    expect(browserReply('hello')).toContain('short answer');
  });

  it('has no speech input in the app yet (WP-7); the browser build plays it only when asked', () => {
    expect(createChatSpeech('?mock-fast')).toBeNull();
    expect(createChatSpeech('?mock-speech')).not.toBeNull();
  });
});
