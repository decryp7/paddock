/** Pure view model of a chat: turns built from stored messages and from the events of a running turn (Debrief's `chat-model`). */
import type { AiState } from '@decryp7/paddock-ui/shell/pitwall';
import type { Approval, AssistantEvent, Conversation, ContextItem, EndReason, ProviderPhase, Usage } from '../generated/assistant';
import { citationsOf, type AssistantCitation } from './wire';

/** What a tool call looked like, and how it ended. */
export interface ToolCard {
  readonly callId: string;
  readonly name: string;
  /** The call as the user reads it. */
  readonly summary: string;
  readonly write: boolean;
  /** What it changes cannot be undone: its approval is a danger choice with Decline as the default. */
  readonly destructive: boolean;
  /** `awaiting`: a write waits for the user's decision. */
  readonly state: 'running' | 'awaiting' | 'ok' | 'failed';
  /** The card's line once finished. */
  readonly result: string | null;
  /** The structured result, for the app's renderer of this tool (never shown as HTML). */
  readonly data: unknown;
  /** The arguments of the call, known once a write asks for approval (shown in the prompt's details). */
  readonly input?: unknown;
  /** The user declined this call: a choice, not a failure (the card is neutral). */
  readonly declined?: boolean;
  /** The user stopped the turn while this call ran or waited: a choice, not a failure (the card is neutral). */
  readonly stopped?: boolean;
  /** The engine's approval card offers "Always allow" (the policy remembers this tool's answer). */
  readonly remember?: boolean;
  /** How the call was approved, once it ran (the engine's word: not asked, once, always, remembered). */
  readonly approval?: Approval;
}

/** The text of a context chip, in the composer and in the sent question alike: "Current file: src/main.rs". */
export function chipText(label: string, detail: string | null | undefined): string {
  return detail ? `${label}: ${detail}` : label;
}

/** Marks a waiting call as declined by the user (or takes the mark back when the decision could not be sent). */
export function setDeclined(t: TurnView, callId: string, declined: boolean): TurnView {
  return setCard(t, callId, (c) => ({ ...c, declined }));
}

