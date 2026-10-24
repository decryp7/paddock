import type { AssistantCommand } from '../generated/assistant-commands';
import { ASSISTANT_EVENT, type AssistantEvent, type AssistantInfo, type Conversation, type ConversationSummary, type Envelope, type SendAck, type SendRequest } from '../generated/assistant';
import { parseConversation, parseEnvelope, parseInfo, parseSummaries } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and an
 * adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface AssistantIpc {
  invoke<T>(command: AssistantCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * The assistant's port (`paddock-assistant`'s commands and its `paddock://assistant` event). The engine decides everything
 * (tools, budgets, approvals, saving); this only asks for the user's choices and shows what the engine reports.
 */
export abstract class AssistantBackend {
  /** The provider's name and the tools. */
  abstract info(): Promise<AssistantInfo>;
  /** The conversations, newest first. */
  abstract conversations(): Promise<ConversationSummary[]>;
  /** One conversation with its messages. */
  abstract load(id: string): Promise<Conversation>;
  abstract delete(id: string): Promise<void>;
  /** Starts a turn; returns at once, the answer arrives through `listen`. */
  abstract send(request: SendRequest): Promise<SendAck>;
  /** Stops the conversation's turn; false when none runs. */
  abstract stop(id: string): Promise<boolean>;
  /** Answers the write waiting in the conversation's turn. */
  abstract approve(id: string, callId: string, approved: boolean): Promise<void>;
  /** Every event until the returned function is called. */
  abstract listen(handler: (event: Envelope) => void): Promise<Unlisten>;
}

/** The backend over Tauri. Payloads are parsed at this edge. */
export class TauriAssistantBackend extends AssistantBackend {
  constructor(private readonly ipc: AssistantIpc) {
    super();
  }

  async info(): Promise<AssistantInfo> {
    const info = parseInfo(await this.ipc.invoke<unknown>('assistant_info'));
    if (!info) throw new Error('The engine sent no valid assistant information.');
    return info;
  }

  async conversations(): Promise<ConversationSummary[]> {
    return parseSummaries(await this.ipc.invoke<unknown>('assistant_conversations'));
  }

  async load(id: string): Promise<Conversation> {
    const c = parseConversation(await this.ipc.invoke<unknown>('assistant_load', { conversationId: id }));
    if (!c) throw new Error('The engine sent no valid conversation.');
    return c;
  }

  async delete(id: string): Promise<void> {
    await this.ipc.invoke<unknown>('assistant_delete', { conversationId: id });
  }

  async send(request: SendRequest): Promise<SendAck> {
    const ack = await this.ipc.invoke<SendAck>('assistant_send', { request });
    if (typeof ack?.conversationId !== 'string') throw new Error('The engine did not start the question.');
    return ack;
  }

  async stop(id: string): Promise<boolean> {
    return (await this.ipc.invoke<unknown>('assistant_stop', { conversationId: id })) === true;
  }

  async approve(id: string, callId: string, approved: boolean): Promise<void> {
    await this.ipc.invoke<unknown>('assistant_approve_tool', { conversationId: id, callId, approved });
  }

  listen(handler: (event: Envelope) => void): Promise<Unlisten> {
    return this.ipc.listen(ASSISTANT_EVENT, (payload) => {
      const e = parseEnvelope(payload);
      if (e) handler(e);
    });
  }
}

/** What the fake engine of [`MemoryAssistantBackend`] answers to a question. */
export interface MemoryAssistantOptions {
  /** The answer to a question (default: a short markdown answer, see `DEMO_ANSWER`); the slash inputs of the class doc run the demo tools. */
  readonly reply?: (text: string) => string;
  /** The info (default: a read-only tool `find` and a writing tool `erase`). */
  readonly info?: AssistantInfo;
  /** Conversations to start with. */
  readonly conversations?: readonly Conversation[];
  /** A question is refused with this message. */
  readonly sendError?: string;
}

/** The default answer of [`MemoryAssistantBackend`]: a paragraph, a list, inline code and a code block (markdown). */
export const DEMO_ANSWER =
  'A request first passes through the router, which picks a handler by method and path.\n\n' +
  'Three stages matter:\n\n' +
  '- Routing matches the path and the method\n' +
  '- Middleware adds the session and the logging\n' +
  '- `handle_request` builds the response\n\n' +
  '```rust\nfn handle_request(req: Request) -> Response {\n    router.dispatch(req)\n}\n```\n\n' +
  'Ask about any stage for more detail.';

/**
 * The assistant held in memory, playing the engine (browser build, demos, tests): deterministic, no timers. A question
 * streams its answer (`DEMO_ANSWER` unless `reply` is set) word by word, one microtask apart. The first word of the
 * question selects a scripted behaviour:
 *
 * - `/tool [what]`: a read-only tool call (`find`) that finishes with a citation (`item 7`), then the answer.
 * - `/tool-slow`: a `find` call that stays running until `stop()`; the turn then ends as cancelled, with no answer.
 * - `/tool-fail`: a `find` call that fails with a reason ("The index is locked by another process."), then the answer.
 * - `/write`: asks for the user's approval of a writing tool (`erase`) and waits for it (needs "Allow actions").
 * - `/fail`: streams a few words and then ends the turn with an error event of kind `failed`.
 * - `/slow`: streams its first words and then waits until `stop()` (a running turn with Stop visible).
 * - anything else: just the answer.
 *
 * `hold()` pauses the stream before the first word until `release()` (so a test can press Stop on a running turn).
 */
export class MemoryAssistantBackend extends AssistantBackend {
  private readonly listeners = new Set<(e: Envelope) => void>();
  private readonly store = new Map<string, Conversation>();
  private readonly running = new Map<string, { stopped: boolean; decision: ((approved: boolean) => void) | null; call: string | null; halt: (() => void) | null }>();
  private gate: Promise<void> | null = null;
  private open: (() => void) | null = null;
  private counter = 0;
  /** Every request sent, in order. */
  readonly sent: SendRequest[] = [];
  /** Every approval answered: [callId, approved]. */
  readonly approvals: [string, boolean][] = [];
  /** The engine's tool is `erase`; how often it ran. */
  erased = 0;

  constructor(private readonly options: MemoryAssistantOptions = {}) {
    super();
    for (const c of options.conversations ?? []) this.store.set(c.id, c);
  }

  /** Pauses every turn before its first word until `release()`. */
  hold(): void {
    this.gate = new Promise((resolve) => (this.open = resolve));
  }

  release(): void {
    this.open?.();
    this.gate = null;
  }

  /** Delivers an event as the engine would (tests of the store's handling of any event). */
  emit(conversationId: string, event: AssistantEvent): void {
    const seq = ++this.counter;
    for (const l of [...this.listeners]) l({ conversationId, seq, ...event });
  }

  info(): Promise<AssistantInfo> {
    return Promise.resolve(
      this.options.info ?? {
        provider: 'Pitwall (demo)',
        hasWrite: true,
        tools: [
          { name: 'find', description: 'Finds things', write: false },
          { name: 'erase', description: 'Erases things', write: true },
        ],
      },
    );
  }

  conversations(): Promise<ConversationSummary[]> {
    const list = [...this.store.values()].map((c) => ({ id: c.id, title: c.title, updatedMs: c.updatedMs, messageCount: c.messages.length }));
    return Promise.resolve(list.sort((a, b) => b.updatedMs - a.updatedMs));
  }

  load(id: string): Promise<Conversation> {
    const c = this.store.get(id);
    return c ? Promise.resolve(c) : Promise.reject(new Error('That conversation no longer exists.'));
  }

  delete(id: string): Promise<void> {
    return this.store.delete(id) ? Promise.resolve() : Promise.reject(new Error('That conversation no longer exists.'));
  }

  send(request: SendRequest): Promise<SendAck> {
    this.sent.push(request);
    if (this.options.sendError) return Promise.reject(new Error(this.options.sendError));
    const text = request.text.trim();
    let id = request.conversationId ?? null;
    const now = 1_700_000_000_000 + this.counter * 1000;
    const user = { role: 'user' as const, text, context: request.context, atMs: now };
    if (id && this.store.has(id)) {
      const c = this.store.get(id) as Conversation;
      this.store.set(id, { ...c, messages: [...c.messages, user], updatedMs: now });
    } else {
      id = `m${++this.counter}`;
      this.store.set(id, { id, title: text.slice(0, 60), createdMs: now, updatedMs: now, messages: [user] });
    }
    const conversationId = id;
    this.running.set(conversationId, { stopped: false, decision: null, call: null, halt: null });
    void Promise.resolve().then(() => this.run(conversationId, text, request.allowWrite));
    return Promise.resolve({ conversationId });
  }

  stop(id: string): Promise<boolean> {
    const run = this.running.get(id);
    if (!run) return Promise.resolve(false);
    run.stopped = true;
    run.decision?.(false);
    run.halt?.();
    this.release();
    return Promise.resolve(true);
  }

  approve(id: string, callId: string, approved: boolean): Promise<void> {
    const run = this.running.get(id);
    if (!run || run.call !== callId || !run.decision) return Promise.reject(new Error('No action waits for a decision.'));
    this.approvals.push([callId, approved]);
    run.decision(approved);
    return Promise.resolve();
  }

  listen(handler: (event: Envelope) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }

  private async run(id: string, text: string, allowWrite: boolean): Promise<void> {
    const run = this.running.get(id);
    if (!run) return;
    const c = this.store.get(id) as Conversation;
    this.emit(id, { type: 'started', title: c.title });
    if (this.gate) await this.gate;
    let said = '';
    const command = text.split(/\s/, 1)[0] ?? '';
    if (command === '/tool' || command === '/tool-fail' || command === '/tool-slow') {
      const what = text.slice(command.length).trim() || 'everything';
      this.emit(id, { type: 'toolStarted', callId: 'call_1', name: 'find', summary: what, write: false });
      if (command === '/tool-slow') {
        // The card stays "running" until the user presses Stop.
        await new Promise<void>((resolve) => (run.halt = resolve));
        run.halt = null;
      } else if (command === '/tool-fail') {
        this.emit(id, { type: 'toolFinished', callId: 'call_1', name: 'find', ok: false, summary: 'The index is locked by another process.' });
      } else {
        this.emit(id, {
          type: 'toolFinished',
          callId: 'call_1',
          name: 'find',
          ok: true,
          summary: '2 things found',
          data: { citations: [{ text: 'item 7', target: 'record:7', title: 'Item 7' }] },
        });
      }
    } else if (command === '/write' && allowWrite) {
      run.call = 'call_2';
      this.emit(id, { type: 'toolStarted', callId: 'call_2', name: 'erase', summary: 'Erase everything', write: true });
      this.emit(id, { type: 'approvalNeeded', callId: 'call_2', name: 'erase', summary: 'Erase everything', input: {} });
      const approved = await new Promise<boolean>((resolve) => (run.decision = resolve));
      run.decision = null;
      run.call = null;
      if (approved) this.erased++;
      this.emit(id, { type: 'toolFinished', callId: 'call_2', name: 'erase', ok: approved, summary: approved ? 'Erased' : 'Declined by the user' });
    }
    const answer = this.options.reply ? this.options.reply(text) : command === '/tool' ? 'Found item 7 for you.' : DEMO_ANSWER;
    const words = answer.split(/(?<=\s)/);
    for (const word of words) {
      if (run.stopped) break;
      said += word;
      this.emit(id, { type: 'delta', text: word });
      await Promise.resolve();
    }
    if (command === '/fail' && !run.stopped) {
      this.running.delete(id);
      this.emit(id, { type: 'error', kind: 'failed', message: 'The model stopped answering.' });
      return;
    }
    if (command === '/slow' && !run.stopped) {
      // Waits here, mid-answer, until the user presses Stop.
      await new Promise<void>((resolve) => (run.halt = resolve));
      run.halt = null;
    }
    const done = this.store.get(id);
    this.running.delete(id);
    if (done) {
      const end = run.stopped ? ('cancelled' as const) : ('answered' as const);
      this.store.set(id, { ...done, messages: [...done.messages, { role: 'assistant', text: said, end, atMs: done.updatedMs + 1 }] });
    }
    this.emit(id, { type: 'done', text: said, usage: { inputTokens: 10, outputTokens: said.length }, end: run.stopped ? 'cancelled' : 'answered' });
  }
}
