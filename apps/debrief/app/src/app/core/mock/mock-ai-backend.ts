/**
 * A scripted local-AI backend for the browser build (ng serve, Playwright) and UI tests. It mimics the
 * contract of `app/src-tauri/src/ai` with plausible data: set-up progress on an 8 GB VRAM laptop, streamed
 * chat answers with tool activity, proposals and citations. Never the reference for
 * behaviour: the engine is.
 */
import { AiBackend, AiError, Unlisten } from '../../ai/ai-backend';
import {
  AiErrorCode,
  AiImage,
  AiStatus,
  PitwallStatus,
  ChatContext,
  ChatEnvelope,
  ChatEvent,
  ChatEventEnvelope,
  ChatHistoryRecord,
  ChatRecord,
  Citation,
  Narrative,
  Proposal,
  QueryProposal,
  RestoreResult,
  SampleSource,
  SessionInfo,
  TurnRecord,
} from '../../ai/ai.types';
import { demoRecordIds, mockExtractorsProposal, mockMarkersProposal, mockPipelineProposal, mockParserProposal, mockRegexProposal } from './mock-ai-data';
import { mockAsksForRelated, mockExplainImage, mockFormatOf, mockFormatOfPath, mockImage, mockRelatedOutcome } from './mock-ai-vision';
import { FILE_EXISTS_SUFFIX } from '../api.types';
import { MockClock, systemMockClock } from './mock-clock';
import { mockWeb } from '../../browser/mock-browser-backend';
import { mockBrowserTurn } from './mock-ai-browser';
import { MockJobCancelled, mockJobs, mockSteps } from '../../jobs/mock-jobs';
import { JobLink, JobResult } from '../../jobs/jobs.types';
import { mockDraft, mockLeaks, runMockResearch, startMockResearch, wantsResearch, webCitations } from './mock-ai-research';
import { AiStartRequest, DraftArgs, ResearchArgs, ResearchDraft, ResearchStarted } from '../../ai/ai.types';

/** Operations `mockAiFail` may name: their calls reject with an engine error. */
export const AI_FAILABLE = [
  'chat',
  'nlToQuery',
  'explain',
  'summarize',
  'rootCauses',
  'propose',
  'image',
] as const;
export type AiFailable = (typeof AI_FAILABLE)[number];

export interface MockAiOptions {
  /** `ready` (Pitwall is paired and serves a model, unless `pitwall` says otherwise), `setup` (Pitwall absent unless
   * `pitwall` says otherwise), `off` (no AI in this build/run). */
  mode?: 'ready' | 'setup' | 'off';
  /** Delay between streamed pieces, ms (0 in unit tests). */
  stepMs?: number;
  fail?: AiFailable[];
  /** Image support of the model Pitwall serves (ADR 0025): `on` (default) or `none` (text only). */
  vision?: MockVisionMode;
  /** The clock of every simulated step (default: the page's timers). */
  clock?: MockClock;
  /** Pitwall (the family's local LLM server, ADR 0059): `absent` (default), `installed` (not running), `running` (not
   * paired: connecting shows a code and is approved after a moment), `deny` (as `running`, but the request is denied),
   * `paired` (already usable and the active engine). */
  pitwall?: MockPitwallMode;
}

export type MockPitwallMode = 'absent' | 'installed' | 'running' | 'deny' | 'paired';

const MOCK_PITWALL_MODEL = 'qwen3-8b-q4km';
const MOCK_PITWALL_CODE = '482913';

export type MockVisionMode = 'on' | 'none';

/**
 * Browser-demo switches (never used inside Tauri):
 *   ?mockAi=setup|off        start without Pitwall, or with AI unavailable (hidden)
 *   ?mockAiFail=chat,image   these operations fail
 *   ?mockAiVision=none       the model Pitwall serves reads text only
 *   ?mockPitwall=running|deny|installed|paired  Pitwall found on this computer (see MockAiOptions.pitwall)
 *   ?mockAiStepMs=5          streaming speed
 */
export function mockAiOptionsFromSearch(search: string): MockAiOptions {
  const p = new URLSearchParams(search);
  const o: MockAiOptions = {};
  const mode = p.get('mockAi');
  if (mode === 'setup' || mode === 'off' || mode === 'ready') o.mode = mode;
  const step = p.get('mockAiStepMs');
  if (step !== null && /^\d{1,4}$/.test(step)) o.stepMs = Number(step);
  const fail = (p.get('mockAiFail') ?? '')
    .split(',')
    .filter((f): f is AiFailable => (AI_FAILABLE as readonly string[]).includes(f));
  if (fail.length) o.fail = fail;
  const vision = p.get('mockAiVision');
  if (vision === 'on' || vision === 'none') o.vision = vision;
  const pitwall = p.get('mockPitwall');
  if (pitwall === 'running' || pitwall === 'deny' || pitwall === 'installed' || pitwall === 'paired' || pitwall === 'absent') o.pitwall = pitwall;
  return o;
}

