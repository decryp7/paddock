import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatEventEnvelope } from '../../ai/ai.types';
import { MockJobs, mockJobs, resetMockJobs } from '../../jobs/mock-jobs';
import { MockAiBackend, mockAiOptionsFromSearch } from './mock-ai-backend';
import { demoRecordIds } from './mock-ai-data';
import { mockAsksForRelated, mockCleanCopy, mockFormatOf, mockFormatOfPath, mockRelatedOutcome, mockScenario } from './mock-ai-vision';

const code = (p: Promise<unknown>) =>
  p.then(
    () => null,
    (e) => MockAiBackend.codeOf(e),
  );

const PNG64 = 'iVBORw0KGgoAAAANSUhEUg==';

describe('mock vision data', () => {
  it('reads formats by signature and extension, without decoding', () => {
    expect(mockFormatOf(PNG64)).toBe('png');
    expect(mockFormatOf('/9j/4AAQ')).toBe('jpeg');
    expect(mockFormatOf('R0lGODlh')).toBe('gif');
    expect(mockFormatOf('Qk0=')).toBe('bmp');
    expect(mockFormatOf('UklGRiQ=')).toBe('webp');
    expect(mockFormatOf('PHN2Zz4=')).toBeNull();
    expect(mockFormatOfPath('a.JPG')).toBe('jpeg');
    expect(mockFormatOfPath('a.webp')).toBe('webp');
    expect(mockFormatOfPath('a.svg')).toBeNull();
    expect(mockFormatOfPath('noext')).toBeNull();
    expect(mockCleanCopy('<script>')).toMatch(/^data:image\/svg\+xml;base64,/);
    expect(atob(mockCleanCopy('<script>').split(',')[1]!)).not.toContain('<script>');
  });

  it('builds related outcomes over real demo records, with the scenario notes', () => {
    expect(mockScenario('clock-skew.png')).toBe('clock');
    expect(mockScenario('Console.PNG')).toBe('noTime');
    expect(mockScenario('dialog.png')).toBe('normal');
    const o = mockRelatedOutcome('dialog.png');
    expect(o.result.links[0]!.event.id).toBe(demoRecordIds().fatal);
    expect(o.result.links[0]!.event.level).toBe('Fatal');
    expect(o.result.notes).toEqual([]);
    expect(o.result.time?.source).toBe('screen');
    expect(o.explanation.citations[0]).toEqual({ kind: 'record', id: demoRecordIds().fatal, verified: true });
    const clock = mockRelatedOutcome('clock.png');
    expect(clock.result.notes[0]).toMatchObject({ kind: 'clockDisagreement', deltaMs: -180_000 });
    const none = mockRelatedOutcome('console.png');
    expect(none.result.time).toBeNull();
    expect(none.result.notes).toEqual([{ kind: 'noTime' }]);
    expect(none.result.links.every((l) => l.reasons.every((r) => r.kind !== 'time'))).toBe(true);
    expect(none.facts.screenKind).toBe('console');
  });

  it('recognises the related-logs intent roughly like the engine', () => {
    expect(mockAsksForRelated('/related')).toBe(true);
    expect(mockAsksForRelated('/related please')).toBe(true);
    expect(mockAsksForRelated('find the logs related to this screenshot')).toBe(true);
    expect(mockAsksForRelated('what does this say?')).toBe(false);
  });
});

