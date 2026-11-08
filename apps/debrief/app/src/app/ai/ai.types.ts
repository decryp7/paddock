/**
 * Wire contract of the local AI assistant (`app/src-tauri/src/ai/**`, `crates/debrief-ai`), camelCase JSON.
 * Commands: `ai_*` (see `ai-backend.ts`). Events: `debrief://ai-status` (`AiStatus`, all windows),
 * `debrief://ai-chat` (`ChatEventEnvelope`, the chat's window).
 * Every command rejects with `{ code, message }` (`AiErrorPayload`).
 */
import { AgentApprovalRequest, HandOff, PauseReason, QuerySource, Segment } from '../browser/browser.types';
import { ExplainNode, InferredGroup, Level, ParsedPreview } from '../core/api.types';

export const AI_STATUS_EVENT = 'debrief://ai-status';
export const AI_CHAT_EVENT = 'debrief://ai-chat';

export type AiErrorCode =
  | 'unavailable'
  | 'noModel'
  /** The active model cannot read images: install its image projector or choose a "Text + images" model. */
  | 'noVision'
  | 'busy'
  | 'cancelled'
  | 'invalid'
  | 'engine'
  /** The research or agent browser failed (not open, a page that did not answer or changed). */
  | 'browser'
  | 'contextOverflow'
  | 'malformed'
  | 'toolLoopLimit'
  | 'network'
  /** The server certificate could not be verified (a TLS-inspecting proxy such as Zscaler). */
  | 'tls'
  /** The proxy asks for a sign-in (HTTP 407). */
  | 'proxyAuth'
  /** The configured download proxy cannot be used. */
  | 'proxy'
  | 'offline'
  | 'timeout'
  /** The server (or a proxy in between) answered with an unexpected HTTP status. */
  | 'httpStatus'
  | 'notFound'
  | 'integrity'
  | 'disk'
  | 'refused'
  | 'io'
  | 'fileExists'
  /** A path the user did not choose in a dialog (imports and exports). */
  | 'notGranted'
  /** A bounded resource ran out (a knowledge file too large to import). */
  | 'limit';

export interface AiErrorPayload {
  code: AiErrorCode;
  message: string;
}

// ---- status (Pitwall is the only engine, ADR 0070) ----

export type EngineState = 'unavailable' | 'noModel' | 'ready';

export interface AiStatus {
  available: boolean;
  state: EngineState;
  reason: string | null;
  model: string | null;
  safeMode: boolean;
  /** The model Pitwall serves for Debrief reads images ("Text + images", ADR 0025). */
  vision: boolean;
  /** The state of the Pitwall connection (ADR 0059, ADR 0070). */
  pitwall: PitwallStatus;
}

export type PitwallFound = 'absent' | 'installed' | 'running' | 'incompatible';

export interface PitwallStatus {
  found: PitwallFound;
  /** Pitwall's version when it is running, e.g. "1.0.4". */
  version: string | null;
  /** Debrief holds a key Pitwall accepted. */
  paired: boolean;
  /** A pairing waits for approval in Pitwall: the six-digit code the user compares. */
  pairing: PitwallPairing | null;
  /** The model Pitwall picked for Debrief. */
  model: string | null;
  /** Ready to serve Debrief (running, compatible, paired, a model fits). */
  usable: boolean;
  /** One plain sentence: why not usable or what happened (pairing denied/expired, key revoked, stopped); null when fine. */
  problem: string | null;
}

export interface PitwallPairing {
  code: string;
  expiresAtMs: number;
}

// ---- proposals and narratives ----

export interface RegexProposal {
  kind: 'regex';
  pattern: string;
  source: 'model' | 'inferred';
  groups: InferredGroup[];
  matches: boolean[];
  matchCount: number;
  total: number;
  explanation: ExplainNode[];
  rationale: string;
}

export interface ParserProposal {
  kind: 'parser';
  name: string;
  toml: string;
  importer: string | null;
  parsed: number;
  withTimestamp: number;
  preview: ParsedPreview[];
  rationale: string;
  /** ADR 0032: the engine's target reached, the best it got to, or the library reads the file already. */
  status: ParserStatus;
  /** The engine's verdict on the final parser; null from older results. */
  verdict: ParserVerdict | null;
  /** Each round of drafting (by the model) and verification (by the engine). */
  rounds: ParserRound[];
  /** Profile [[chart]] sections for the accepted metrics; empty without metrics. */
  chartsToml: string;
}

export type ParserStatus = 'done' | 'bestEffort' | 'builtin';

/** Which rule a line failed (debrief_core::parser::verify::FailedRule). */
export type FailedRule = 'format' | 'missedEntry' | 'timestamp' | 'level';