interface MockChat {
  record: ChatRecord;
  events: ChatEnvelope[];
  nextSeq: number;
  running: boolean;
  cancel: boolean;
}

export class MockAiBackend extends AiBackend {
  private readonly mode: 'ready' | 'setup' | 'off';
  private readonly stepMs: number;
  private readonly clock: MockClock;
  private readonly fails: Set<AiFailable>;
  private readonly chats = new Map<string, MockChat>();
  private counter = 0;
  private readonly statusHandlers = new Set<(s: AiStatus) => void>();
  private readonly chatHandlers = new Set<(e: ChatEventEnvelope) => void>();
  private readonly continueHook = (mockWeb().continueChat = (sessionId: string, title: string) => this.chatSend(sessionId, `I opened "${title}" for you: read it and go on with my question.`, {} as ChatContext));
  /** Every chat send, for tests. */
  readonly sent: { sessionId: string; text: string; context: ChatContext; images: number[] }[] = [];
  /** Open images (the engine's per-window cache of clean copies, at most 8). */
  private readonly images = new Map<number, AiImage>();
  private imageCounter = 0;
  private readonly pitwallMode: MockPitwallMode;
  private pitwall: PitwallStatus;
  /** Bumped by cancel/forget so a pairing that was waiting for approval ends quietly. */
  private pairingEpoch = 0;

  constructor(options: MockAiOptions = {}) {
    super();
    this.mode = options.mode ?? 'ready';
    this.stepMs = options.stepMs ?? 25;
    this.clock = options.clock ?? systemMockClock;
    this.fails = new Set(options.fail ?? []);
    this.pitwallMode = options.pitwall ?? (this.mode === 'ready' ? 'paired' : 'absent');
    const found = this.pitwallMode === 'absent' ? 'absent' : this.pitwallMode === 'installed' ? 'installed' : 'running';
    const paired = this.pitwallMode === 'paired';
    this.pitwall = {
      found,
      version: found === 'running' ? '1.0.4' : null,
      paired,
      pairing: null,
      model: paired ? MOCK_PITWALL_MODEL : null,
      usable: paired,
      problem: null,
    };
    this.textOnly = options.vision === 'none';
  }

  private readonly textOnly: boolean;

  /** Pitwall serves a model for Debrief (the only way any AI request can run). */
  private get hasModel(): boolean {
    return this.mode !== 'off' && this.pitwall.usable;
  }

  private check(op?: AiFailable): void {
    if (this.mode === 'off')
      throw new AiError('unavailable', 'The AI engine is not installed in this build of Debrief.');
    if (op && this.fails.has(op))
      throw new AiError('engine', `The model engine failed: simulated ${op} failure.`);
  }

  private needModel(op: AiFailable): void {
    this.check(op);
    if (!this.hasModel)
      throw new AiError('noModel', 'Pitwall is not ready to serve Debrief: connect it in Settings ▸ AI.');
  }

  /** A use case as an AI job (one at a time, visible in the Jobs pane), like the engine; no model fails at once. */
  private aiJob<T>(op: AiFailable, title: string, link: JobLink | null, f: () => T, result: (v: T) => JobResult | null, doneTitle?: string): Promise<T> {
    if (this.mode !== 'ready' || !this.hasModel)
      return this.later(() => {
        this.needModel(op);
        return f();
      }, 0);
    return mockJobs()
      .run({ kind: 'ai', title, link, doneTitle }, async (ctx) => {
        await mockSteps(ctx, this.stepMs * 4, 4);
        this.needModel(op);
        const value = f();
        return { value, result: result(value) };
      })
      .catch((e: unknown) => {
        throw e instanceof MockJobCancelled ? new AiError('cancelled', 'cancelled') : e;
      });
  }

  private async later<T>(f: () => T, ms = this.stepMs * 4): Promise<T> {
    await this.clock.sleep(ms);
    return f();
  }

  private currentStatus(): AiStatus {
    const pitwall = structuredClone(this.pitwall);
    if (this.mode === 'off') {
      return {
        available: false,
        state: 'unavailable',
        reason: 'The AI engine is not available in this run of Debrief.',
        model: null,
        safeMode: false,
        vision: false,
        pitwall,
      };
    }
    const usable = this.pitwall.usable;
    return {
      available: true,
      state: usable ? 'ready' : 'noModel',
      reason: usable ? null : (this.pitwall.problem ?? 'Pitwall is not ready to serve Debrief.'),
      model: usable ? this.pitwall.model : null,
      safeMode: false,
      vision: usable && !this.textOnly,
      pitwall,
    };
  }

  private emitStatus(): void {
    const s = this.currentStatus();
    for (const h of this.statusHandlers) h(s);
  }

  // ---- status and Pitwall (ADR 0059, ADR 0070) ----

  status() {
    return this.later(() => this.currentStatus(), 0);
  }


