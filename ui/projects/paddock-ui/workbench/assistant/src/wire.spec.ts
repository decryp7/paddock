import { describe, expect, it } from 'vitest';
import { FIXTURE_CONVERSATION, FIXTURE_EVENTS } from '../generated/assistant-fixtures';
import { assistantMessage, citationsOf, MAX_CITATIONS, parseContext, parseConversation, parseEnvelope, parseInfo, parseSummaries } from './wire';

describe('the wire', () => {
  it('accepts the engine fixtures (produced by the Rust code itself)', () => {
    for (const e of FIXTURE_EVENTS) expect(parseEnvelope(e)).toEqual(e);
    expect(parseConversation(FIXTURE_CONVERSATION)).toEqual(FIXTURE_CONVERSATION);
  });

  // Regression: the phase events (queued, loading the model, generating) were dropped at this edge, so the real app never showed
  // "Loading the model…" (ER-18); only the memory backend, which skips the parse, did.
  it('keeps the phase events of a cold start', () => {
    const phase = { conversationId: 'c', seq: 2, type: 'phase', phase: { kind: 'loading', elapsedMs: 3000 } };
    expect(parseEnvelope(phase)).toEqual(phase);
  });

  it('reads the context meter and refuses anything else (ADR 0023 WP-4b)', () => {
    const good = { usedTokens: 1200, limitTokens: 7168, reserveTokens: 1024, exact: true, level: 'ok' };
    expect(parseContext(good)).toEqual(good);
    for (const bad of [null, 'x', {}, { ...good, usedTokens: '1' }, { ...good, level: 'huge' }, { ...good, exact: 1 }, { ...good, limitTokens: -1 }]) {
      expect(parseContext(bad)).toBeNull();
    }
  });

  it('refuses events that are not events', () => {
    for (const bad of [null, [], 'x', {}, { conversationId: 'c', seq: 1 }, { conversationId: 'c', seq: 'x', type: 'delta' }, { conversationId: 'c', seq: 1, type: 'teleport' }]) {
      expect(parseEnvelope(bad)).toBeNull();
    }
  });

  it('drops malformed messages and refuses malformed conversations', () => {
    const c = parseConversation({ ...FIXTURE_CONVERSATION, messages: [...FIXTURE_CONVERSATION.messages, { role: 'robot', text: 'x' }, 7, { role: 'user' }] });
    expect(c?.messages).toHaveLength(FIXTURE_CONVERSATION.messages.length);
    for (const bad of [null, {}, { id: 'a', title: 'b' }, { id: 1, title: 'b', messages: [] }]) expect(parseConversation(bad)).toBeNull();
  });

  it('keeps well-formed summaries and info only', () => {
    expect(parseSummaries('x')).toEqual([]);
    const good = { id: 'a', title: 't', updatedMs: 1, messageCount: 2 };
    expect(parseSummaries([good, { id: 'b' }, null])).toEqual([good]);
    expect(parseInfo(null)).toBeNull();
    expect(parseInfo({ provider: 'p', tools: 'x' })).toBeNull();
    expect(parseInfo({ provider: 'P', tools: [{ name: 'a', description: 'd', write: true, destructive: false }, { name: 1 }], hasWrite: false })).toEqual({
      provider: 'P',
      tools: [{ name: 'a', description: 'd', write: true, destructive: false }],
      hasWrite: true,
    });
  });

  it('reads citations from tool data and skips what is malformed or oversized', () => {
    expect(citationsOf(undefined)).toEqual([]);
    expect(citationsOf({ citations: 'x' })).toEqual([]);
    const many = Array.from({ length: MAX_CITATIONS + 5 }, (_, i) => ({ text: `t${i}`, target: `x${i}` }));
    expect(citationsOf({ citations: many })).toHaveLength(MAX_CITATIONS);
    expect(
      citationsOf({
        citations: [{ text: 'a', target: 'b', title: 'T' }, { text: 'a', target: 'b' }, { text: ' ', target: 'b' }, { text: 'x'.repeat(300), target: 'b' }, { text: 'a', target: 'y'.repeat(2000) }, { text: 1, target: 'b' }, null],
      }),
    ).toEqual([{ text: 'a', target: 'b', title: 'T' }, { text: 'a', target: 'b' }]);
  });

  it('words a failure from the engine, an Error, a string or the fallback', () => {
    expect(assistantMessage({ code: 'busy', message: 'Busy.' }, 'f')).toBe('Busy.');
    expect(assistantMessage(new Error('boom'), 'f')).toBe('boom');
    expect(assistantMessage('plain', 'f')).toBe('plain');
    expect(assistantMessage({ message: '' }, 'f')).toBe('f');
    expect(assistantMessage(undefined, 'f')).toBe('f');
  });
});