export interface LineFailure {
  /** 1-based line in the file. */
  line: number;
  fragment: string;
  rule: FailedRule;
  reason: string;
}

export interface TimestampCheck {
  accepted: boolean;
  timed: number;
  events: number;
  backwards: number;
  first: number | null;
  last: number | null;
  plausible: boolean;
  reason: string | null;
  suggestedFormat: string | null;
}

export type ZoneSource = 'timestamps' | 'same-machine' | 'modification-time' | 'text-hint' | 'asked' | 'user';

export interface ZoneGuess {
  offset: string;
  source: ZoneSource;
  confidence: number;
  detail: string;
}

export interface FieldCheck {
  field: string;
  coverage: number;
  distinct: number;
  accepted: boolean;
  reason: string;
}

export interface MetricCheck {
  name: string;
  unit: 'bytes' | 'kb' | 'mb' | 'gb' | 'percent' | 'ms' | 's' | 'count';
  matches: number;
  plausible: number;
  min: number | null;
  max: number | null;
  accepted: boolean;
  reason: string;
}

/** The engine's verdict on a parser (debrief_core::parser::verify::ParserVerdict). */
export interface ParserVerdict {
  score: number;
  lines: number;
  readable: number;
  events: number;
  withTimestamp: number;
  withLevel: number;
  readsLevels: boolean;
  failures: LineFailure[];
  timestamps: TimestampCheck;
  zone: ZoneGuess;
  identity: FieldCheck[];
  metrics: MetricCheck[];
  error: string | null;
}

export interface ParserRound {
  round: number;
  score: number;
  timestampsAccepted: boolean;
  failures: number;
  note: string;
}

export interface PipelineProposal {
  kind: 'pipeline';
  toml: string;
  profileToml: string;
  steps: { index: number; step: string; input: number; output: number }[];
  sampleTotal: number;
  kept: number;
  dropped: number;
  changed: number;
  examples: { before: string; after: string }[];
  rationale: string;
}

export interface MarkersProposal {
  kind: 'markers';
  toml: string;
  profileToml: string;
  markers: { kind: string; label: string; matches: number }[];
  sampled: number;
  rationale: string;
}

export interface ExtractorsProposal {
  kind: 'extractors';
  toml: string;
  profileToml: string;
  extractors: {
    name: string;
    fields: string[];
    matches: number;
    examples: Record<string, unknown>[];
  }[];
  sampled: number;
  rationale: string;
}

export interface QueryProposal {
  kind: 'query';
  ql: string;
  explanation: string;
  estimatedMatches: number | null;
  repaired: boolean;
}

export type Proposal =
  | RegexProposal
  | ParserProposal
  | PipelineProposal
  | MarkersProposal
  | ExtractorsProposal
  | QueryProposal;
export type ProposalKind = Proposal['kind'];
/** The proposals that change the active profile (applied through the profile's validated save). */
export type ProfileChangeKind = 'pipeline' | 'markers' | 'extractors';

export type Citation =
  | { kind: 'record'; id: number; verified: boolean }
  | { kind: 'query'; ql: string }
  | { kind: 'web'; title: string; url: string };

export interface Evidence {
  recordIds: number[];
  /** The query-language queries the tools ran, exactly as run (the chat shows each as an editable chip). */
  queries: string[];
  /** The SQL texts the assistant's `run_sql` fallback ran (shown read-only in the same chip). */
  sql?: string[];
}

export interface Usage {
  inputTokens: number;
  outputTokens: number;
}

export interface Narrative {
  text: string;
  citations: Citation[];
  evidence: Evidence;
  usage: Usage;
}

// ---- chat ----

/** Where the user is, sent with each turn; the engine validates every value and fetches the details itself. */
export interface ChatContext {
  query?: string | null;
  /** Epoch ms. */
  timeRange?: [number, number] | null;
  selectedIds: number[];
  focusedInstance?: string | null;
  openView?: string | null;
}

export type ChatEvent =
  | { type: 'delta'; text: string }
  | { type: 'toolStarted'; name: string; args: string }
  | { type: 'toolFinished'; name: string; summary: string; error: boolean; evidence: Evidence }
  | { type: 'proposal'; proposal: Proposal }
  /** A search or page the model suggests; nothing loads until the user opens it. */
  | { type: 'linkSuggestion'; kind: 'search' | 'link'; url: string; title: string; query: string | null; segments: Segment[] }
  /** An agent step waits for the user's approval. */
  | { type: 'agentApproval'; request: AgentApprovalRequest }
  /** An agent step starts (one model call each). */
  | { type: 'agentStep'; step: number; limit: number; action: string; site: string | null }
  /** Agent mode stopped and handed the browser back. */
  | { type: 'agentPaused'; reason: PauseReason; steps: number; handOff: HandOff | null }
  /** The model offers web research (or the search text needs confirming): "Research this" and agent mode buttons. */
  | { type: 'researchOffer'; question: string; query: string; segments: Segment[]; needsConfirm: boolean }
  | { type: 'done'; text: string; citations: Citation[]; usage: Usage; notes: string[] }
  | { type: 'error'; message: string; cancelled: boolean };

