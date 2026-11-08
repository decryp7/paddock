import { MemoryAssistantBackend, TauriAssistantBackend } from '@decryp7/paddock-ui/workbench/assistant';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createChatBackend, resetChatBackend } from './chat-backend';

describe('createChatBackend', () => {
  afterEach(() => {
    resetChatBackend();
    vi.unstubAllGlobals();
  });

  it('is the memory engine in the browser, one per page so a tab switch keeps the conversations', () => {
    const a = createChatBackend();
    expect(a).toBeInstanceOf(MemoryAssistantBackend);
    expect(createChatBackend()).toBe(a);
    resetChatBackend();
    expect(createChatBackend()).not.toBe(a);
  });

  it("is the app's assistant commands inside Tauri", async () => {
    const invoke = vi.fn(() => Promise.resolve({ provider: 'Pitwall', tools: [], hasWrite: false }));
    vi.stubGlobal('window', Object.assign(window, { __TAURI_INTERNALS__: { invoke, transformCallback: () => 1 } }));
    const b = createChatBackend();
    expect(b).toBeInstanceOf(TauriAssistantBackend);
    await b.info();
    expect(invoke).toHaveBeenCalledWith('assistant_info', {});
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });
});
