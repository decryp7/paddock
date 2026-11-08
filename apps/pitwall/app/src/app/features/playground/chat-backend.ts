import { Injectable, inject, signal } from '@angular/core';
import {
  AssistantBackend,
  MemoryAssistantBackend,
  TauriAssistantBackend,
  type ApprovalChoice,
  type AssistantInfo,
  type Conversation,
  type ConversationSummary,
  type Envelope,
  type SendAck,
  type SendRequest,
  type Unlisten,
} from '@decryp7/paddock-ui/workbench/assistant';
import { tauriIpc } from '../../shell/ipc';

let browser: AssistantBackend | null = null;

/**
 * Whether the Chat tab's next new chat is saved ("Don't save this chat", ADR 0023 Decision 3), and what each chat started
 * here was sent with. Rust decides and keeps it; this only carries the user's choice to the question that starts a chat.
 */
@Injectable()
export class ChatSaving {
  /** The next new chat is saved (Settings > Chats' default until the user flips the Chat bar's switch). */
  readonly saveNext = signal(true);
  /** Chats started in this tab and whether each was saved, so the switch needs no round trip after a send. */
  readonly started = new Map<string, boolean>();
}

/** The assistant's port with the user's "Don't save" choice added to the question that starts a chat; the rest passes. */
export class SavingAssistantBackend extends AssistantBackend {
  constructor(
    /** The engine's own port. */
    readonly inner: AssistantBackend,
    private readonly saving: ChatSaving,
  ) {
    super();
  }

  info(): Promise<AssistantInfo> {
    return this.inner.info();
  }
  conversations(): Promise<ConversationSummary[]> {
    return this.inner.conversations();
  }
  load(id: string): Promise<Conversation> {
    return this.inner.load(id);
  }
  delete(id: string): Promise<void> {
    return this.inner.delete(id);
  }
  async send(request: SendRequest): Promise<SendAck> {
    if (request.conversationId) return this.inner.send(request);
    const save = this.saving.saveNext();
    const ack = await this.inner.send({ ...request, save });
    this.saving.started.set(ack.conversationId, save);
    return ack;
  }
  stop(id: string): Promise<boolean> {
    return this.inner.stop(id);
  }
  approve(id: string, callId: string, choice: ApprovalChoice): Promise<void> {
    return this.inner.approve(id, callId, choice);
  }
  listen(handler: (event: Envelope) => void): Promise<Unlisten> {
    return this.inner.listen(handler);
  }
}

/**
 * The Playground chat's engine (ADR 0023 Decision 1: `paddock-assistant` in the app process). Inside Tauri it is the app's
 * `assistant_*` commands and the `paddock://assistant` event; in the browser build (development, UI tests) the shared
 * memory engine, one per page so its conversations outlive a tab switch. It is never wrapped in a per-call timeout: a
 * turn returns at once and runs on, and nothing in the UI ends it but Stop (ER-19). The question that starts a chat
 * carries the Chat bar's "Don't save" choice (`ChatSaving`).
 */
export function createChatBackend(): SavingAssistantBackend {
  const saving = inject(ChatSaving);
  const ipc = tauriIpc();
  const inner = ipc ? new TauriAssistantBackend(ipc) : (browser ??= new MemoryAssistantBackend());
  return new SavingAssistantBackend(inner, saving);
}

/** Forgets the browser build's one chat engine (specs that start the page more than once). */
export function resetChatBackend(): void {
  browser = null;
}