/** A chat event as replayed (`ai_chat_events_since`): numbered from 1 per chat; 0 = outside the stream. */
export type ChatEnvelope = ChatEvent & { seq: number };
/** A chat event as streamed to the window. */
export type ChatEventEnvelope = ChatEnvelope & { sessionId: string };

export interface SessionInfo {
  id: string;
  title: string;
  turns: number;
  running: boolean;
}

export interface TurnRecord {
  user: string;
  assistant: string;
  citations: Citation[];
  proposals: Proposal[];
  web: { title: string; url: string }[];
  cancelled: boolean;
  usage: Usage;
}

export interface ChatRecord {
  id: string;
  title: string;
  turns: TurnRecord[];
}

/** Saved with the project and the session under the UI state key `chats`. */
export interface ChatHistoryRecord {
  version: number;
  sessions: ChatRecord[];
}

export interface RestoreResult {
  sessions: SessionInfo[];
  warning: string | null;
}

/** Where a proposal's sample comes from: text the user gave, the lines around an event, or (parsers) an open source's file. */
export interface SampleSource {
  recordId?: number | null;
  sample?: string | null;
  fileName?: string | null;
  /** A parser is generated from this open source's file (read by Rust, ADR 0032). */
  sourceId?: number | null;
}

/** Explain, summarise, root causes: an answer that runs in the background ("Explanation ready · View"). */
export type NarrativeRequest = { op: 'explain'; recordId: number } | { op: 'summarize'; from: number; to: number } | { op: 'rootCauses' };

/** `ai_start`: a use case as a background AI job (app/src-tauri/src/ai/requests.rs `AiRequest`, tagged by `op`). */
export type AiStartRequest =
  | NarrativeRequest
  | { op: 'regex'; lines: string[]; intent: string | null }
  | { op: 'parser'; recordId: number | null; sample: string | null; fileName: string | null; sourceId?: number | null }
  | { op: 'pipeline' | 'markers' | 'extractors'; goal: string; recordId: number | null; sample: string | null }
  | { op: 'query'; question: string }
  | { op: 'folderSetup'; request: unknown }
  /** "Explain with AI" on an open image; the answer is a narrative. */
  | { op: 'explainImage'; imageId: number; question: string | null }
  /** "Find related logs" for an open image; the answer is a `RelatedOutcome` (job result `relatedLogs`). */
  | { op: 'relatedLogs'; imageId: number };

// ---- images (ADR 0025: app/src-tauri/src/ai/vision.rs, crates/debrief-core/src/related, crates/debrief-ai/src/vision) ----

export type ImageSourceFormat = 'png' | 'jpeg' | 'bmp' | 'gif' | 'webp';

/** An image opened for the AI (`ai_image_open`, `ai_image_paste`): the clean copy Rust re-encoded, never the file. */
export interface AiImage {
  id: number;
  name: string;
  /** The file it came from; null when pasted or dropped. */
  path: string | null;
  width: number;
  height: number;
  originalWidth: number;
  originalHeight: number;
  sourceFormat: ImageSourceFormat;
  /** `data:image/png;base64,…` (or jpeg) of the clean copy. */
  dataUrl: string;
  /** EXIF capture time as the device wrote it, `YYYY-MM-DD HH:MM:SS`. */
  capturedAt: string | null;
}

export type ScreenKind = 'error_dialog' | 'frozen_window' | 'console' | 'dashboard' | 'log_viewer' | 'browser' | 'other';

/** What the model read off a screenshot (`debrief_ai::vision::ScreenFacts`, bounded by the engine). */
export interface ScreenFacts {
  screenKind: ScreenKind;
  windowTitle: string | null;
  application: string | null;
  timestamps: { text: string; hasDate: boolean }[];
  errorTexts: string[];
  errorCodes: string[];
  exceptionNames: string[];
  machineNames: string[];
  userNames: string[];
  serverNames: string[];
  ids: string[];
  versions: string[];
  summary: string;
}

export type ImageTimeSource = 'screen' | 'screenTimeOfDay' | 'exif' | 'fileName' | 'fileModified';

/** When the screenshot was taken, as the engine decided (`debrief_core::related::ImageTime`). */
export interface ImageTime {
  atMs: number;
  precisionMs: number;
  source: ImageTimeSource;
  text: string;
}

