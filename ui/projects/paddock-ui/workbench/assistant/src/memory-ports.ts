import { signal } from '@angular/core';
import type { Attachment } from '../generated/assistant';
import type { Unlisten } from './backend';
import { AssistantAttachments, AssistantSpeech, type AttachmentChip, type ImageAction, type AttachRules, type AttachSource, type SpeechState } from './composer-ports';

/** A plausible SHA-256 for the demo (not a real digest: the memory port keeps nothing). */
function fakeSha(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193) >>> 0;
  return h.toString(16).padStart(8, '0').repeat(8);
}

/** A 1 x 1 PNG, the memory port's thumbnail of every picture. */
const TINY_PICTURE = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGD4DwABBAEAwS2OUAAAAABJRU5ErkJggg==';

/**
 * Attachments held in memory, playing a host's engine (browser build, demos, tests): text files up to 256 KiB and pictures
 * are taken, a file over the limit is refused in words. `blockImages(words)` plays a model that cannot read pictures: they are
 * refused with those words and a picture already waiting cannot be sent. Deterministic, no timers. `hold()` keeps every `add`
 * waiting until `release()` (so a test can see "Attaching…").
 */
export class MemoryAssistantAttachments extends AssistantAttachments {
  private gate: Promise<void> | null = null;
  private open: (() => void) | null = null;
  /** Every file handed over, by name. */
  readonly added: string[] = [];
  /** Every file taken off a question, by name. */
  readonly removed: string[] = [];
  private readonly block = signal<string | null>(null);
  override readonly imageBlock = this.block.asReadonly();
  private readonly action = signal<ImageAction | null>(null);
  override readonly imageAction = this.action.asReadonly();
  /** The thumbnails of the pictures taken, by digest. */
  private readonly thumbs = new Map<string, string>();
  /** The size of each file taken, by its digest (the memory engine's size source; files are at most 256 KiB, 16 a question). */
  private readonly sizes = new Map<string, number>();

  constructor(private readonly limits: Partial<AttachRules> = {}) {
    super();
  }

  /** Plays a model that cannot read images (`null`: one that can), with the button the host offers beside its words (or none). */
  blockImages(words: string | null, action: ImageAction | null = null): void {
    this.block.set(words);
    this.action.set(action);
  }

  hold(): void {
    this.gate = new Promise((resolve) => (this.open = resolve));
  }

  release(): void {
    this.open?.();
    this.gate = null;
  }

  rules(): Promise<AttachRules> {
    return Promise.resolve({
      maxFileBytes: 256 * 1024,
      maxImageBytes: 25 * 1024 * 1024,
      maxCount: 16,
      accept: 'image/png,image/jpeg,image/webp,image/gif,.txt,.md,.rs,.ts,.json,text/*',
      hint: 'Attach images or text and code files (UTF-8, up to 256 KiB each)',
      ...this.limits,
    });
  }

  override remove(attachment: Attachment): Promise<void> {
    this.removed.push(attachment.kind === 'file' ? attachment.name : attachment.sha256);
    return Promise.resolve();
  }

  async add(file: AttachSource): Promise<AttachmentChip> {
    // Read before the first await (rule 29): the gate as it is when the file is handed over.
    const blocked = this.block();
    if (this.gate) await this.gate;
    this.added.push(file.name);
    const bytes = await file.bytes();
    if (file.type.startsWith('image/')) {
      if (blocked) throw new Error(blocked);
      const sha256 = fakeSha(file.name + bytes.length);
      this.sizes.set(sha256, bytes.length);
      this.thumbs.set(sha256, TINY_PICTURE);
      return { attachment: { kind: 'image', sha256, mime: file.type, width: 640, height: 480 }, label: file.name, detail: '640 × 480', thumbnail: TINY_PICTURE };
    }
    const text = new TextDecoder().decode(bytes);
    const lines = text.split('\n').filter((l, i, all) => l || i < all.length - 1).length;
    const size = bytes.length < 1024 ? `${bytes.length} B` : `${Math.round(bytes.length / 1024)} KB`;
    const sha256 = fakeSha(file.name + text);
    this.sizes.set(sha256, bytes.length);
    return { attachment: { kind: 'file', sha256, name: file.name }, label: file.name, detail: `${size} · ${lines} ${lines === 1 ? 'line' : 'lines'}` };
  }

  override thumbnail(attachment: Attachment): Promise<string | null> {
    return Promise.resolve(this.thumbs.get(attachment.sha256) ?? null);
  }

  /** About how many tokens a file this port took takes: four bytes a token, as the engine estimates (for
   *  `MemoryAssistantOptions.attachmentTokens`); a plausible 250 for one it does not know. */
  tokensOf(attachment: Attachment): number {
    const bytes = this.sizes.get(attachment.sha256);
    return bytes === undefined ? 250 : Math.ceil(bytes / 4);
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