  async pitwallConnect(): Promise<PitwallStatus> {
    this.check();
    if (this.pitwall.found !== 'running') {
      this.pitwall = { ...this.pitwall, problem: 'Pitwall is not running.' };
      this.emitStatus();
      return structuredClone(this.pitwall);
    }
    if (this.pitwall.paired) {
      this.pitwall = { ...this.pitwall, problem: null };
      this.emitStatus();
      return structuredClone(this.pitwall);
    }
    const epoch = ++this.pairingEpoch;
    this.pitwall = { ...this.pitwall, problem: null, pairing: { code: MOCK_PITWALL_CODE, expiresAtMs: Date.now() + 120_000 } };
    this.emitStatus();
    const waiting = structuredClone(this.pitwall);
    void this.clock.sleep(this.stepMs * 8).then(() => {
      if (epoch !== this.pairingEpoch || !this.pitwall.pairing) return;
      this.pitwall =
        this.pitwallMode === 'deny'
          ? { ...this.pitwall, pairing: null, problem: 'Pitwall denied the request.' }
          : { ...this.pitwall, pairing: null, paired: true, usable: true, model: MOCK_PITWALL_MODEL, problem: null };
      this.emitStatus();
    });
    return waiting;
  }

  async pitwallCancel(): Promise<boolean> {
    const waiting = !!this.pitwall.pairing;
    this.pairingEpoch++;
    this.pitwall = { ...this.pitwall, pairing: null };
    this.emitStatus();
    return waiting;
  }

  async pitwallForget(): Promise<AiStatus> {
    this.pairingEpoch++;
    this.pitwall = { ...this.pitwall, paired: false, usable: false, model: null, pairing: null, problem: null };
    this.emitStatus();
    return this.currentStatus();
  }

  async pitwallRefresh(): Promise<AiStatus> {
    return this.currentStatus();
  }

  // ---- images (ADR 0025) ----

  private addImage(name: string, path: string | null, format: NonNullable<ReturnType<typeof mockFormatOf>>): AiImage {
    const img = mockImage(++this.imageCounter, name, path, format);
    this.images.set(img.id, img);
    // The engine keeps 8 clean copies per window; the oldest goes first.
    for (const id of this.images.keys()) {
      if (this.images.size <= 8) break;
      this.images.delete(id);
    }
    return structuredClone(img);
  }

  imageOpen(path: string) {
    return this.later(() => {
      this.check('image');
      const format = mockFormatOfPath(path);
      if (!format) throw new AiError('invalid', 'Debrief reads PNG, JPEG, BMP, GIF and WebP images');
      return this.addImage(path.split(/[\\/]/).pop() || 'image', path, format);
    });
  }

  imagePaste(data: string, name?: string | null) {
    return this.later(() => {
      this.check('image');
      if (!data) throw new AiError('invalid', 'the image is empty');
      // Base64 of 32 MB (the engine's file limit).
      if (data.length > Math.ceil((32 * 1024 * 1024) / 3) * 4) throw new AiError('invalid', 'the image is larger than 32 MB');
      const format = mockFormatOf(data);
      if (!format) throw new AiError('invalid', 'this is not a PNG, JPEG, BMP, GIF or WebP image');
      return this.addImage((name ?? '').trim() || 'Pasted image', null, format);
    });
  }

  imageClose(id: number) {
    return this.later(() => this.images.delete(id), 0);
  }

  private image(id: number): AiImage {
    const img = this.images.get(id);
    if (!img) throw new AiError('notFound', 'the image is no longer open: open it again');
    return img;
  }

  /** The engine fails an image request of a text-only model (typed `NoVision`). */
  private needVision(): void {
    if (!this.currentStatus().vision)
      throw new AiError('noVision', 'The model Pitwall serves reads text only: choose a model that reads images in Pitwall.');
  }

  // ---- use cases ----

