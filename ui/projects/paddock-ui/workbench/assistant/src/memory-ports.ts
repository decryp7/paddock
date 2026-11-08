import type { Attachment } from '../generated/assistant';
import type { Unlisten } from './backend';
import { AssistantAttachments, AssistantSpeech, type AttachmentChip, type AttachRules, type AttachSource, type SpeechState } from './composer-ports';

/** A plausible SHA-256 for the demo (not a real digest: the memory port keeps nothing). */
function fakeSha(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193) >>> 0;
  return h.toString(16).padStart(8, '0').repeat(8);
}

/**
 * Attachments held in memory, playing a host's engine (browser build, demos, tests): text files up to 256 KiB are taken, an
 * image or a file over the limit is refused in words. Deterministic, no timers. `hold()` keeps every `add` waiting until
 * `release()` (so a test can see "Attaching…").
 */
export class MemoryAssistantAttachments extends AssistantAttachments {
  private gate: Promise<void> | null = null;
  private open: (() => void) | null = null;
  /** Every file handed over, by name. */
  readonly added: string[] = [];
  /** Every file taken off a question, by name. */
  readonly removed: string[] = [];

  constructor(private readonly limits: Partial<AttachRules> = {}) {
    super();
  }

  hold(): void {
    this.gate = new Promise((resolve) => (this.open = resolve));
  }

  release(): void {
    this.open?.();
    this.gate = null;
  }

  rules(): Promise<AttachRules> {
    return Promise.resolve({ maxFileBytes: 256 * 1024, maxCount: 16, accept: '.txt,.md,.rs,.ts,.json,text/*', hint: 'Attach text or code files (UTF-8, up to 256 KiB each)', ...this.limits });
  }

  override remove(attachment: Attachment): Promise<void> {
    this.removed.push(attachment.kind === 'file' ? attachment.name : attachment.sha256);
    return Promise.resolve();
  }

  async add(file: AttachSource): Promise<AttachmentChip> {
    if (this.gate) await this.gate;
    this.added.push(file.name);
    if (file.type.startsWith('image/')) throw new Error(`"${file.name}" is an image. The chat can't attach images yet; attach text or code files.`);
    const bytes = await file.bytes();
    const text = new TextDecoder().decode(bytes);
    const lines = text.split('\n').filter((l, i, all) => l || i < all.length - 1).length;
    const size = bytes.length < 1024 ? `${bytes.length} B` : `${Math.round(bytes.length / 1024)} KB`;
    return { attachment: { kind: 'file', sha256: fakeSha(file.name + text), name: file.name }, label: file.name, detail: `${size} · ${lines} ${lines === 1 ? 'line' : 'lines'}` };
  }
}

/**
 * Push-to-talk held in memory (browser build, demos, tests): `start` records, `tick(ms)` moves the recording's clock (the
 * engine's, never the pane's), `finish` answers with the transcript given. `fail` makes the next start fail with words.
 */
export class MemoryAssistantSpeech extends AssistantSpeech {
  private readonly listeners = new Set<(s: SpeechState) => void>();
  private state: SpeechState = { kind: 'idle' };
  private failure: string | null = null;
  private gate: Promise<void> | null = null;
  private open: (() => void) | null = null;

  constructor(private transcript = 'How busy is this computer right now?') {
    super();
  }

  private set(state: SpeechState): void {
    this.state = state;
    for (const l of [...this.listeners]) l(state);
  }

  /** The current state (tests). */
  get now(): SpeechState {
    return this.state;
  }

  /** The next start fails with this message. */
  fail(message: string): void {
    this.failure = message;
  }

  /** The recording has run `ms` more. */
  tick(ms: number): void {
    if (this.state.kind === 'recording') this.set({ kind: 'recording', elapsedMs: this.state.elapsedMs + ms });
  }

  /** Keeps the next `finish` transcribing until `release()` (so a test or a screenshot can see "Writing down…"). */
  hold(): void {
    this.gate = new Promise((resolve) => (this.open = resolve));
  }

  release(): void {
    this.open?.();
    this.gate = null;
  }

  /** What the next `finish` hears. */
  hear(transcript: string): void {
    this.transcript = transcript;
  }

  start(): Promise<void> {
    if (this.failure) {
      const message = this.failure;
      this.failure = null;
      this.set({ kind: 'failed', message });
      return Promise.reject(new Error(message));
    }
    this.set({ kind: 'recording', elapsedMs: 0 });
    return Promise.resolve();
  }

  async finish(): Promise<string> {
    this.set({ kind: 'transcribing' });
    await (this.gate ?? Promise.resolve());
    this.set({ kind: 'idle' });
    return this.transcript;
  }

  cancel(): Promise<void> {
    this.set({ kind: 'idle' });
    return Promise.resolve();
  }

  listen(handler: (state: SpeechState) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }
}
