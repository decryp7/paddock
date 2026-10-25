import { describe, expect, it } from 'vitest';
import type { AssistantEvent } from '../generated/assistant';
import { FIXTURE_CONVERSATION, FIXTURE_EVENTS } from '../generated/assistant-fixtures';
import { answerText, chipText, conversationTitle, reduceTurn, setDeclined, startTurn, turnsOf, type TurnView } from './chat-model';

const run = (events: readonly AssistantEvent[], from: TurnView = startTurn('q')): TurnView => events.reduce(reduceTurn, from);
const usage = { inputTokens: 1, outputTokens: 2 };

describe('reduceTurn', () => {
  it('replays the engine fixture into one finished turn with its tool card and citations-free answer', () => {
    const t = run(FIXTURE_EVENTS.map((e) => e as AssistantEvent), startTurn('Why?'));
    expect(t.running).toBe(false);
    expect(t.end).toBe('answered');
    expect(t.parts.map((p) => p.kind)).toEqual(['tool', 'text']);
    expect(t.parts[0]).toMatchObject({ card: { name: 'find_events', state: 'ok', result: '3 events found', data: { ids: [4, 7, 9] } } });
    expect(answerText(t)).toBe('The build failed at event 7');
  });

  it('joins deltas, shows an answer that came whole, and keeps notes and budget messages', () => {
    let t = run([{ type: 'delta', text: 'a' }, { type: 'delta', text: 'b' }, { type: 'notice', message: 'careful' }]);
    expect(t.parts).toEqual([{ kind: 'text', text: 'ab' }]);
    t = reduceTurn(t, { type: 'done', text: 'ab', usage, end: 'budgetTime', message: 'Out of time.' });
    expect([t.notes, t.message, t.end, t.usage]).toEqual([['careful'], 'Out of time.', 'budgetTime', usage]);
    const whole = run([{ type: 'started', title: 't' }, { type: 'done', text: 'All at once', usage, end: 'answered' }]);
    expect(answerText(whole)).toBe('All at once');
    expect(whole.message).toBeNull();
    expect(answerText(run([{ type: 'done', text: '', usage, end: 'cancelled' }]))).toBe('');
  });

  it('marks a card destructive from its start or from the approval it asks', () => {
    const started = run([{ type: 'toolStarted', callId: 'c1', name: 'erase', summary: 'Erase', write: true, destructive: true }]);
    const asked = run([
      { type: 'toolStarted', callId: 'c1', name: 'erase', summary: 'Erase', write: true, destructive: false },
      { type: 'approvalNeeded', destructive: true, callId: 'c1', name: 'erase', summary: 'Erase', input: {} },
    ]);
    for (const t of [started, asked]) expect(t.parts.map((p) => p.kind === 'tool' && p.card.destructive)).toEqual([true]);
  });

  it('waits for a write, collects citations without duplicates, and closes dangling cards when the turn ends', () => {
    let t = run([
      { type: 'toolStarted', callId: 'c1', name: 'erase', summary: 'Erase', write: true, destructive: false },
      { type: 'approvalNeeded', destructive: false, callId: 'c1', name: 'erase', summary: 'Erase', input: {} },
      { type: 'toolStarted', callId: 'c2', name: 'find', summary: 'x', write: false, destructive: false },
    ]);
    expect(t.parts.map((p) => (p.kind === 'tool' ? p.card.state : 'text'))).toEqual(['awaiting', 'running']);
    const cite = { text: 'item 7', target: 'record:7' };
    t = reduceTurn(t, { type: 'toolFinished', callId: 'c2', name: 'find', ok: true, summary: 'ok', data: { citations: [cite] } });
    t = reduceTurn(t, { type: 'toolFinished', callId: 'zzz', name: 'find', ok: true, summary: 'again', data: { citations: [cite] } });
    expect(t.citations).toEqual([cite]);
    const failed = reduceTurn(t, { type: 'error', kind: 'failed', message: 'Pitwall stopped.' });
    expect(failed.running).toBe(false);
    expect(failed.error).toEqual({ message: 'Pitwall stopped.', needsAi: null });
    expect(failed.parts[0]).toMatchObject({ card: { state: 'failed' } });
    const needs = reduceTurn(startTurn('q'), { type: 'error', kind: 'needsAi', message: 'm', needsAi: { kind: 'notRunning' } });
    expect(needs.error?.needsAi).toEqual({ kind: 'notRunning' });
  });
});