  proposeRegex(lines: string[]) {
    if (!lines.length) return Promise.reject(new AiError('invalid', 'give 1 to 50 example lines'));
    return this.aiJob('propose', 'Suggest a regex', null, () => mockRegexProposal(lines), proposal);
  }
  proposeParser(source: SampleSource) {
    return this.aiJob('propose', 'Generate a parser', eventLink(source), () => mockParserProposal(), proposal);
  }
  proposePipeline(goal: string, source?: SampleSource) {
    return this.aiJob('propose', `Propose pipeline steps: ${goal}`, eventLink(source), () => mockPipelineProposal(goal), proposal);
  }
  proposeMarkers(goal: string, source?: SampleSource) {
    return this.aiJob('propose', `Propose markers: ${goal}`, eventLink(source), () => mockMarkersProposal(goal), proposal);
  }
  proposeExtractors(goal: string, source?: SampleSource) {
    return this.aiJob('propose', `Propose extractors: ${goal}`, eventLink(source), () => mockExtractorsProposal(goal), proposal);
  }
  nlToQuery(question: string) {
    return this.aiJob('nlToQuery', `Write a query: ${question.trim().slice(0, 60)}`, null, (): QueryProposal => {
      const q = question.toLowerCase();
      const ql = q.includes('warn')
        ? 'level = warning'
        : q.includes('web02')
          ? 'MachineName = WEB02 and level >= error'
          : 'level >= error';
      return {
        kind: 'query',
        ql,
        explanation: `Events matching "${question.trim()}": ${ql}.`,
        estimatedMatches: 37,
        repaired: false,
      };
    }, proposal);
  }
  explainEvent(recordId: number) {
    return this.aiJob('explain', `Explain event ${recordId}`, { type: 'event', recordId }, (): Narrative => {
      return {
        text: `Event ${recordId} is a failed order submission: the payment gateway timed out after 30 s.\n\n- **Likely cause:** the gateway pool was exhausted\n- **Next step:** check \`level >= error\` around it`,
        citations: [
          { kind: 'record', id: recordId, verified: true },
          { kind: 'query', ql: 'level >= error' },
        ],
        evidence: { recordIds: [recordId], queries: ['level >= error'] },
        usage: { inputTokens: 900, outputTokens: 80 },
      };
    }, narrative, 'Explanation ready');
  }
  summarize(from: number, to: number) {
    return this.aiJob('summarize', 'Summarize a time range', { type: 'range', from, to }, (): Narrative => {
      const { fatal } = demoRecordIds();
      return {
        text: `Between ${new Date(from).toISOString().slice(11, 16)} and ${new Date(to).toISOString().slice(11, 16)} UTC, WEB02 ran out of memory and crashed (event ${fatal}); it restarted a minute later.`,
        citations: [{ kind: 'record', id: fatal, verified: true }],
        evidence: { recordIds: [fatal], queries: [] },
        usage: { inputTokens: 1200, outputTokens: 60 },
      };
    }, narrative, 'Summary ready');
  }
  rootCauses() {
    return this.aiJob('rootCauses', 'Find likely root causes', null, (): Narrative => {
      const { fatal, error } = demoRecordIds();
      return {
        text: `1. **Memory leak on WEB02** led to an OutOfMemoryException (event ${fatal}).\n2. **Payment gateway timeouts** caused failed orders (event ${error}); see \`level >= error\`.`,
        citations: [
          { kind: 'record', id: fatal, verified: true },
          { kind: 'record', id: error, verified: true },
          { kind: 'query', ql: 'level >= error' },
        ],
        evidence: { recordIds: [fatal, error], queries: ['level >= error'] },
        usage: { inputTokens: 2400, outputTokens: 120 },
      };
    }, narrative, 'Root causes ready');
  }
  /** "Research this": the mock model drafts the search text (it keeps a sample host name when the question has one, so
   * the leak guard has something to flag). */
  async researchDraft(args: DraftArgs): Promise<ResearchDraft> {
    this.needModel('chat');
    const event = args.recordId !== null && args.recordId !== undefined;
    if (event && (args.recordId ?? 0) < 0) throw new AiError('notFound', `there is no event #${args.recordId} in the loaded logs`);
    const question = (args.question ?? '').trim() || (event ? 'What does this error mean and how can it be fixed: System.UnauthorizedAccessException 0x80070005' : '');
    if (!question) throw new AiError('invalid', 'give the question to research');
    const query = event && !args.question ? 'System.UnauthorizedAccessException Access is denied 0x80070005' : mockDraft(question);
    const segments = query.split(/(\bWEB0\d\b)/i).filter((t) => t).map((t) => ({ text: t, leak: mockLeaks(t) ? ('logValue' as const) : null }));
    return { question, checked: { query, needsConfirm: mockLeaks(query), segments }, source: 'model' };
  }

  /** "Research this" / "Research…": a research job whose answer becomes a chat turn, or a search text to confirm. */
  async research(args: ResearchArgs): Promise<ResearchStarted> {
    this.needModel('chat');
    const question = (args.question ?? '').trim() || (args.recordId !== null && args.recordId !== undefined ? 'What does this error mean and how can it be fixed: System.UnauthorizedAccessException 0x80070005' : '');
    if (!question) throw new AiError('invalid', 'give the question to research');
    if (args.sessionId && !this.chats.has(args.sessionId)) throw new AiError('notFound', 'this chat no longer exists');
    const query = (args.query ?? '').trim() || question;
    if (mockLeaks(query) && !args.confirmed) {
      const segments = query.split(/(\bWEB0\d\b)/i).filter((t) => t).map((t) => ({ text: t, leak: mockLeaks(t) ? ('logValue' as const) : null }));
      return { status: 'confirm', question, checked: { query, needsConfirm: true, segments } };
    }
    // Like the engine: 10, 20 or 30 results (the smallest choice at least the one asked for).
    const max = [10, 20, 30].find((c) => c >= (args.maxResults ?? 10)) ?? 30;
    const jobId = startMockResearch(question, max, this.stepMs, args.sessionId ?? null, (r) => this.recordAnswer(args.sessionId ?? null, `Research: ${question}`, r.answer, webCitations(r)));
    return { status: 'started', jobId };
  }

