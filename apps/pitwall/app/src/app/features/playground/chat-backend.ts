import { Injectable, inject, signal } from '@angular/core';
import {
  AssistantBackend,
  AssistantSpeech,
  MemoryAssistantBackend,
  MemoryAssistantSpeech,
  TauriAssistantBackend,
  type ApprovalChoice,
  type AssistantInfo,
  type Attachment,
  type ContextUse,
  type Conversation,
  type ConversationSummary,
  type Envelope,
  type SendAck,
  type SendRequest,
  type Unlisten,
} from '@decryp7/paddock-ui/workbench/assistant';
import { BACKEND } from '../../core/backend';
import { openedWith } from '../../core/backend-factory';
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

/**
 * The assistant's port with the user's "Don't save" choice added to the question that starts a chat, and the model's window
 * read again (Rust's `chat_refresh_window`) before the meter is measured and before a question is sent, so a per-slot change
 * in Settings or a reload with other slots reaches both; the rest passes.
 */
export class SavingAssistantBackend extends AssistantBackend {
  constructor(
    /** The engine's own port. */
    readonly inner: AssistantBackend,
    private readonly saving: ChatSaving,
    /** Reads the window again (none: the window never changes, as in the browser build). */
    private readonly refreshWindow: (() => Promise<unknown>) | null = null,
  ) {
    super();
  }

  /** A failed read keeps the last window: the engine checks again when it sends. */
  private async fresh(): Promise<void> {
    try {
      await this.refreshWindow?.();
    } catch {
      // The last window stands; the server's own error reaches the user on the next model call.
    }
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
    await this.fresh();
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
  override async context(id: string | null, attachments: readonly Attachment[] = []): Promise<ContextUse | null> {
    await this.fresh();
    return this.inner.context(id, attachments);
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
  // Browser build only: `?mock-context=<tokens>` gives the memory engine a small window, so screenshots and specs reach the
  // meter's high and full levels; `?mock-context=unknown` plays a model whose window is not known.
  const wanted = new URLSearchParams(openedWith()).get('mock-context');
  const tokens = wanted === 'unknown' ? -1 : Number(wanted);
  const pitwall = inject(BACKEND, { optional: true });
  // The mock backend took the files (`chatAttach`) and knows their sizes: about four bytes a token, as Rust estimates.
  const sizes = ipc ? null : (pitwall as { chatAttachedBytes?(sha256: string): number | undefined } | null);
  const attachmentTokens = (a: Attachment): number => {
    const bytes = sizes?.chatAttachedBytes?.(a.sha256);
    return bytes === undefined ? 250 : Math.ceil(bytes / 4);
  };
  const room = tokens > 0 ? { contextTokens: tokens } : tokens < 0 ? { contextTokens: 0 } : {};
  const inner = ipc ? new TauriAssistantBackend(ipc) : (browser ??= new MemoryAssistantBackend({ reply: browserReply, attachmentTokens, ...room }));
  // In the app the window follows the model's context per slot (Rust reads it again); the browser build's window is fixed.
  const refresh = ipc && pitwall ? () => pitwall.chatRefreshWindow() : null;
  return new SavingAssistantBackend(inner, saving, refresh);
}

/**
 * The browser build's stand-in model: a short answer on the question's subject (the computer's load, its disks and backups,
 * an attached file), so screenshots and demos read as a chat. The memory engine's slash commands still play their scripts.
 */
export function browserReply(question: string): string {
  const q = question.toLowerCase();
  if (/backup|disk|volume|full|space/.test(q))
    return 'The backups paused because the data volume filled up at 03:10, as **disk-report.md** says. The cleanup script only removes snapshots older than seven days (`prune_snapshots(7)`), so it is safe to run; free at least 20 GB before the next backup.';
  if (/busy|cpu|load|memory|slow/.test(q)) return 'The computer is lightly loaded: CPU at 18 %, memory at 54 %, and no process above 5 % in the last minute.';
  if (/summar|read|file|attached/.test(q)) return 'The file lists three events: the volume filled up at 03:10, backups paused at 03:12, and an alert went out at 03:15.';
  return 'Here is a short answer from the local model. Connect an app in Connected apps to let it look things up.';
}

/**
 * Speech input for the Chat tab: none yet in the app (ADR 0023 WP-7 brings capture and transcription). The browser build
 * plays it with `?mock-speech` for screenshots and specs, through `globalThis.__pitwallSpeech` (its `tick` moves the clock).
 */
export function createChatSpeech(search: string = openedWith()): AssistantSpeech | null {
  if (tauriIpc() || !new URLSearchParams(search).has('mock-speech')) return null;
  const speech = new MemoryAssistantSpeech();
  (globalThis as { __pitwallSpeech?: MemoryAssistantSpeech }).__pitwallSpeech = speech;
  return speech;
}

/** Forgets the browser build's one chat engine (specs that start the page more than once). */
export function resetChatBackend(): void {
  browser = null;
}
