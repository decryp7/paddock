import type { AssistantCommand } from '../generated/assistant-commands';
import {
  ASSISTANT_EVENT,
  type Approval,
  type ApprovalChoice,
  type AssistantEvent,
  type AssistantInfo,
  type ContextUse,
  type Conversation,
  type ConversationSummary,
  type Envelope,
  type SendAck,
  type SendRequest,
  type StoredMessage,
} from '../generated/assistant';
import { parseContext, parseConversation, parseEnvelope, parseInfo, parseSummaries } from './wire';

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
  /** Answers the call waiting in the conversation's turn with the user's choice (`alwaysAllow` only when its card offers it). */
  abstract approve(id: string, callId: string, choice: ApprovalChoice): Promise<void>;
  /** Every event until the returned function is called. */
  abstract listen(handler: (event: Envelope) => void): Promise<Unlisten>;
  /**
   * How full the model's context is for a conversation (`null`: a new one), as the engine measures it (ADR 0023 WP-4b, the
   * composer's meter). `null` when this backend does not measure: the pane then shows no meter.
   */
  context(id: string | null): Promise<ContextUse | null> {
    void id;
    return Promise.resolve(null);
  }
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

  async approve(id: string, callId: string, choice: ApprovalChoice): Promise<void> {
    await this.ipc.invoke<unknown>('assistant_approve_tool', { conversationId: id, callId, choice });
  }

  override async context(id: string | null): Promise<ContextUse | null> {
    return parseContext(await this.ipc.invoke<unknown>('assistant_context', { conversationId: id }));
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
  /** The model's window for the context meter (default 8192 tokens; 0: unknown). */
  readonly contextTokens?: number;
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
 * - `/tool-fail`: a `find` call that fails with a reason ("The index is locked by another process."), then a short answer about it.
 * - `/write`: asks for the user's approval of a writing tool (`erase`) and waits for it (needs "Allow actions"); it asks every
 *   call, without "Always allow", as the engine's default policy does.
 * - `/look`: a read-only `find` call that asks first, offering "Always allow" (as Pitwall's policy does); after Always
 *   allow, later `/look` calls run without asking and say they were allowed earlier.
 * - `/cold`: reports the phases of a cold start (queued, loading the model at 0 s and 12 s, generating), then the answer.
 * - `/fail`: streams a few words and then ends the turn with an error event of kind `failed`.
 * - `/slow`: streams its first words and then waits until `stop()` (a running turn with Stop visible).
 * - anything else: just the answer.
 *
 * `hold()` pauses the stream before the first word until `release()` (so a test can press Stop on a running turn).
 */
export class MemoryAssistantBackend extends AssistantBackend {
  private readonly listeners = new Set<(e: Envelope) => void>();
  private readonly store = new Map<string, Conversation>();
  private readonly running = new Map<string, { stopped: boolean; decision: ((choice: ApprovalChoice) => void) | null; call: string | null; remember: boolean; halt: (() => void) | null }>();
  private gate: Promise<void> | null = null;
  private open: (() => void) | null = null;
  private counter = 0;
  /** Every request sent, in order. */
  readonly sent: SendRequest[] = [];
  /** Every approval answered: [callId, choice]. */
  readonly approvals: [string, ApprovalChoice][] = [];
  /** The engine's tool is `erase`; how often it ran. */
  erased = 0;
  /** The user chose Always allow for `find` (the `/look` calls). */
  lookRemembered = false;

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
          { name: 'find', description: 'Finds things', write: false, destructive: false },
          { name: 'erase', description: 'Erases things', write: true, destructive: true },
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
    const user = { role: 'user' as const, text, context: request.context, ...(request.attachments?.length ? { attachments: request.attachments } : {}), atMs: now };
    if (id && this.store.has(id)) {
      const c = this.store.get(id) as Conversation;
      this.store.set(id, { ...c, messages: [...c.messages, user], updatedMs: now });
    } else {
      id = `m${++this.counter}`;
      this.store.set(id, { id, title: text.slice(0, 60), createdMs: now, updatedMs: now, messages: [user] });
    }
    const conversationId = id;
    this.running.set(conversationId, { stopped: false, decision: null, call: null, remember: false, halt: null });
    void Promise.resolve().then(() => this.run(conversationId, text, request.allowWrite));
    return Promise.resolve({ conversationId });
  }

  stop(id: string): Promise<boolean> {
    const run = this.running.get(id);
    if (!run) return Promise.resolve(false);
    run.stopped = true;
    run.decision?.('deny');
    run.halt?.();
    this.release();
    return Promise.resolve(true);
  }

  approve(id: string, callId: string, choice: ApprovalChoice): Promise<void> {
    const run = this.running.get(id);
    if (!run || run.call !== callId || !run.decision) return Promise.reject(new Error('No action waits for a decision.'));
    // As the engine: Always allow only where the card offered it (the write asks every time).
    if (choice === 'alwaysAllow' && !run.remember) return Promise.reject(new Error('This tool asks every time it is used, so it cannot be always allowed.'));
    this.approvals.push([callId, choice]);
    run.decision(choice);
    return Promise.resolve();
  }

  /** A plausible meter for the demo (about four characters a token over the conversation); the engine's rule is Rust's. */
  override context(id: string | null): Promise<ContextUse | null> {
    const limit = this.options.contextTokens ?? 8192;
    const c = id ? this.store.get(id) : undefined;
    const used = 40 + Math.ceil((c?.messages ?? []).reduce((n, m) => n + m.text.length, 0) / 4);
    const level = limit === 0 ? 'unknown' : used >= limit ? 'full' : used * 100 >= limit * 80 ? 'high' : 'ok';
    return Promise.resolve({ usedTokens: used, limitTokens: limit, exact: c !== undefined, level });
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
    let outcome = '';
    // Like the engine, the turn's messages are kept in order and saved when it ends.
    const kept: StoredMessage[] = [];
    const at = (): number => c.updatedMs + kept.length + 1;
    let call: { id: string; summary: string; write: boolean; destructive: boolean } | null = null;
    const keep = (last: StoredMessage): void => {
      const now = this.store.get(id);
      if (now) this.store.set(id, { ...now, messages: [...now.messages, ...kept, last] });
    };
    const startCall = (id_: string, name: string, summary: string, write: boolean, destructive: boolean): void => {
      call = { id: id_, summary, write, destructive };
      this.emit(id, { type: 'toolStarted', callId: id_, name, summary, write, destructive });
      kept.push({ role: 'assistant', text: '', toolCalls: [{ id: id_, name, input: {} }], atMs: at() });
    };
    const finishCall = (name: string, ok: boolean, summary: string, data?: unknown, how?: 'declined' | 'stopped', approval?: Approval): void => {
      const started = call as NonNullable<typeof call>;
      this.emit(id, { type: 'toolFinished', callId: started.id, name, ok, summary, ...(data === undefined ? {} : { data }), ...(how ? { outcome: how } : {}), ...(approval ? { approval } : {}) });
      kept.push({
        role: 'tool',
        text: summary,
        toolCallId: started.id,
        toolName: name,
        ok,
        summary,
        callSummary: started.summary,
        ...(started.write ? { write: true } : {}),
        ...(started.destructive ? { destructive: true } : {}),
        ...(how ? { outcome: how } : {}),
        ...(approval ? { approval } : {}),
        ...(data === undefined ? {} : { data }),
        atMs: at(),
      });
    };
    /** Waits for the user's choice on the running call. */
    const ask = async (callId: string, remember: boolean): Promise<ApprovalChoice> => {
      run.call = callId;
      run.remember = remember;
      const choice = await new Promise<ApprovalChoice>((resolve) => (run.decision = resolve));
      run.decision = null;
      run.call = null;
      run.remember = false;
      return choice;
    };
    const command = text.split(/\s/, 1)[0] ?? '';
    if (command === '/tool' || command === '/tool-fail' || command === '/tool-slow') {
      const what = text.slice(command.length).trim() || 'everything';
      startCall('call_1', 'find', what, false, false);
      if (command === '/tool-slow') {
        // The card stays "running" until the user presses Stop.
        await new Promise<void>((resolve) => (run.halt = resolve));
        run.halt = null;
        // As the engine: the call that was running reports that it was stopped.
        finishCall('find', false, 'Stopped by the user', undefined, 'stopped');
      } else if (command === '/tool-fail') {
        finishCall('find', false, 'The index is locked by another process.');
        outcome = 'I could not look that up: the index is locked by another process. Try again in a moment.';
      } else {
        finishCall('find', true, '2 things found', { citations: [{ text: 'item 7', target: 'record:7', title: 'Item 7' }] }, undefined, 'notAsked');
      }
    } else if (command === '/look') {
      startCall('call_1', 'find', 'Find everything', false, false);
      const choice: ApprovalChoice | 'remembered' = this.lookRemembered ? 'remembered' : await (async () => {
        this.emit(id, { type: 'approvalNeeded', callId: 'call_1', name: 'find', summary: 'Find everything', destructive: false, remember: true, input: { what: 'everything' } });
        return ask('call_1', true);
      })();
      if (choice === 'deny') {
        finishCall('find', false, 'Declined by the user', undefined, run.stopped ? 'stopped' : 'declined');
        outcome = 'I did not look anything up.';
      } else {
        if (choice === 'alwaysAllow') this.lookRemembered = true;
        finishCall('find', true, '2 things found', undefined, undefined, choice === 'remembered' ? 'remembered' : choice === 'alwaysAllow' ? 'always' : 'once');
        outcome = 'Found two things.';
      }
    } else if (command === '/write' && allowWrite) {
      startCall('call_2', 'erase', 'Erase everything', true, true);
      this.emit(id, { type: 'approvalNeeded', callId: 'call_2', name: 'erase', summary: 'Erase everything', destructive: true, remember: false, input: { scope: 'everything', keepBackups: false } });
      const approved = (await ask('call_2', false)) !== 'deny';
      if (approved) this.erased++;
      finishCall('erase', approved, approved ? 'Erased' : 'Declined by the user', undefined, approved ? undefined : run.stopped ? 'stopped' : 'declined', approved ? 'once' : undefined);
      outcome = approved ? 'Done. I erased everything.' : 'I did not erase anything.';
    } else if (command === '/cold') {
      this.emit(id, { type: 'phase', phase: { kind: 'queued' } });
      this.emit(id, { type: 'phase', phase: { kind: 'loading', elapsedMs: 0 } });
      this.emit(id, { type: 'phase', phase: { kind: 'loading', elapsedMs: 12_000 } });
      this.emit(id, { type: 'phase', phase: { kind: 'generating' } });
    }
    const answer = this.options.reply
      ? this.options.reply(text)
      : command === '/tool'
        ? 'Found item 7 for you.'
        : command === '/fail'
          ? 'Let me explain how the request is handled, starting with the router and'
          : outcome || DEMO_ANSWER;
    const words = answer.split(/(?<=\s)/);
    for (const word of words) {
      if (run.stopped) break;
      said += word;
      this.emit(id, { type: 'delta', text: word });
      await Promise.resolve();
    }
    if (command === '/fail' && !run.stopped) {
      this.running.delete(id);
      keep({ role: 'assistant', text: said, end: 'failed', error: { message: 'The model stopped answering.' }, atMs: at() });
      this.emit(id, { type: 'error', kind: 'failed', message: 'The model stopped answering.' });
      return;
    }
    if (command === '/slow' && !run.stopped) {
      // Waits here, mid-answer, until the user presses Stop.
      await new Promise<void>((resolve) => (run.halt = resolve));
      run.halt = null;
    }
    this.running.delete(id);
    keep({ role: 'assistant', text: said, end: run.stopped ? 'cancelled' : 'answered', atMs: at() });
    this.emit(id, { type: 'done', text: said, usage: { inputTokens: 10, outputTokens: said.length }, end: run.stopped ? 'cancelled' : 'answered' });
  }
}