  /** Adds an answer as a turn of the chat (a new chat when it is missing or busy); returns the chat. */
  private recordAnswer(sessionId: string | null, user: string, text: string, citations: Citation[]): string {
    let c = sessionId ? this.chats.get(sessionId) : undefined;
    if (!c || c.running) {
      const id = `chat-${++this.counter}`;
      c = { record: { id, title: '', turns: [] }, events: [], nextSeq: 1, running: false, cancel: false };
      this.chats.set(id, c);
    }
    if (!c.record.title) c.record.title = user.slice(0, 60);
    this.emitChat(c, { type: 'done', text, citations, usage: { inputTokens: 0, outputTokens: 0 }, notes: [] });
    c.record.turns.push({ user, assistant: text, citations, proposals: [], web: citations.flatMap((x) => (x.kind === 'web' ? [{ title: x.title, url: x.url }] : [])), cancelled: false, usage: { inputTokens: 0, outputTokens: 0 } } as TurnRecord);
    return c.record.id;
  }

  /** Every use case as a background AI job: resolves with the job's id at once, or rejects when it cannot start. */
  async start(request: AiStartRequest): Promise<number> {
    const before = mockJobs().nextId();
    const answer: Promise<unknown> = this.useCase(request);
    if (mockJobs().nextId() === before) {
      // No job: it did not start (no model, invalid arguments); say why.
      await answer;
      throw new AiError('engine', 'the request did not start');
    }
    answer.catch(() => undefined);
    return before;
  }

  private useCase(r: AiStartRequest): Promise<unknown> {
    switch (r.op) {
      case 'explain':
        return this.explainEvent(r.recordId);
      case 'summarize':
        return this.summarize(r.from, r.to);
      case 'rootCauses':
        return this.rootCauses();
      case 'regex':
        return this.proposeRegex(r.lines);
      case 'parser':
        return this.proposeParser(r);
      case 'pipeline':
        return this.proposePipeline(r.goal, r);
      case 'markers':
        return this.proposeMarkers(r.goal, r);
      case 'extractors':
        return this.proposeExtractors(r.goal, r);
      case 'query':
        return this.nlToQuery(r.question);
      case 'folderSetup':
        return Promise.reject(new AiError('invalid', 'the folder set-up starts its own AI job'));
      case 'explainImage':
        return this.aiJob('image', 'Explain an image', null, (): Narrative => {
          const img = this.image(r.imageId);
          this.needVision();
          return mockExplainImage(img.name, r.question);
        }, narrative, 'Image explained');
      case 'relatedLogs':
        return this.aiJob('image', 'Find the logs related to an image', null, () => {
          const img = this.image(r.imageId);
          this.needVision();
          return mockRelatedOutcome(img.name);
        }, (o): JobResult => ({ type: 'relatedLogs', result: o }), 'Related logs found');
    }
  }

  /** Stops the window's use cases (running and waiting), not its chat turns. */
  cancel() {
    return this.later(() => mockJobs().cancelWhere((j) => j.kind === 'ai' && j.source.link?.type !== 'chat'), 0);
  }

  // ---- chat ----

  private info(c: MockChat): SessionInfo {
    return {
      id: c.record.id,
      title: c.record.title,
      turns: c.record.turns.length,
      running: c.running,
    };
  }

  private chat(id: string): MockChat {
    const c = this.chats.get(id);
    if (!c) throw new AiError('notFound', 'this chat no longer exists');
    return c;
  }

  chatSessions() {
    return this.later(() => {
      this.check();
      return [...this.chats.values()].map((c) => this.info(c));
    }, 0);
  }
  chatNew() {
    return this.later(() => {
      this.check();
      const id = `chat-${++this.counter}`;
      const c: MockChat = {
        record: { id, title: '', turns: [] },
        events: [],
        nextSeq: 1,
        running: false,
        cancel: false,
      };
      this.chats.set(id, c);
      return this.info(c);
    }, 0);
  }
  chatGet(sessionId: string) {
    return this.later(() => structuredClone(this.chat(sessionId).record), 0);
  }
  chatDelete(sessionId: string) {
    return this.later(() => {
      const c = this.chats.get(sessionId);
      if (c) c.cancel = true;
      mockWeb().forget(sessionId);
      return this.chats.delete(sessionId);
    }, 0);
  }

