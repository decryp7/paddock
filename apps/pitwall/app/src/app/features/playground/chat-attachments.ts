import { Injectable, computed, inject, signal } from '@angular/core';
import { AssistantAttachments, type Attachment, type AttachmentChip, type AttachRules, type AttachSource } from '@decryp7/paddock-ui/workbench/assistant';
import type { ChatImages } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';

/** "8 B", "12 KB", "1.2 MB": a file's size as its chip shows it. */
export function sizeText(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${Math.round(bytes / 1000)} KB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

/** What the chosen model means for pictures, as Rust's picker reported it (the Chat tab sets it, the attachments read it). */
@Injectable()
export class ChatImagesState {
  readonly images = signal<ChatImages | null>(null);
}

/**
 * The chat's attachments (ADR 0023 Decision 4, WP-4b, WP-5) over Pitwall's commands: Rust checks each file (text or code in
 * UTF-8, at most 256 KiB; a picture by its bytes, prepared for the chosen model) and keeps it until its question is sent. This
 * only carries the bytes and names the chip; a refusal is Rust's sentence, and so is why pictures cannot be sent now.
 */
@Injectable()
export class PitwallAttachments extends AssistantAttachments {
  private readonly backend = inject(BACKEND);
  private readonly state = inject(ChatImagesState);

  /** Rust's words when the chosen model cannot read pictures; null while it can, or before the models are known. */
  override readonly imageBlock = computed(() => {
    const images = this.state.images();
    return images && !images.allowed ? images.reason : null;
  });

  override thumbnail(attachment: Attachment): Promise<string | null> {
    return this.backend.chatThumbnail(attachment.sha256).catch(() => null);
  }

  rules(): Promise<AttachRules> {
    return this.backend.chatAttachRules();
  }

  override remove(attachment: Attachment): Promise<void> {
    return this.backend.chatDetach(attachment.sha256);
  }

  async add(file: AttachSource): Promise<AttachmentChip> {
    try {
      const taken = await this.backend.chatAttach(file.name, await file.bytes());
      if (taken.attachment.kind === 'image') {
        const { width, height } = taken.attachment;
        return { attachment: taken.attachment, label: file.name, detail: `${width} × ${height} · ${sizeText(taken.bytes)}`, thumbnail: taken.thumbnail };
      }
      const lines = `${taken.lines} ${taken.lines === 1 ? 'line' : 'lines'}`;
      return { attachment: taken.attachment, label: taken.attachment.name, detail: `${sizeText(taken.bytes)} · ${lines}` };
    } catch (e) {
      throw new Error(toCommandError(e).message, { cause: e });
    }
  }
}
