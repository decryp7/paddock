import { TestBed } from '@angular/core/testing';
import { MemoryAssistantBackend, TauriAssistantBackend, type SendRequest } from '@decryp7/paddock-ui/workbench/assistant';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ChatSaving, createChatBackend, createChatSpeech, resetChatBackend, SavingAssistantBackend } from './chat-backend';

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

  it("passes the context meter's question to the engine (ADR 0023 WP-4b)", async () => {
    const { backend } = create();
    const used = { usedTokens: 10, limitTokens: 100, reserveTokens: 0, exact: true, level: 'ok' as const };
    const spy = vi.spyOn(backend.inner, 'context').mockResolvedValue(used);
    expect(await backend.context('c1')).toEqual(used);
    expect(spy).toHaveBeenCalledWith('c1');
  });

  it('has no speech input in the app yet (WP-7); the browser build plays it only when asked', () => {
    expect(createChatSpeech('?mock-fast')).toBeNull();
    expect(createChatSpeech('?mock-speech')).not.toBeNull();
  });
});