  /** Starts the turn as an AI job (it may wait behind other AI requests); resolves with the job's id. */
  async chatSend(sessionId: string, text: string, context: ChatContext, images: number[] = []): Promise<number> {
    this.needModel('chat');
    const c = this.chat(sessionId);
    if (c.running)
      throw new AiError('busy', 'This chat is still answering; wait for it or stop it first.');
    if (!text.trim()) throw new AiError('invalid', 'the question is empty');
    if (images.length > 4) throw new AiError('invalid', 'at most 4 images per question');
    const shown = images.map((id) => this.image(id));
    if (shown.length) this.needVision();
    this.sent.push({ sessionId, text, context, images: [...images] });
    c.running = true;
    c.cancel = false;
    const question = text.trim();
    const job = mockJobs().start({ kind: 'ai', title: `Chat: ${question.slice(0, 60)}`, link: { type: 'chat', sessionId } }, async (ctx) => {
      const stop = this.clock.setInterval(() => {
        if (ctx.cancelled()) c.cancel = true;
      }, 20);
      try {
        await this.runTurn(c, question, shown);
      } finally {
        this.clock.clearInterval(stop);
      }
      return { value: undefined, result: { type: 'chat', sessionId } };
    });
    job.done.catch(() => {
      // Cancelled while waiting: the chat is free again and told.
      if (c.running) {
        c.running = false;
        this.emitChat(c, { type: 'error', message: 'Stopped before it started.', cancelled: true });
      }
    });
    return job.id;
  }

  private emitChat(c: MockChat, event: ChatEvent): void {
    const env = { ...event, seq: c.nextSeq++ } as ChatEnvelope;
    c.events.push(env);
    const out = { ...env, sessionId: c.record.id } as ChatEventEnvelope;
    for (const h of this.chatHandlers) h(out);
  }

  private async stream(c: MockChat, text: string): Promise<boolean> {
    for (const piece of text.match(/.{1,12}/gs) ?? []) {
      if (c.cancel) return false;
      this.emitChat(c, { type: 'delta', text: piece });
      await this.clock.sleep(this.stepMs);
    }
    return !c.cancel;
  }

