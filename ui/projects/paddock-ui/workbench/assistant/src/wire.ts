import type { AssistantInfo, ContextUse, Conversation, ConversationSummary, Envelope, StoredMessage, ToolInfo } from '../generated/assistant';

/**
 * Something the answer may link to (a record, a query, a page): the phrase as the model writes it and what activating it
 * does (`target`, the app's string: `record:12`, an https URL). Tools return them in `data.citations`; only cited phrases
 * become buttons (the model cannot invent a link).
 */
export interface AssistantCitation {
  readonly text: string;
  readonly target: string;
  readonly title?: string;
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const isString = (v: unknown): v is string => typeof v === 'string';
const isNumber = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

/** Most citations kept from one tool result, and the longest phrase or target. */
export const MAX_CITATIONS = 50;
const MAX_CITE_TEXT = 200;

/** The citations of a tool result's `data` (`{ citations: [{ text, target, title? }] }`); anything malformed is skipped. */
export function citationsOf(data: unknown): AssistantCitation[] {
  if (!isObject(data) || !Array.isArray(data['citations'])) return [];
  const out: AssistantCitation[] = [];
  for (const c of data['citations'].slice(0, MAX_CITATIONS)) {
    if (!isObject(c) || !isString(c['text']) || !isString(c['target'])) continue;
    if (!c['text'].trim() || c['text'].length > MAX_CITE_TEXT || c['target'].length > MAX_CITE_TEXT * 5) continue;
    out.push(isString(c['title']) ? { text: c['text'], target: c['target'], title: c['title'] } : { text: c['text'], target: c['target'] });
  }
  return out;
}

const EVENT_TYPES = new Set(['started', 'phase', 'delta', 'toolStarted', 'approvalNeeded', 'toolFinished', 'notice', 'done', 'error']);

/** An event as the engine sends it, or `null` when it is not one (parse, don't validate: the edge). */
export function parseEnvelope(raw: unknown): Envelope | null {
  if (!isObject(raw) || !isString(raw['conversationId']) || !isNumber(raw['seq']) || !isString(raw['type']) || !EVENT_TYPES.has(raw['type'])) return null;
  return raw as unknown as Envelope;
}

const ROLES = new Set(['user', 'assistant', 'tool']);

function isMessage(m: unknown): m is StoredMessage {
  return isObject(m) && isString(m['role']) && ROLES.has(m['role']) && isString(m['text']);
}

/** A conversation as the engine sends it, or `null`. */
export function parseConversation(raw: unknown): Conversation | null {
  if (!isObject(raw) || !isString(raw['id']) || !isString(raw['title']) || !Array.isArray(raw['messages'])) return null;
  return { ...(raw as unknown as Conversation), messages: raw['messages'].filter(isMessage) };
}

/** The conversation list; entries that are not summaries are dropped. */
export function parseSummaries(raw: unknown): ConversationSummary[] {
  if (!Array.isArray(raw)) return [];
  return raw.filter((s): s is ConversationSummary => isObject(s) && isString(s['id']) && isString(s['title']) && isNumber(s['updatedMs']) && isNumber(s['messageCount']));
}

/** The assistant's info, or `null`. */
export function parseInfo(raw: unknown): AssistantInfo | null {
  if (!isObject(raw) || !isString(raw['provider']) || !Array.isArray(raw['tools'])) return null;
  const tools = raw['tools'].filter((t): t is ToolInfo => isObject(t) && isString(t['name']) && isString(t['description']) && typeof t['write'] === 'boolean');
  return { provider: raw['provider'], tools, hasWrite: tools.some((t) => t.write) };
}

const LEVELS = new Set(['unknown', 'ok', 'high', 'full']);

/** The context meter as the engine sends it, or `null`. */
export function parseContext(raw: unknown): ContextUse | null {
  if (!isObject(raw) || !isNumber(raw['usedTokens']) || !isNumber(raw['limitTokens']) || !isNumber(raw['reserveTokens']) || typeof raw['exact'] !== 'boolean') return null;
  if (raw['usedTokens'] < 0 || raw['limitTokens'] < 0 || raw['reserveTokens'] < 0 || !isString(raw['level']) || !LEVELS.has(raw['level'])) return null;
  return { usedTokens: raw['usedTokens'], limitTokens: raw['limitTokens'], reserveTokens: raw['reserveTokens'], exact: raw['exact'], level: raw['level'] as ContextUse['level'] };
}

/** The sentence for a failed command: the engine's `CommandError.message`, else the fallback. */
export function assistantMessage(error: unknown, fallback: string): string {
  if (isObject(error) && isString(error['message']) && error['message']) return error['message'];
  if (error instanceof Error && error.message) return error.message;
  return isString(error) && error ? error : fallback;
}
