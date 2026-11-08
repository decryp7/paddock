import { invoke } from '../core/activity/ipc';
import { listenHere } from '../windowing/window-events';
import { AiBackend, toAiError, Unlisten } from './ai-backend';
import {
  AI_CHAT_EVENT,
  AI_STATUS_EVENT,
  AiImage,
  AiStatus,
  PitwallStatus,
  ChatContext,
  ChatEnvelope,
  ChatEventEnvelope,
  ChatHistoryRecord,
  ChatRecord,
  AiStartRequest,
  RestoreResult,
  SessionInfo,
  DraftArgs,
  ResearchArgs,
  ResearchDraft,
  ResearchStarted,
} from './ai.types';

/** Subscribes to an event with its payload; tests pass fakes. */
export type ListenFn = (event: string, handler: (payload: unknown) => void) => Promise<Unlisten>;

const isObject = (v: unknown): v is Record<string, unknown> =>
  !!v && typeof v === 'object' && !Array.isArray(v);

/** Talks to the Rust shell; `call` is Tauri's `invoke` and `listen` the window-scoped listener (tests pass fakes). */
export class TauriAiBackend extends AiBackend {
  constructor(
    private readonly call: typeof invoke = invoke,
    private readonly listen: ListenFn = listenHere,
  ) {
    super();
  }

  private async run<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await this.call<T>(cmd, args);
    } catch (e) {
      throw toAiError(e);
    }
  }

  status() {
    return this.run<AiStatus>('ai_status');
  }
  imageOpen(path: string) {
    return this.run<AiImage>('ai_image_open', { path });
  }
  imagePaste(data: string, name?: string | null) {
    return this.run<AiImage>('ai_image_paste', { data, name: name ?? null });
  }
  imageClose(id: number) {
    return this.run<boolean>('ai_image_close', { id });
  }
  pitwallConnect() {
    return this.run<PitwallStatus>('ai_pitwall_connect');
  }
  pitwallCancel() {
    return this.run<boolean>('ai_pitwall_cancel');
  }
  pitwallForget() {
    return this.run<AiStatus>('ai_pitwall_forget');
  }
  pitwallRefresh() {
    return this.run<AiStatus>('ai_pitwall_refresh');
  }
  start(request: AiStartRequest) {
    return this.run<number>('ai_start', { request });
  }
  researchDraft(args: DraftArgs) {
    return this.run<ResearchDraft>('ai_research_draft', { args });
  }
  research(args: ResearchArgs) {
    return this.run<ResearchStarted>('ai_research', { args });
  }
  cancel() {
    return this.run<number>('ai_cancel');
  }
  chatSessions() {
    return this.run<SessionInfo[]>('ai_chat_sessions');
  }
  chatNew() {
    return this.run<SessionInfo>('ai_chat_new');
  }
  chatGet(sessionId: string) {
    return this.run<ChatRecord>('ai_chat_get', { sessionId });
  }
  chatDelete(sessionId: string) {
    return this.run<boolean>('ai_chat_delete', { sessionId });
  }
  chatSend(sessionId: string, text: string, context: ChatContext, images?: number[]) {
    return this.run<number>('ai_chat_send', { sessionId, text, context, images: images?.length ? images : null });
  }
  chatCancel(sessionId: string) {
    return this.run<boolean>('ai_chat_cancel', { sessionId });
  }
  chatEventsSince(sessionId: string, seq: number) {
    return this.run<ChatEnvelope[] | null>('ai_chat_events_since', { sessionId, seq });
  }
  chatHistory() {
    return this.run<ChatHistoryRecord>('ai_chat_history');
  }
  chatRestore(record: unknown) {
    return this.run<RestoreResult>('ai_chat_restore', { record: record ?? null });
  }
  chatExportMarkdown(sessionId: string, path: string, overwrite = false) {
    return this.run<string>('ai_chat_export_markdown', {
      sessionId,
      path,
      ...(overwrite ? { overwrite } : {}),
    });
  }
  suggestedPrompts() {
    return this.run<string[]>('ai_suggested_prompts');
  }
  openLink(url: string) {
    return this.run<void>('ai_open_link', { url });
  }
  onStatus(handler: (s: AiStatus) => void) {
    return this.listen(AI_STATUS_EVENT, (p) => {
      if (isObject(p) && typeof p['state'] === 'string' && typeof p['available'] === 'boolean')
        handler(p as unknown as AiStatus);
    });
  }
  onChat(handler: (e: ChatEventEnvelope) => void) {
    return this.listen(AI_CHAT_EVENT, (p) => {
      if (
        isObject(p) &&
        typeof p['sessionId'] === 'string' &&
        typeof p['type'] === 'string' &&
        typeof p['seq'] === 'number'
      )
        handler(p as unknown as ChatEventEnvelope);
    });
  }
}
