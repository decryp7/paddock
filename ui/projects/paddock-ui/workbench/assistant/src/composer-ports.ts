import type { Attachment } from '../generated/assistant';
import type { Unlisten } from './backend';

/** A file the user picked, pasted or dropped, before the host has read it. */
export interface AttachSource {
  /** Its name as the system gave it. */
  readonly name: string;
  /** Its media type as the system guessed it (may be empty). */
  readonly type: string;
  /** Its size in bytes. */
  readonly size: number;
  /** Its bytes (read only when the host takes it). */
  bytes(): Promise<Uint8Array>;
}

/** What the composer may attach now, in the host engine's words and numbers. */
export interface AttachRules {
  /** Largest file in bytes (the engine checks again). */
  readonly maxFileBytes: number;
  /** Most attachments one question carries. */
  readonly maxCount: number;
  /** The file picker's filter (`accept`). */
  readonly accept: string;
  /** What can be attached, in a few words (the attach button's tooltip). */
  readonly hint: string;
}

/** An attachment as the composer and the sent question show it. */
export interface AttachmentChip {
  /** The reference the question carries (the bytes stay in the host). */
  readonly attachment: Attachment;
  /** The chip's text: the file's name. */
  readonly label: string;
  /** A second, quieter part ("12 KB · 340 lines"); `null` for none. */
  readonly detail: string | null;
}

/**
 * The host's attachments (ADR 0023 Decision 4, WP-4b): optional. Provide it next to `AssistantBackend` and the composer gains
 * an attach button, paste and drop; without it the composer has none. The host's engine checks every file (type, size, text
 * encoding) and keeps the bytes; a refusal is a thrown error whose message says why and what to do.
 */
export abstract class AssistantAttachments {
  /** What may be attached now. */
  abstract rules(): Promise<AttachRules>;
  /** Hands one file to the engine; resolves with its chip or rejects with the engine's words. */
  abstract add(file: AttachSource): Promise<AttachmentChip>;
  /** The user took a file off the question before sending it: the engine may forget it (default: nothing to do). */
  remove(attachment: Attachment): Promise<void> {
    void attachment;
    return Promise.resolve();
  }
  /** The chip of an attachment a stored question carries (for a reopened conversation); a host may say more. */
  chipOf(attachment: Attachment): AttachmentChip {
    return attachmentChip(attachment);
  }
}

/** The plain chip of an attachment: a file's name, an image's size, a clip's length. */
export function attachmentChip(attachment: Attachment): AttachmentChip {
  switch (attachment.kind) {
    case 'file':
      return { attachment, label: attachment.name, detail: null };
    case 'image':
      return { attachment, label: $localize`:@@pd.wb.assistant.attach.image:Image`, detail: `${attachment.width} × ${attachment.height}` };
    case 'audio':
      return { attachment, label: $localize`:@@pd.wb.assistant.attach.audio:Audio clip`, detail: `${attachment.seconds} s` };
  }
}

/** Where push-to-talk is, as the host's engine reports it (it owns the microphone and the clock, ER-18). */
export type SpeechState =
  | { readonly kind: 'idle' }
  | { readonly kind: 'recording'; readonly elapsedMs: number }
  | { readonly kind: 'transcribing' }
  | { readonly kind: 'failed'; readonly message: string };

/**
 * The host's speech input (ADR 0023 Decision 5, WP-4b's half): optional. With it the composer gains a microphone button:
 * press to record, press again to stop; the engine transcribes and the text lands in the question to read and edit (nothing
 * is sent by voice alone). Escape cancels a recording. The engine captures, keeps the elapsed time and reports every state
 * through `listen`; this pane keeps no clock of its own.
 */
export abstract class AssistantSpeech {
  /** Starts recording (the engine may first ask for the microphone). */
  abstract start(): Promise<void>;
  /** Stops recording and resolves with the transcript (empty when nothing was heard). */
  abstract finish(): Promise<string>;
  /** Stops recording and throws the clip away. */
  abstract cancel(): Promise<void>;
  /** Every state change until the returned function is called. */
  abstract listen(handler: (state: SpeechState) => void): Promise<Unlisten>;
}
