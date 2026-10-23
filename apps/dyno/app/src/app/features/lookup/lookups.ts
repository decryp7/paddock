import { Injectable, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import type { HashSource } from '../../core/api.types';
import { Dialogs } from '../../shared/dialog';
import { HashSearchDialog, type HashSearchData } from './hash-search-dialog';
import { WebSearchDialog, type WebSearchData } from './web-search-dialog';

/** The two right-click lookups, opened from a context menu: a web search and a hash search. */
@Injectable({ providedIn: 'root' })
export class Lookups {
  private readonly dialogs = inject(Dialogs);

  /** "Search the web…" about `text` (a program name, an issue title); resolves `true` when the browser was opened. */
  async searchWeb(text: string, subject: string): Promise<boolean> {
    const ref = this.dialogs.open<boolean, WebSearchData>(WebSearchDialog, { text, subject });
    return (await firstValueFrom(ref.closed)) === true;
  }

  /** "Search this file's hash on VirusTotal…" for a program; resolves `true` when the browser was opened. */
  async searchHash(source: HashSource, name: string): Promise<boolean> {
    const ref = this.dialogs.open<boolean, HashSearchData>(HashSearchDialog, { source, name });
    return (await firstValueFrom(ref.closed)) === true;
  }
}
