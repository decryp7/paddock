/** Pure view model of a chat: turns built from stored messages and from the events of a running turn (Debrief's `chat-model`). */
import type { AiState } from '@decryp7/paddock-ui/shell/pitwall';
import type { AssistantEvent, Conversation, ContextItem, EndReason, Usage } from '../generated/assistant';
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
}

export function startTurn(user: string, context: readonly ContextItem[] = []): TurnView {
  return { user, context, parts: [], citations: [], notes: [], error: null, end: null, message: null, running: true, usage: null };
}

/** The text of an answer: its text parts joined. */
export function answerText(t: TurnView): string {
  return t.parts.map((p) => (p.kind === 'text' ? p.text : '')).join('');
}

function setCard(t: TurnView, callId: string, change: (c: ToolCard) => ToolCard): TurnView {
  return { ...t, parts: t.parts.map((p) => (p.kind === 'tool' && p.card.callId === callId ? { kind: 'tool', card: change(p.card) } : p)) };
}

/** A turn that ended: nothing runs, and a card still waiting can no longer be answered. */
function ended(t: TurnView): TurnView {
  return {
    ...t,
    running: false,
    parts: t.parts.map((p) => (p.kind === 'tool' && (p.card.state === 'running' || p.card.state === 'awaiting') ? { kind: 'tool', card: { ...p.card, state: 'failed', result: p.card.result ?? null } } : p)),
  };
}

/** Applies one event of the running turn. */
export function reduceTurn(t: TurnView, e: AssistantEvent): TurnView {
  switch (e.type) {
    case 'started':
      return t;
    case 'delta': {
      const last = t.parts.at(-1);
      if (last?.kind === 'text') return { ...t, parts: [...t.parts.slice(0, -1), { kind: 'text', text: last.text + e.text }] };
      return { ...t, parts: [...t.parts, { kind: 'text', text: e.text }] };
    }
    case 'toolStarted': {
      const card: ToolCard = { callId: e.callId, name: e.name, summary: e.summary, write: e.write, destructive: e.destructive, state: 'running', result: null, data: undefined };
      return { ...t, parts: [...t.parts, { kind: 'tool', card }] };
    }
    case 'approvalNeeded':
      return setCard(t, e.callId, (c) => ({ ...c, state: 'awaiting', input: e.input, destructive: e.destructive }));
    case 'toolFinished': {
      const cites = citationsOf(e.data);
      const next = setCard(t, e.callId, (c) => ({ ...c, state: e.ok ? 'ok' : 'failed', result: e.summary, data: e.data }));
      return cites.length ? { ...next, citations: [...next.citations, ...cites.filter((c) => !next.citations.some((k) => k.text === c.text && k.target === c.target))] } : next;
    }
    case 'notice':
      return { ...t, notes: [...t.notes, e.message] };
    case 'done': {
      const finished = ended(t);
      // An answer that arrived whole (no deltas) still shows.
      const parts = finished.parts.some((p) => p.kind === 'text') || !e.text ? finished.parts : [...finished.parts, { kind: 'text' as const, text: e.text }];
      return { ...finished, parts, end: e.end, message: e.message ?? null, usage: e.usage };
    }
    case 'error':
      return { ...ended(t), error: { message: e.message, needsAi: e.needsAi ?? null }, end: 'failed' };
  }
}

/** The turns of a stored conversation. */
export function turnsOf(c: Conversation): TurnView[] {
  const turns: TurnView[] = [];
  const open = (): TurnView | undefined => turns.at(-1);
  const put = (t: TurnView): void => void (turns[turns.length - 1] = t);
  for (const m of c.messages) {
    if (m.role === 'user') {
      turns.push({ ...startTurn(m.text, m.context ?? []), running: false });
      continue;
    }
    const t = open();
    if (!t) continue;
    if (m.role === 'assistant') {
      const parts: TurnPart[] = [...t.parts];
      if (m.text) parts.push({ kind: 'text', text: m.text });
      for (const call of m.toolCalls ?? []) {
        parts.push({ kind: 'tool', card: { callId: call.id, name: call.name, summary: '', write: false, destructive: false, state: 'failed', result: null, data: undefined } });
      }
      put({ ...t, parts, end: m.end ?? t.end });
    } else {
      const done = setCard(t, m.toolCallId ?? '', (card) => ({ ...card, summary: card.summary || (m.summary ?? ''), state: m.ok === false ? 'failed' : 'ok', result: m.summary ?? null, data: m.data }));
      const cites = citationsOf(m.data);
      put(cites.length ? { ...done, citations: [...done.citations, ...cites] } : done);
    }
  }
  return turns;
}