  private async runTurn(c: MockChat, text: string, images: AiImage[] = []): Promise<void> {
    const q = text.toLowerCase();
    const ids = demoRecordIds();
    let answer = '';
    const citations: Citation[] = [];
    const proposals: Proposal[] = [];
    const web: { title: string; url: string }[] = [];
    const tool = async (
      name: string,
      args: string,
      summary: string,
      recordIds: number[] = [],
      queries: string[] = [],
      error = false,
      sql: string[] = [],
    ) => {
      this.emitChat(c, { type: 'toolStarted', name, args });
      await this.clock.sleep(this.stepMs * 2);
      this.emitChat(c, {
        type: 'toolFinished',
        name,
        summary,
        error,
        evidence: { recordIds, queries, sql },
      });
    };
    const finish = (ok: boolean, partial: string) => {
      c.running = false;
      if (!c.record.title) c.record.title = text.slice(0, 60);
      if (ok) {
        this.emitChat(c, {
          type: 'done',
          text: answer,
          citations,
          usage: { inputTokens: 1500, outputTokens: Math.ceil(answer.length / 4) },
          notes: [],
        });
        c.record.turns.push({
          // History never keeps the images (ADR 0025): the question says "[image]".
          user: images.length ? `${images.map(() => '[image]').join(' ')} ${text}` : text,
          assistant: answer,
          citations,
          proposals,
          web,
          cancelled: false,
          usage: { inputTokens: 1500, outputTokens: Math.ceil(answer.length / 4) },
        });
      } else {
        this.emitChat(c, { type: 'error', message: 'cancelled', cancelled: true });
        if (partial.trim())
          c.record.turns.push({
            user: text,
            assistant: partial,
            citations: [],
            proposals: [],
            web: [],
            cancelled: true,
            usage: { inputTokens: 0, outputTokens: 0 },
          } as TurnRecord);
      }
    };
    try {
      await this.clock.sleep(this.stepMs);
      const browsed = images.length ? null : await mockBrowserTurn({
        sessionId: c.record.id,
        question: text,
        web: mockWeb(),
        emit: (e) => this.emitChat(c, e),
        tool: (name, args, summary, error = false) => tool(name, args, summary, [], [], error),
        cancelled: () => c.cancel,
        citations,
        sources: web,
      });
      const first = images[0];
      if (first && mockAsksForRelated(text)) {
        // "Find the logs related to this screenshot": the engine runs the related search as this turn.
        await tool('related_logs', JSON.stringify({ image: first.name }), 'Read the image and searched the logs');
        const { result, explanation } = mockRelatedOutcome(first.name);
        answer = `${explanation.text}\n\nLinked events:\n${result.links.map((l) => `- event ${l.event.id}: ${l.event.message.slice(0, 60)}`).join('\n')}`;
        citations.push(...result.links.map((l) => ({ kind: 'record' as const, id: l.event.id, verified: true })));
      } else if (first) {
        await tool('read_image', JSON.stringify({ images: images.length }), `Read ${images.length} image${images.length === 1 ? '' : 's'}`);
        const n = mockExplainImage(first.name, text);
        answer = n.text;
        citations.push(...n.citations);
      } else if (browsed !== null) {
        answer = browsed;
      } else if (wantsResearch(text) && mockWeb().researchAuto) {
        // Research mode: the model calls web_research; the research runs as a job (Jobs pane) and the turn waits.
        this.emitChat(c, { type: 'toolStarted', name: 'web_research', args: JSON.stringify({ question: text }) });
        const report = await mockJobs().run({ kind: 'research', title: `Research: ${text.slice(0, 60)}`, link: { type: 'chat', sessionId: c.record.id } }, async (ctx) => ({
          value: await runMockResearch(ctx, text, 5, this.stepMs),
          result: { type: 'chat', sessionId: c.record.id } as JobResult,
        }));
        this.emitChat(c, { type: 'toolFinished', name: 'web_research', summary: `read ${report.sources.length} of 5 results`, error: false, evidence: { recordIds: [], queries: [] } });
        answer = report.answer;
        for (const s of report.sources) {
          citations.push({ kind: 'web', title: s.title, url: s.url });
          web.push(s);
        }
      } else if (wantsResearch(text)) {
        // Automatic research is off: the model offers the buttons instead of guessing.
        this.emitChat(c, { type: 'researchOffer', question: text, query: text, segments: [], needsConfirm: false });
        answer = 'This needs information from the web. Use **Research this** below, or let the AI drive the browser for this chat.';
      } else if (q.includes('regex')) {
        await tool(
          'propose_regex',
          '{"example_lines":["…"]}',
          'Regular expression ready to review',
        );
        const p = mockRegexProposal([
          '2026-10-14 09:00:02 [INF] Worker started',
          '2026-10-14 09:00:03 [WRN] Queue depth 812',
        ]);
        proposals.push(p);
        this.emitChat(c, { type: 'proposal', proposal: p });
        answer =
          'I prepared a regular expression for these lines; review it and apply it as a highlight rule.';
      } else if (q.includes('pipeline') || q.includes('drop')) {
        await tool(
          'propose_pipeline',
          '{"goal":"drop health checks"}',
          'Pipeline steps ready to review',
        );
        const p = mockPipelineProposal('drop health checks');
        proposals.push(p);
        this.emitChat(c, { type: 'proposal', proposal: p });
        answer =
          'I prepared a pipeline step that drops health checks. Nothing changes until you apply it.';
      } else if (q.includes('marker')) {
        await tool('propose_markers', '{"goal":"timeouts"}', 'Markers ready to review');
        const p = mockMarkersProposal('timeouts');
        proposals.push(p);
        this.emitChat(c, { type: 'proposal', proposal: p });
        answer = 'I prepared a marker for timeouts.';
      } else if (q.includes('extract')) {
        await tool('propose_extractors', '{"goal":"durations"}', 'Extractors ready to review');
        const p = mockExtractorsProposal('durations');
        proposals.push(p);
        this.emitChat(c, { type: 'proposal', proposal: p });
        answer = 'I prepared an extractor for request durations.';
      } else if (q.includes('query')) {
        await tool('propose_query', '{"question":"errors on WEB02"}', 'Query ready to review');
        const p: QueryProposal = {
          kind: 'query',
          ql: 'MachineName = WEB02 and level >= error',
          explanation: 'Errors and worse on WEB02.',
          estimatedMatches: 14,
          repaired: false,
        };
        proposals.push(p);
        this.emitChat(c, { type: 'proposal', proposal: p });
        answer = 'Here is a query for errors on WEB02: `MachineName = WEB02 and level >= error`.';
        citations.push({ kind: 'query', ql: p.ql });
      } else if (q.includes('percentile')) {
        // The query language cannot say "95th percentile of the gap between events": the fallback is SQL, shown in the chat.
        const sql = 'SELECT quantile_cont(gap, 0.95) AS p95 FROM (SELECT epoch(ts) - epoch(lag(ts) OVER (ORDER BY ts)) AS gap FROM events) WHERE gap IS NOT NULL';
        await tool('run_sql', JSON.stringify({ sql }), 'SQL: 1 rows', [], [], false, [sql]);
        answer = 'I used SQL: the 95th percentile of the gap between events is 17 seconds.';
      } else if (q.includes('graphic builder')) {
        const ql = 'level >= error and ProcessName = "CompanyA.Graphic.Builder.exe"';
        await tool('find_values', '{"text":"graphic builder"}', '1 matching values');
        await tool('run_query', JSON.stringify({ ql }), '2 events', [ids.error, ids.fatal], [ql]);
        answer = `CompanyA.Graphic.Builder.exe logged 2 errors (events ${ids.error} and ${ids.fatal}).`;
        citations.push({ kind: 'record', id: ids.error, verified: true }, { kind: 'record', id: ids.fatal, verified: true });
      } else if (q.includes('unknown') || q.includes('kubernetes')) {
        await tool(
          'run_query',
          '{"ql":"message contains \\"kubernetes\\""}',
          '0 events',
          [],
          ['message contains "kubernetes"'],
        );
        answer = 'Not found in the logs: no event mentions that.';
      } else {
        await tool(
          'run_query',
          '{"ql":"level = fatal"}',
          '1 event',
          [ids.fatal],
          ['level = fatal'],
        );
        answer = `WEB02 crashed with an OutOfMemoryException (event ${ids.fatal}) after its working set grew for an hour.\n\n- Errors before the crash: \`level >= error\`\n- It restarted about a minute later\n\n\`\`\`\nSystem.OutOfMemoryException: Insufficient memory\n\`\`\``;
        citations.push(
          { kind: 'record', id: ids.fatal, verified: true },
          { kind: 'query', ql: 'level >= error' },
        );
      }
      if (c.cancel) return finish(false, '');
      const ok = await this.stream(c, answer);
      finish(ok, ok ? answer : answer.slice(0, 20));
    } catch {
      c.running = false;
      this.emitChat(c, {
        type: 'error',
        message: 'The assistant stopped unexpectedly.',
        cancelled: false,
      });
    }
  }

