import { Injectable, inject } from '@angular/core';
import { AssistantAttachments, type Attachment, type AttachmentChip, type AttachRules, type AttachSource } from '@decryp7/paddock-ui/workbench/assistant';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';

/** "8 B", "12 KB", "1.2 MB": a file's size as its chip shows it. */
export function sizeText(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${Math.round(bytes / 1000)} KB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

/**
 * The chat's attachments (ADR 0023 Decision 4, WP-4b) over Pitwall's commands: Rust checks each file (text or code in UTF-8,
 * at most 256 KiB; images wait for WP-5) and keeps it until its question is sent. This only carries the bytes and names the
 * chip; a refusal is Rust's sentence.
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
      const lines = `${taken.lines} ${taken.lines === 1 ? 'line' : 'lines'}`;
      return { attachment: taken.attachment, label: taken.attachment.name, detail: `${sizeText(taken.bytes)} · ${lines}` };
    } catch (e) {
      throw new Error(toCommandError(e).message, { cause: e });
    }
  }
}
