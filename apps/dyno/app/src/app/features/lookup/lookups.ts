import { Injectable, inject } from '@angular/core';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import type { HashSource } from '../../core/api.types';

/** The two right-click lookups, opened from a context menu: a web search and a hash search, each a real OS window. */
@Injectable({ providedIn: 'root' })
export class Lookups {
  private readonly dialogs = inject(DialogService);

  /** "Search the web…" about `text` (a program name, an issue title); resolves `true` when the browser was opened. */
  async searchWeb(text: string, subject: string): Promise<boolean> {
    const outcome = await this.dialogs.open<true>('dy.web-search', { inputs: { text, subject }, title: `Search the web: ${subject}` }).closed;
    return outcome.kind === 'answered';
  }

  /** "Search this file's hash on VirusTotal…" for a program; resolves `true` when the browser was opened. */
  async searchHash(source: HashSource, name: string): Promise<boolean> {
    const outcome = await this.dialogs.open<true>('dy.file-hash', { inputs: { source, name }, title: "Search the file's hash on VirusTotal" }).closed;
    return outcome.kind === 'answered';
  }
}