export interface RelatedEvent {
  id: number;
  ts: number | null;
  level: Level;
  message: string;
  exception: string | null;
  source: number;
  sourceName: string;
  group: string;
  fields: Record<string, unknown>;
}

export type RelatedTitleTarget =
  | { kind: 'source'; name: string }
  | { kind: 'lane'; lane: string }
  | { kind: 'field'; field: string; value: string };

export type RelatedClueKind = 'exceptionName' | 'errorCode' | 'id' | 'errorText' | 'machine' | 'user' | 'server' | 'version';

/** Why an event is linked to the image (`debrief_core::related::Reason`). */
export type RelatedReason =
  /** `deltaMs` = event time − image time. */
  | { kind: 'time'; deltaMs: number }
  | { kind: 'title'; text: string; target: RelatedTitleTarget }
  | { kind: 'text'; clue: string; clueKind: RelatedClueKind; partial: boolean };

export interface RelatedLink {
  event: RelatedEvent;
  score: number;
  reasons: RelatedReason[];
}

export type RelatedNote =
  | { kind: 'noTime' }
  | { kind: 'outsideData'; from: number | null; to: number | null }
  /** The clocks likely disagree: offer the clock-offset correction of `sourceId`. */
  | { kind: 'clockDisagreement'; deltaMs: number; sourceId: number; sourceName: string }
  | { kind: 'offsetApplied'; sourceName: string; offsetMs: number }
  | { kind: 'nothingToSearch' };

export interface RelatedResult {
  time: ImageTime | null;
  links: RelatedLink[];
  notes: RelatedNote[];
  examined: number;
}

/** The answer of "find related logs" (job result `relatedLogs`): a typed result notes can link to later. */
export interface RelatedOutcome {
  facts: ScreenFacts;
  result: RelatedResult;
  explanation: Narrative;
}

/** `ai_research` arguments (app/src-tauri/src/browser/research.rs `ResearchArgs`). */
export interface ResearchArgs {
  question?: string | null;
  recordId?: number | null;
  sessionId?: string | null;
  /** 1..10; the setting when absent. */
  maxResults?: number | null;
  /** The search text as the user confirmed or edited it. */
  query?: string | null;
  confirmed?: boolean;
}

/** A search text checked by the leak guard. */
export interface CheckedQuery {
  query: string;
  needsConfirm: boolean;
  segments: Segment[];
}

/** `ai_research` answer: the research job started, or the search text needs the user's confirmation first. */
export type ResearchStarted = { status: 'started'; jobId: number } | { status: 'confirm'; checked: CheckedQuery; question: string };

/** `ai_research_draft` arguments: the question (one about the event when absent) and the event. */
export interface DraftArgs {
  question?: string | null;
  recordId?: number | null;
}

/** `ai_research_draft` answer: the question and the search text the model drafted (or the engine built), checked by the
 * leak guard. The user sees and edits it before `ai_research` runs it. */
export interface ResearchDraft {
  question: string;
  checked: CheckedQuery;
  source: QuerySource;
}

/**
 * Knowledge files (ADR 0029; `app/src-tauri/src/ai/knowledge/commands.rs`, `debrief_ai::knowledge`): Markdown notes the AI
 * reads with every request. `project` files are saved with the project (UI state key `knowledge`), `global` ones in
 * Debrief's configuration folder.
 */
export type KnowledgeScope = 'project' | 'global';

/** A knowledge file in a list (no text). */
export interface KnowledgeInfo {
  id: string;
  scope: KnowledgeScope;
  name: string;
  enabled: boolean;
  /** Size of the text in bytes. */
  bytes: number;
}

/** A knowledge file with its Markdown text (`ai_knowledge_get`). */
export interface KnowledgeText extends KnowledgeInfo {
  text: string;
}

/** A change to a file: only the fields given change. */
export interface KnowledgeChange {
  name?: string;
  enabled?: boolean;
  text?: string;
}

/** Both scopes' files, problems reading the global ones, and the engine's limits (`ai_knowledge_list`). */
export interface KnowledgeList {
  project: KnowledgeInfo[];
  global: KnowledgeInfo[];
  warnings: string[];
  /** The global files were saved by a newer Debrief: they cannot be changed here. */
  globalReadOnly: boolean;
  maxFileBytes: number;
  maxFiles: number;
}

/** The project's knowledge as saved in projects and sessions (v1, owned by the engine). */
export interface KnowledgeRecord {
  version: number;
  files: { id: string; name: string; enabled: boolean; text: string }[];
}

/** The project's files after `ai_knowledge_project_restore`, and why saved ones were not loaded. */
export interface KnowledgeRestore {
  files: KnowledgeInfo[];
  warning: string | null;
}
