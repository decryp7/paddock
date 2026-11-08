import { describe, expect, it, vi } from 'vitest';
import { ChatEventEnvelope } from '../../ai/ai.types';
import { AI_FAILABLE, MockAiBackend, mockAiOptionsFromSearch } from './mock-ai-backend';
import { demoRecordIds } from './mock-ai-data';

const code = (p: Promise<unknown>) =>
  p.then(
    () => null,
    (e) => MockAiBackend.codeOf(e),
  );

function chatOf(b: MockAiBackend) {
  const events: ChatEventEnvelope[] = [];
  void b.onChat((e) => events.push(e));
  return events;
}

async function turn(b: MockAiBackend, events: ChatEventEnvelope[], text: string, id: string) {
  await b.chatSend(id, text, { selectedIds: [] });
  await vi.waitFor(() => expect(events.at(-1)?.type).toMatch(/done|error/));
  return events.splice(0);
}

describe('mockAiOptionsFromSearch', () => {
  it('reads the browser switches', () => {
    expect(mockAiOptionsFromSearch('')).toEqual({});
    expect(
      mockAiOptionsFromSearch(
        '?mockAi=setup&mockAiStepMs=5&mockAiFail=chat,bogus,setup',
      ),
    ).toEqual({ mode: 'setup', stepMs: 5, fail: ['chat'] });
    expect(mockAiOptionsFromSearch('?mockAi=weird&mockAiStepMs=x')).toEqual({});
    expect(AI_FAILABLE).toContain('propose');
    expect(mockAiOptionsFromSearch('?mockAiVision=none').vision).toBe('none');
    expect(mockAiOptionsFromSearch('?mockAiVision=missing')).toEqual({});
    expect(mockAiOptionsFromSearch('?mockPitwall=running').pitwall).toBe('running');
  });
});