  chatCancel(sessionId: string) {
    return this.later(() => {
      const c = this.chats.get(sessionId);
      if (!c?.running) return false;
      c.cancel = true;
      mockJobs().cancelWhere((j) => j.state === 'queued' && j.source.link?.type === 'chat' && j.source.link.sessionId === sessionId);
      mockWeb().denyAll(sessionId);
      return true;
    }, 0);
  }
  chatEventsSince(sessionId: string, seq: number) {
    return this.later(() => {
      const c = this.chats.get(sessionId);
      return c ? structuredClone(c.events.filter((e) => e.seq > seq)) : null;
    }, 0);
  }
  chatHistory() {
    return this.later(
      (): ChatHistoryRecord => ({
        version: 1,
        sessions: [...this.chats.values()].map((c) => structuredClone(c.record)),
      }),
      0,
    );
  }
  chatRestore(record: unknown) {
    return this.later((): RestoreResult => {
      this.chats.clear();
      const r = record as ChatHistoryRecord | null | undefined;
      if (r == null) return { sessions: [], warning: null };
      if (typeof r !== 'object' || typeof r.version !== 'number' || !Array.isArray(r.sessions))
        return { sessions: [], warning: 'The saved chats are damaged and were not loaded.' };
      if (r.version > 1)
        return {
          sessions: [],
          warning: 'The saved chats were written by a newer Debrief and were not loaded.',
        };
      for (const s of r.sessions) {
        this.chats.set(s.id, {
          record: structuredClone(s),
          events: [],
          nextSeq: 1,
          running: false,
          cancel: false,
        });
        const n = Number(s.id.replace('chat-', ''));
        if (Number.isFinite(n)) this.counter = Math.max(this.counter, n);
      }
      return { sessions: [...this.chats.values()].map((c) => this.info(c)), warning: null };
    }, 0);
  }

  private readonly written = new Set<string>();
  chatExportMarkdown(sessionId: string, path: string, overwrite = false) {
    return this.later(() => {
      this.chat(sessionId);
      if (this.written.has(path) && !overwrite)
        throw new AiError('fileExists', `${path}${FILE_EXISTS_SUFFIX}`);
      this.written.add(path);
      return path;
    }, 0);
  }
  suggestedPrompts() {
    return this.later(() => {
      this.check();
      return [
        'What happened around 10:42 (OrderService crashed)?',
        'Why is "Payment gateway timeout" happening and what should I do?',
        'Which instance has the most errors?',
        'What should I look at first?',
      ];
    }, 0);
  }
  /** Links "opened" in the system browser (the browser build never navigates away). */
  readonly opened: string[] = [];
  openLink(url: string) {
    return this.later(() => {
      if (!/^https:\/\//.test(url))
        throw new AiError('refused', 'refused: only https links to public sites are opened');
      this.opened.push(url);
    }, 0);
  }
  private subscribe<T>(set: Set<(e: T) => void>, h: (e: T) => void): Promise<Unlisten> {
    set.add(h);
    return Promise.resolve(() => set.delete(h));
  }
  onStatus(handler: (s: AiStatus) => void) {
    return this.subscribe(this.statusHandlers, handler);
  }
  onChat(handler: (e: ChatEventEnvelope) => void) {
    return this.subscribe(this.chatHandlers, handler);
  }
  /** Test hook: the code an operation rejects with. */
  static codeOf(e: unknown): AiErrorCode | null {
    return e instanceof AiError ? e.code : null;
  }
}

/** View of a proposal job opens the proposal to review. */
function proposal(p: unknown): JobResult {
  return { type: 'proposal', proposal: p };
}

/** View of an answer job shows the answer with its citations. */
function narrative(n: unknown): JobResult {
  return { type: 'narrative', narrative: n };
}

function eventLink(source?: SampleSource): JobLink | null {
  return source && 'recordId' in source && typeof source.recordId === 'number' ? { type: 'event', recordId: source.recordId } : null;
}
