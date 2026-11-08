import { MemoryAssistantBackend, TauriAssistantBackend, type AssistantBackend } from '@decryp7/paddock-ui/workbench/assistant';
import { tauriIpc } from '../../shell/ipc';

let browser: AssistantBackend | null = null;

/**
 * The Playground chat's engine (ADR 0023 Decision 1: `paddock-assistant` in the app process). Inside Tauri it is the app's
 * `assistant_*` commands and the `paddock://assistant` event; in the browser build (development, UI tests) the shared
 * memory engine, one per page so its conversations outlive a tab switch. It is never wrapped in a per-call timeout: a
 * turn returns at once and runs on, and nothing in the UI ends it but Stop (ER-19).
 */
export function createChatBackend(): AssistantBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriAssistantBackend(ipc);
  return (browser ??= new MemoryAssistantBackend());
}

/** Forgets the browser build's one chat engine (specs that start the page more than once). */
export function resetChatBackend(): void {
  browser = null;
}
