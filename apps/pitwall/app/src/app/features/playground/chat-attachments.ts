import { Injectable, inject } from '@angular/core';
import { AssistantAttachments, type Attachment, type AttachmentChip, type AttachRules, type AttachSource } from '@decryp7/paddock-ui/workbench/assistant';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import type { ChatAttached } from '../../core/api.types';

/** "8 B", "12 KB", "1.2 MB": a file's size as its chip shows it. */
export function sizeText(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${Math.round(bytes / 1000)} KB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

/** The chip's second part for a file Rust took: an image's size in pixels and bytes, a text file's bytes and lines. */
export function attachedDetail(taken: ChatAttached): string {
  if (taken.attachment.kind === 'image') return `${taken.attachment.width} × ${taken.attachment.height} · ${sizeText(taken.bytes)}`;
  return `${sizeText(taken.bytes)} · ${taken.lines} ${taken.lines === 1 ? 'line' : 'lines'}`;
}

/**
 * The chat's attachments (ADR 0023 Decision 4, WP-4b and WP-5) over Pitwall's commands: Rust checks each file (an image up to
 * 25 MiB, prepared for the chat's model; text or code in UTF-8 up to 256 KiB) and keeps it until its question is sent. This
 * only carries the bytes and names the chip; a refusal is Rust's sentence.
 */
@Injectable()
export class PitwallAttachments extends AssistantAttachments {
  private readonly backend = inject(BACKEND);

  rules(): Promise<AttachRules> {
    return this.backend.chatAttachRules();
  }

  override remove(attachment: Attachment): Promise<void> {
    return this.backend.chatDetach(attachment.sha256);
  }

  async add(file: AttachSource): Promise<AttachmentChip> {
    try {
      const taken = await this.backend.chatAttach(file.name, await file.bytes());
      return { attachment: taken.attachment, label: taken.name, detail: attachedDetail(taken) };
    } catch (e) {
      throw new Error(toCommandError(e).message, { cause: e });
    }
  }
}