describe('MockAiBackend images', () => {
  beforeEach(() => resetMockJobs(new MockJobs({ visibleMs: 0, attentionMs: 0, failOnce: new Set() })));
  afterEach(() => resetMockJobs());

  it('reads the vision switch', () => {
    expect(mockAiOptionsFromSearch('?mockAiVision=missing').vision).toBeUndefined();
    expect(mockAiOptionsFromSearch('?mockAiVision=none').vision).toBe('none');
    expect(mockAiOptionsFromSearch('?mockAiVision=on').vision).toBe('on');
    expect(mockAiOptionsFromSearch('?mockAiVision=odd')).toEqual({});
  });

  it('opens, pastes and closes images like the engine, keeping 8', async () => {
    const b = new MockAiBackend({ stepMs: 0 });
    expect((await b.status()).vision).toBe(true);
    const a = await b.imageOpen('C:\\shots\\photo.jpg');
    expect(a).toMatchObject({ name: 'photo.jpg', path: 'C:\\shots\\photo.jpg', sourceFormat: 'jpeg', capturedAt: '2026-10-14 10:41:26' });
    const p = await b.imagePaste(PNG64, null);
    expect(p).toMatchObject({ name: 'Pasted image', path: null, sourceFormat: 'png', capturedAt: null });
    expect(await code(b.imageOpen('/x/a.tiff'))).toBe('invalid');
    expect(await code(b.imagePaste('', 'x'))).toBe('invalid');
    expect(await code(b.imagePaste('PHN2Zz4=', 'x.svg'))).toBe('invalid');
    expect(await code(b.imagePaste(PNG64 + 'A'.repeat(45_000_000), 'big.png'))).toBe('invalid');
    expect(await b.imageClose(a.id)).toBe(true);
    expect(await b.imageClose(a.id)).toBe(false);
    for (let i = 0; i < 9; i++) await b.imagePaste(PNG64, `${i}.png`);
    // The oldest of more than 8 open images is forgotten.
    const evicted = await mockJobs().wait(await b.start({ op: 'relatedLogs', imageId: p.id }));
    expect(evicted.error).toContain('no longer open');
    const fails = new MockAiBackend({ stepMs: 0, fail: ['image'] });
    expect(await code(fails.imageOpen('/x/a.png'))).toBe('engine');
    expect((await new MockAiBackend({ mode: 'off' }).status()).vision).toBe(false);
  });

  it('explains an image and finds its related logs as AI jobs', async () => {
    const b = new MockAiBackend({ stepMs: 0 });
    const img = await b.imagePaste(PNG64, 'dialog.png');
    const explain = await b.start({ op: 'explainImage', imageId: img.id, question: null });
    const ex = await mockJobs().wait(explain);
    expect(ex.state).toBe('done');
    expect(ex.result).toMatchObject({ type: 'narrative' });
    const related = await b.start({ op: 'relatedLogs', imageId: img.id });
    const rel = await mockJobs().wait(related);
    expect(rel.result?.type).toBe('relatedLogs');
    expect((rel.result as { result: { result: { links: unknown[] } } }).result.result.links.length).toBeGreaterThan(0);
    const gone = await b.start({ op: 'relatedLogs', imageId: 999 });
    const end = await mockJobs().wait(gone);
    expect(end).toMatchObject({ state: 'failed', error: 'the image is no longer open: open it again' });
  });

  it('fails image requests of a text-only model with NoVision', async () => {
    const b = new MockAiBackend({ stepMs: 0, vision: 'none' });
    expect((await b.status()).vision).toBe(false);
    const img = await b.imagePaste(PNG64, 'x.png');
    const end = await mockJobs().wait(await b.start({ op: 'explainImage', imageId: img.id, question: 'q' }));
    expect(end.state).toBe('failed');
    expect(end.error).toContain('reads text only');
    const chat = await b.chatNew();
    expect(await code(b.chatSend(chat.id, 'what is it?', { selectedIds: [] }, [img.id]))).toBe('noVision');
    expect(await code(b.chatSend(chat.id, 'no image', { selectedIds: [] }))).toBeNull();
  });

  it('answers chat questions with images, and runs the related search for a related question', async () => {
    const b = new MockAiBackend({ stepMs: 0 });
    const events: ChatEventEnvelope[] = [];
    void b.onChat((e) => events.push(e));
    const chat = await b.chatNew();
    const img = await b.imagePaste(PNG64, 'dialog.png');
    expect(await code(b.chatSend(chat.id, 'x', { selectedIds: [] }, [1, 2, 3, 4, 5]))).toBe('invalid');
    expect(await code(b.chatSend(chat.id, 'x', { selectedIds: [] }, [999]))).toBe('notFound');
    await b.chatSend(chat.id, 'what is this?', { selectedIds: [] }, [img.id]);
    await vi.waitFor(() => expect(events.at(-1)?.type).toBe('done'));
    expect(events.find((e) => e.type === 'toolStarted')).toMatchObject({ name: 'read_image' });
    expect(b.sent.at(-1)?.images).toEqual([img.id]);
    events.length = 0;
    await b.chatSend(chat.id, 'find the logs related to this screenshot', { selectedIds: [] }, [img.id]);
    await vi.waitFor(() => expect(events.at(-1)?.type).toBe('done'));
    const done = events.at(-1) as Extract<ChatEventEnvelope, { type: 'done' }>;
    expect(done.text).toContain('Linked events:');
    expect(done.citations.some((c) => c.kind === 'record' && c.id === demoRecordIds().fatal)).toBe(true);
    const rec = await b.chatGet(chat.id);
    expect(rec.turns.map((t) => t.user)).toEqual(['[image] what is this?', '[image] find the logs related to this screenshot']);
  });
});
