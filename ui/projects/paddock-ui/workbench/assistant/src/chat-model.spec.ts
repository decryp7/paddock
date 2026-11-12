import { describe, expect, it } from 'vitest';
import type { AssistantEvent, Conversation } from '../generated/assistant';
import { FIXTURE_CONVERSATION, FIXTURE_EVENTS } from '../generated/assistant-fixtures';
import { answerText, chipText, conversationTitle, reduceTurn, setDeclined, startTurn, turnsOf, waitingPhase, type TurnView } from './chat-model';

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
    const started = run([{ type: 'toolStarted', callId: 'c1', name: 'erase', summary: 'Erase', write: true, destructive: true, input: {} }]);
    const asked = run([
      { type: 'toolStarted', callId: 'c1', name: 'erase', summary: 'Erase', write: true, destructive: false, input: {} },
      { type: 'approvalNeeded', destructive: true, remember: false, callId: 'c1', name: 'erase', summary: 'Erase', description: '', input: {} },
    ]);
    for (const t of [started, asked]) expect(t.parts.map((p) => p.kind === 'tool' && p.card.destructive)).toEqual([true]);
  });

  it('waits for a write, collects citations without duplicates, and closes dangling cards when the turn ends', () => {
    let t = run([
      { type: 'toolStarted', callId: 'c1', name: 'erase', summary: 'Erase', write: true, destructive: false, input: {} },
      { type: 'approvalNeeded', destructive: false, remember: false, callId: 'c1', name: 'erase', summary: 'Erase', description: '', input: {} },
      { type: 'toolStarted', callId: 'c2', name: 'find', summary: 'x', write: false, destructive: false, input: {} },
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

describe('phases and approvals (ADR 0023 WP-4a)', () => {
  it('keeps the phase the engine reported until the words or a tool call arrive', () => {
    let t = run([{ type: 'phase', phase: { kind: 'queued' } }]);
    expect(waitingPhase(t)).toEqual({ kind: 'queued' });
    t = reduceTurn(t, { type: 'phase', phase: { kind: 'loading', elapsedMs: 1500 } });
    expect(waitingPhase(t)).toEqual({ kind: 'loading', elapsedMs: 1500 });
    expect(waitingPhase(reduceTurn(t, { type: 'phase', phase: { kind: 'generating' } }))).toBeNull();
    expect(waitingPhase(reduceTurn(t, { type: 'delta', text: 'a' }))).toBeNull();
    expect(waitingPhase(reduceTurn(t, { type: 'toolStarted', callId: 'c1', name: 'find', summary: 'x', write: false, destructive: false, input: {} }))).toBeNull();
    expect(waitingPhase(reduceTurn(t, { type: 'done', text: '', usage, end: 'cancelled' }))).toBeNull();
    expect(waitingPhase(startTurn('q'))).toBeNull();
  });

  it('keeps the arguments, the tool description and the result on the card', () => {
    const t = run([
      { type: 'toolStarted', callId: 'c1', name: 'find', summary: 'x', write: false, destructive: false, input: { q: 'a' } },
      { type: 'approvalNeeded', destructive: false, remember: true, callId: 'c1', name: 'find', summary: 'x', description: 'Finds things.', input: { q: 'a' } },
      { type: 'toolFinished', callId: 'c1', name: 'find', ok: true, summary: 'ok', output: 'three things' },
      { type: 'toolStarted', callId: 'c2', name: 'find', summary: 'y', write: false, destructive: false, input: {} },
      { type: 'toolFinished', callId: 'c2', name: 'find', ok: true, summary: 'ok' },
    ]);
    expect(t.parts.map((p) => (p.kind === 'tool' ? [p.card.input, p.card.description, p.card.output] : null))).toEqual([
      [{ q: 'a' }, 'Finds things.', 'three things'],
      [undefined, undefined, undefined],
    ]);
  });

  it('says on each card how its call was approved, live and when reopened', () => {
    const t = run([
      { type: 'toolStarted', callId: 'c1', name: 'find', summary: 'x', write: false, destructive: false, input: {} },
      { type: 'approvalNeeded', destructive: false, remember: true, callId: 'c1', name: 'find', summary: 'x', description: '', input: {} },
      { type: 'toolFinished', callId: 'c1', name: 'find', ok: true, summary: 'ok', approval: 'always' },
      { type: 'toolStarted', callId: 'c2', name: 'find', summary: 'y', write: false, destructive: false, input: {} },
      { type: 'toolFinished', callId: 'c2', name: 'find', ok: true, summary: 'ok', approval: 'remembered' },
    ]);
    expect(t.parts.map((p) => (p.kind === 'tool' ? [p.card.approval, p.card.remember] : null))).toEqual([
      ['always', true],
      ['remembered', undefined],
    ]);
    const c: Conversation = {
      id: 'x',
      title: 't',
      createdMs: 1,
      updatedMs: 2,
      messages: [
        { role: 'user', text: 'q', atMs: 1 },
        { role: 'assistant', text: '', toolCalls: [{ id: 'c1', name: 'find', input: {} }], atMs: 2 },
        { role: 'tool', text: 'r', toolCallId: 'c1', toolName: 'find', ok: true, summary: 'ok', approval: 'once', atMs: 3 },
      ],
    };
    const [reopened] = turnsOf(c);
    expect(reopened?.parts[0]).toMatchObject({ kind: 'tool', card: { approval: 'once' } });
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
    const base = run([{ type: 'toolStarted', callId: 'c1', name: 'erase', summary: 'Erase', write: true, destructive: false, input: {} }]);
    const declined = reduceTurn(setDeclined(base, 'c1', true), { type: 'toolFinished', callId: 'c1', name: 'erase', ok: false, summary: 'Declined by the user' });
    expect(declined.parts[0]).toMatchObject({ kind: 'tool', card: { state: 'failed', declined: true } });
    expect(setDeclined(setDeclined(base, 'c1', true), 'c1', false).parts[0]).toMatchObject({ card: { declined: false } });
  });
});

describe('a turn stopped by the user', () => {
  it('marks a call that was still running as stopped, not as a failure', () => {
    const base = run([{ type: 'toolStarted', callId: 'c1', name: 'find', summary: 'x', write: false, destructive: false, input: {} }]);
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

describe('turnsOf keeps what the live turn showed', () => {
  const at = { atMs: 1 };
  const conversation = (messages: Conversation['messages']): Conversation => ({ id: 'c', title: 't', createdMs: 1, updatedMs: 1, messages });

  it('restores the call as read, its kind, how the user ended it, the failure and the budget message', () => {
    const [t] = turnsOf(
      conversation([
        { role: 'user', text: 'q', ...at },
        { role: 'assistant', text: '', toolCalls: [{ id: 'c1', name: 'erase', input: { all: true } }, { id: 'c2', name: 'find', input: {} }], ...at },
        { role: 'tool', text: 'no', toolCallId: 'c1', toolName: 'erase', ok: false, summary: 'Declined', callSummary: 'Erase everything', write: true, destructive: true, outcome: 'declined', ...at },
        { role: 'assistant', text: 'half', end: 'failed', error: { message: 'It broke', needsAi: { kind: 'notRunning' } }, endMessage: 'Out of tokens', ...at },
      ]),
    );
    expect(t?.parts).toEqual([
      { kind: 'tool', card: { callId: 'c1', name: 'erase', summary: 'Erase everything', write: true, destructive: true, state: 'failed', result: 'Declined', data: undefined, input: { all: true }, declined: true } },
      { kind: 'text', text: 'half' },
    ]);
    expect(t).toMatchObject({ end: 'failed', error: { message: 'It broke', needsAi: { kind: 'notRunning' } }, message: 'Out of tokens' });
  });

  it('shows a citation once, and falls back to the result line for a file without the call summary', () => {
    const cite = { citations: [{ text: 'item 7', target: 'record:7' }, { text: 'item 7', target: 'record:7' }] };
    const [t] = turnsOf(
      conversation([
        { role: 'user', text: 'q', ...at },
        { role: 'assistant', text: '', toolCalls: [{ id: 'c1', name: 'find', input: {} }], ...at },
        { role: 'tool', text: 'r', toolCallId: 'c1', toolName: 'find', ok: true, summary: '2 found', data: cite, ...at },
        { role: 'assistant', text: 'ok', end: 'answered', ...at },
      ]),
    );
    expect(t?.citations).toEqual([{ text: 'item 7', target: 'record:7' }]);
    expect(t?.parts[0]).toMatchObject({ card: { summary: '2 found', state: 'ok', write: false } });
  });
});