describe('MockAiBackend', () => {
  it('is unavailable when off', async () => {
    const b = new MockAiBackend({ mode: 'off', stepMs: 0 });
    expect((await b.status()).state).toBe('unavailable');
    const calls = [
      () => b.chatNew(),
      () => b.chatSessions(),
      () => b.suggestedPrompts(),
      () => b.nlToQuery('q'),
    ];
    for (const call of calls) expect(await code(call())).toBe('unavailable');
  });

  it('serves a model once Pitwall is paired, and says so in its status', async () => {
    const b = new MockAiBackend({ mode: 'setup', stepMs: 0, pitwall: 'running' });
    const statuses: string[] = [];
    void b.onStatus((s) => statuses.push(s.state));
    const before = await b.status();
    expect([before.state, before.model, before.vision]).toEqual(['noModel', null, false]);
    expect(await code(b.chatNew())).toBe(null);
    expect(await code(b.nlToQuery('q'))).toBe('noModel');
    await b.pitwallConnect();
    await vi.waitFor(() => expect(statuses.at(-1)).toBe('ready'));
    const after = await b.status();
    expect([after.state, after.model, after.vision]).toEqual(['ready', 'qwen3-8b-q4km', true]);
    expect((await new MockAiBackend({ stepMs: 0, vision: 'none' }).status()).vision).toBe(false);
    const failing = new MockAiBackend({ mode: 'setup', stepMs: 0, pitwall: 'absent' });
    expect((await failing.pitwallConnect()).problem).toBe('Pitwall is not running.');
  });

  it('opens only https links', async () => {
    const b = new MockAiBackend({ stepMs: 0 });
    expect(await code(b.openLink('http://x'))).toBe('refused');
    await b.openLink('https://x');
    expect(b.opened).toEqual(['https://x']);
    const unsub = await b.onStatus(() => undefined);
    unsub();
  });

  it('answers the use cases', async () => {
    const b = new MockAiBackend({ stepMs: 0 });
    expect((await b.proposeRegex(['2026-10-14 09:00:02 [INF] x'])).matchCount).toBe(1);
    expect(await code(b.proposeRegex([]))).toBe('invalid');
    expect((await b.proposeParser({})).kind).toBe('parser');
    expect((await b.proposePipeline('g')).rationale).toContain('"g"');
    expect((await b.proposeMarkers('g')).markers).toHaveLength(1);
    expect((await b.proposeExtractors('g')).extractors).toHaveLength(1);
    expect((await b.nlToQuery('show warnings')).ql).toBe('level = warning');
    expect((await b.nlToQuery('errors on web02')).ql).toContain('WEB02');
    expect((await b.nlToQuery('errors')).ql).toBe('level >= error');
    expect((await b.explainEvent(5)).citations[0]).toEqual({
      kind: 'record',
      id: 5,
      verified: true,
    });
    expect((await b.summarize(0, 60_000)).text).toContain('00:00');
    expect((await b.rootCauses()).citations).toHaveLength(3);
    expect(await b.cancel()).toBe(0);
    const ids = demoRecordIds();
    expect(demoRecordIds()).toBe(ids);
    const none = new MockAiBackend({ mode: 'setup', stepMs: 0 });
    expect(await code(none.explainEvent(1))).toBe('noModel');
    const failing = new MockAiBackend({
      stepMs: 0,
      fail: ['propose', 'nlToQuery', 'explain', 'summarize', 'rootCauses'],
    });
    for (const p of [
      failing.proposeParser({}),
      failing.nlToQuery('q'),
      failing.explainEvent(1),
      failing.summarize(0, 1),
      failing.rootCauses(),
    ])
      expect(await code(p)).toBe('engine');
  });

  it('streams the scripted chat answers', async () => {
    const b = new MockAiBackend({ stepMs: 0 });
    const events = chatOf(b);
    expect(await code(b.chatGet('x'))).toBe('notFound');
    const { id } = await b.chatNew();
    expect(await code(b.chatSend(id, '  ', { selectedIds: [] }))).toBe('invalid');
    expect(await code(b.chatSend('x', 'q', { selectedIds: [] }))).toBe('notFound');
    const types = (ev: ChatEventEnvelope[]) => ev.map((e) => e.type);
    let ev = await turn(b, events, 'what crashed?', id);
    expect(types(ev).slice(0, 2)).toEqual(['toolStarted', 'toolFinished']);
    expect(ev.at(-1)?.type).toBe('done');
    for (const [q, kind] of [
      ['write a regex', 'regex'],
      ['drop these with a pipeline', 'pipeline'],
      ['add a marker', 'markers'],
      ['extract the duration', 'extractors'],
      ['give me a query', 'query'],
    ] as [string, string][]) {
      ev = await turn(b, events, q, id);
      const p = ev.find((e) => e.type === 'proposal');
      expect(p && p.type === 'proposal' ? p.proposal.kind : null).toBe(kind);
    }
    ev = await turn(b, events, 'anything about kubernetes?', id);
    expect(ev.at(-1)).toMatchObject({
      type: 'done',
      text: expect.stringContaining('Not found in the logs'),
    });
    expect(b.sent).toHaveLength(7);
    expect((await b.chatSessions())[0]).toMatchObject({
      id,
      turns: 7,
      running: false,
      title: 'what crashed?',
    });
    expect((await b.chatEventsSince(id, 0))!.length).toBeGreaterThan(10);
    expect(await b.chatEventsSince('none', 0)).toBeNull();
    expect((await b.chatGet(id)).turns).toHaveLength(7);
    expect(
      await code(
        new MockAiBackend({ stepMs: 0, fail: ['chat'] }).chatSend(id, 'q', { selectedIds: [] }),
      ),
    ).toBe('engine');
  });

  it('cancels while streaming and refuses a second turn', async () => {
    const b = new MockAiBackend({ stepMs: 5 });
    const events = chatOf(b);
    const { id } = await b.chatNew();
    await b.chatSend(id, 'what crashed?', { selectedIds: [] });
    expect(await code(b.chatSend(id, 'again', { selectedIds: [] }))).toBe('busy');
    await vi.waitFor(() => expect(events.some((e) => e.type === 'delta')).toBe(true));
    expect(await b.chatCancel(id)).toBe(true);
    await vi.waitFor(() => expect(events.at(-1)).toMatchObject({ type: 'error', cancelled: true }));
    expect((await b.chatGet(id)).turns[0]?.cancelled).toBe(true);
    expect(await b.chatCancel(id)).toBe(false);
    // Cancelled before anything streamed.
    await b.chatSend(id, 'write a regex', { selectedIds: [] });
    await b.chatCancel(id);
    await vi.waitFor(() =>
      expect((async () => (await b.chatSessions())[0]?.running)()).resolves.toBe(false),
    );
  });

  it('saves, restores and exports chats', async () => {
    const b = new MockAiBackend({ stepMs: 0 });
    const events = chatOf(b);
    const { id } = await b.chatNew();
    await turn(b, events, 'q', id);
    const h = await b.chatHistory();
    expect(h.sessions).toHaveLength(1);
    expect(await b.chatExportMarkdown(id, '/a.md')).toBe('/a.md');
    expect(await code(b.chatExportMarkdown(id, '/a.md'))).toBe('fileExists');
    expect(await b.chatExportMarkdown(id, '/a.md', true)).toBe('/a.md');
    expect(await code(b.chatExportMarkdown('none', '/b.md'))).toBe('notFound');
    const fresh = new MockAiBackend({ stepMs: 0 });
    expect(
      (
        await fresh.chatRestore({
          ...h,
          sessions: [...h.sessions, { ...h.sessions[0], id: 'custom' }],
        })
      ).sessions.map((s) => s.id),
    ).toEqual([id, 'custom']);
    expect((await fresh.chatNew()).id).toBe('chat-2');
    expect(await fresh.chatRestore(null)).toEqual({ sessions: [], warning: null });
    expect((await fresh.chatRestore('junk')).warning).toContain('damaged');
    expect((await fresh.chatRestore({ version: 2, sessions: [] })).warning).toContain('newer');
    expect((await fresh.suggestedPrompts()).length).toBeGreaterThan(2);
  });
});