describe('turnsOf', () => {
  it('rebuilds the turns of a stored conversation with their tool cards', () => {
    const turns = turnsOf(FIXTURE_CONVERSATION);
    expect(turns).toHaveLength(1);
    const [t] = turns as [TurnView];
    expect(t.user).toBe('Why did the build fail?');
    expect(t.context[0]?.label).toBe('Selection');
    expect(t.running).toBe(false);
    expect(t.end).toBe('answered');
    expect(t.parts[0]).toMatchObject({ kind: 'tool', card: { callId: 'call_1', name: 'find_events', state: 'ok', result: '3 events found' } });
    expect(answerText(t)).toBe('The build failed at event 7: the linker ran out of memory.');
  });

  it('ignores messages before a question, marks failed and unanswered calls, and takes citations from results', () => {
    const base = { createdMs: 1, updatedMs: 1, id: 'c', title: 't' };
    const turns = turnsOf({
      ...base,
      messages: [
        { role: 'assistant', text: 'orphan', atMs: 0 },
        { role: 'tool', text: 'orphan', atMs: 0 },
        { role: 'user', text: 'q', atMs: 1 },
        { role: 'assistant', text: '', toolCalls: [{ id: 'a', name: 'x', input: {} }, { id: 'b', name: 'y', input: {} }], atMs: 2 },
        { role: 'tool', text: 'r', toolCallId: 'a', ok: false, summary: 'broke', atMs: 3 },
        { role: 'tool', text: 'r', toolCallId: 'b', data: { citations: [{ text: 'k', target: 't' }] }, atMs: 3 },
      ],
    });
    expect(turns).toHaveLength(1);
    expect(turns[0]?.parts.map((p) => (p.kind === 'tool' ? `${p.card.callId}:${p.card.state}` : 'text'))).toEqual(['a:failed', 'b:ok']);
    expect(turns[0]?.citations).toEqual([{ text: 'k', target: 't' }]);
  });
});

describe('conversationTitle', () => {
  it('drops a leading slash command and uses sentence case', () => {
    expect(conversationTitle('/slow Explain the request pipeline')).toBe('Explain the request pipeline');
    expect(conversationTitle('why did the build fail?')).toBe('Why did the build fail?');
    expect(conversationTitle('/slow')).toBe('Slow');
    expect(conversationTitle('  /tool   find it ')).toBe('Find it');
    expect(conversationTitle('')).toBe('');
  });
});

describe('setDeclined', () => {
  it('marks a call declined and keeps the mark when the engine reports it did not run', () => {
    const base = run([{ type: 'toolStarted', callId: 'c1', name: 'erase', summary: 'Erase', write: true, destructive: false }]);
    const declined = reduceTurn(setDeclined(base, 'c1', true), { type: 'toolFinished', callId: 'c1', name: 'erase', ok: false, summary: 'Declined by the user' });
    expect(declined.parts[0]).toMatchObject({ kind: 'tool', card: { state: 'failed', declined: true } });
    expect(setDeclined(setDeclined(base, 'c1', true), 'c1', false).parts[0]).toMatchObject({ card: { declined: false } });
  });
});

describe('a turn stopped by the user', () => {
  it('marks a call that was still running as stopped, not as a failure', () => {
    const base = run([{ type: 'toolStarted', callId: 'c1', name: 'find', summary: 'x', write: false, destructive: false }]);
    const done = reduceTurn(base, { type: 'done', text: '', usage: { inputTokens: 0, outputTokens: 0 }, end: 'cancelled' });
    expect(done.parts[0]).toMatchObject({ card: { state: 'failed', stopped: true } });
    const failed = reduceTurn(base, { type: 'error', kind: 'failed', message: 'x' });
    expect(failed.parts[0]).toMatchObject({ card: { state: 'failed' } });
    expect((failed.parts[0] as { card: { stopped?: boolean } }).card.stopped).toBeUndefined();
  });
});

describe('chipText', () => {
  it('joins the label and the detail the same way everywhere', () => {
    expect(chipText('Current file', 'src/main.rs')).toBe('Current file: src/main.rs');
    expect(chipText('Selection', null)).toBe('Selection');
  });
});