/** The conversation's heading: the title without a leading slash command, in sentence case. */
export function conversationTitle(raw: string): string {
  const trimmed = raw.trim();
  const rest = trimmed.replace(/^\/[^\s]*\s*/, '');
  const base = rest || trimmed.replace(/^\//, '');
  return base.charAt(0).toLocaleUpperCase() + base.slice(1);
}

/** A piece of an answer in order: text, or a tool call between two texts. */
export type TurnPart = { readonly kind: 'text'; readonly text: string } | { readonly kind: 'tool'; readonly card: ToolCard };

export interface TurnView {
  readonly user: string;
  readonly context: readonly ContextItem[];
  readonly parts: readonly TurnPart[];
  readonly citations: readonly AssistantCitation[];
  readonly notes: readonly string[];
  /** A failure of the turn; `needsAi` says Pitwall is the cause (the shared component shows the fix). */
  readonly error: { readonly message: string; readonly needsAi: AiState | null } | null;
  readonly end: EndReason | null;
  /** What happened when a budget ended the turn. */
  readonly message: string | null;
  readonly running: boolean;
  readonly usage: Usage | null;
  /** The last phase the engine reported (queued, loading the model, generating); `null` once words or a tool call came. */
  readonly phase: ProviderPhase | null;
}

export function startTurn(user: string, context: readonly ContextItem[] = []): TurnView {
  return { user, context, parts: [], citations: [], notes: [], error: null, end: null, message: null, running: true, usage: null, phase: null };
}

/** The phase to show while a turn waits for its words (queued or loading the model); `null` when there is none to show. */
export function waitingPhase(t: TurnView): ProviderPhase | null {
  return t.running && t.phase && t.phase.kind !== 'generating' ? t.phase : null;
}

/** The text of an answer: its text parts joined. */
export function answerText(t: TurnView): string {
  return t.parts.map((p) => (p.kind === 'text' ? p.text : '')).join('');
}

function setCard(t: TurnView, callId: string, change: (c: ToolCard) => ToolCard): TurnView {
  return { ...t, parts: t.parts.map((p) => (p.kind === 'tool' && p.card.callId === callId ? { kind: 'tool', card: change(p.card) } : p)) };
}

/** A turn that ended: nothing runs, and a card still waiting can no longer be answered. */
function ended(t: TurnView, byUser = false): TurnView {
  return {
    ...t,
    running: false,
    phase: null,
    parts: t.parts.map((p) =>
      p.kind === 'tool' && (p.card.state === 'running' || p.card.state === 'awaiting')
        ? { kind: 'tool', card: { ...p.card, state: 'failed', result: p.card.result ?? null, ...(byUser ? { stopped: true } : {}) } }
        : p,
    ),
  };
}

/** Applies one event of the running turn. */
export function reduceTurn(t: TurnView, e: AssistantEvent): TurnView {
  switch (e.type) {
    case 'started':
      return t;
    case 'phase':
      return { ...t, phase: e.phase };
    case 'delta': {
      const last = t.parts.at(-1);
      if (last?.kind === 'text') return { ...t, phase: null, parts: [...t.parts.slice(0, -1), { kind: 'text', text: last.text + e.text }] };
      return { ...t, phase: null, parts: [...t.parts, { kind: 'text', text: e.text }] };
    }
    case 'toolStarted': {
      const card: ToolCard = { callId: e.callId, name: e.name, summary: e.summary, write: e.write, destructive: e.destructive, state: 'running', result: null, data: undefined };
      return { ...t, phase: null, parts: [...t.parts, { kind: 'tool', card }] };
    }
    case 'approvalNeeded':
      return setCard(t, e.callId, (c) => ({ ...c, state: 'awaiting', input: e.input, destructive: e.destructive, ...(e.remember ? { remember: true } : {}) }));
    case 'toolFinished': {
      const cites = citationsOf(e.data);
      const next = setCard(t, e.callId, (c) => ({
        ...c,
        state: e.ok ? 'ok' : 'failed',
        result: e.summary,
        data: e.data,
        ...(e.outcome === 'declined' ? { declined: true } : {}),
        ...(e.outcome === 'stopped' ? { stopped: true } : {}),
        ...(e.approval ? { approval: e.approval } : {}),
      }));
      return cites.length ? { ...next, citations: [...next.citations, ...cites.filter((c) => !next.citations.some((k) => k.text === c.text && k.target === c.target))] } : next;
    }
    case 'notice':
      return { ...t, notes: [...t.notes, e.message] };
    case 'done': {
      const finished = ended(t, e.end === 'cancelled');
      // An answer that arrived whole (no deltas) still shows.
      const parts = finished.parts.some((p) => p.kind === 'text') || !e.text ? finished.parts : [...finished.parts, { kind: 'text' as const, text: e.text }];
      return { ...finished, parts, end: e.end, message: e.message ?? null, usage: e.usage };
    }
    case 'error':
      return { ...ended(t), error: { message: e.message, needsAi: e.needsAi ?? null }, end: 'failed' };
  }
}

/** Adds citations a turn does not show yet (the same phrase and target is shown once). */
function withCitations(t: TurnView, cites: readonly AssistantCitation[]): TurnView {
  const fresh = cites.filter((c, i) => !t.citations.some((k) => k.text === c.text && k.target === c.target) && cites.findIndex((k) => k.text === c.text && k.target === c.target) === i);
  return fresh.length ? { ...t, citations: [...t.citations, ...fresh] } : t;
}

/** The turns of a stored conversation: the same views the running turns had (tool cards, citations, how each ended). */
export function turnsOf(c: Conversation): TurnView[] {
  const turns: TurnView[] = [];
  const open = (): TurnView | undefined => turns.at(-1);
  const put = (t: TurnView): void => void (turns[turns.length - 1] = t);
  /** The input of each call, for the details of a write. */
  const inputs = new Map<string, unknown>();
  for (const m of c.messages) {
    if (m.role === 'user') {
      turns.push({ ...startTurn(m.text, m.context ?? []), running: false });
      inputs.clear();
      continue;
    }
    const t = open();
    if (!t) continue;
    if (m.role === 'assistant') {
      const parts: TurnPart[] = [...t.parts];
      if (m.text) parts.push({ kind: 'text', text: m.text });
      for (const call of m.toolCalls ?? []) {
        inputs.set(call.id, call.input);
        parts.push({ kind: 'tool', card: { callId: call.id, name: call.name, summary: '', write: false, destructive: false, state: 'failed', result: null, data: undefined } });
      }
      put({
        ...t,
        parts,
        end: m.end ?? t.end,
        message: m.endMessage ?? t.message,
        error: m.error ? { message: m.error.message, needsAi: m.error.needsAi ?? null } : t.error,
      });
    } else {
      const write = m.write === true;
      const done = setCard(t, m.toolCallId ?? '', (card) => ({
        ...card,
        summary: m.callSummary ?? m.summary ?? '',
        write,
        destructive: m.destructive === true,
        state: m.ok === false ? 'failed' : 'ok',
        result: m.summary ?? null,
        data: m.data,
        ...(write ? { input: inputs.get(card.callId) } : {}),
        ...(m.outcome === 'declined' ? { declined: true } : {}),
        ...(m.outcome === 'stopped' ? { stopped: true } : {}),
        ...(m.approval ? { approval: m.approval } : {}),
      }));
      put(withCitations(done, citationsOf(m.data)));
    }
  }
  // A call that never got a result was never started (the turn ended first): the live view had no card for it either.
  return turns.map((t) => ({ ...t, parts: t.parts.filter((p) => p.kind !== 'tool' || p.card.state !== 'failed' || p.card.result !== null) }));
}
