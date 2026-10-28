import { ChangeDetectionStrategy, Component, OnInit, computed, inject, input } from '@angular/core';
import { DialogActionError, type DialogAction, type DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import type { HashSource } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { formatBytes } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { LookupStore } from '../../state/lookup.store';
import { MatTooltip } from '@angular/material/tooltip';

/**
 * "Search this file's hash on VirusTotal": Dyno reads the file on this computer, shows its SHA-256, and only then, on a click,
 * opens VirusTotal's page for that hash in the browser. The file is never uploaded; only the hash goes into the address.
 */
@Component({
  selector: 'dy-hash-search-dialog',
  imports: [MatTooltip, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
      <p class="subject"><strong>{{ name() }}</strong></p>
      <p class="msg">Dyno computed this hash here, on your computer. Nothing is uploaded: the button opens VirusTotal's page for the hash in your browser.</p>
      @if (store.hash(); as h) {
        <dl>
          <dt>File</dt><dd class="mono sel" [matTooltip]="h.path">{{ h.path }}</dd>
          <dt>Size</dt><dd>{{ size() }}</dd>
          <dt>SHA-256</dt>
          <dd class="hash">
            <code class="mono sel" aria-label="SHA-256 hash">{{ h.sha256 }}</code>
          </dd>
        </dl>
      } @else if (!store.error()) {
        <div class="skeleton line" aria-busy="true" aria-label="Reading the file"></div>
        <p class="wait">Reading the file…</p>
      }
      @if (store.error(); as e) {
        <div class="err" role="alert"><dy-icon name="alert" /><span>{{ e.message }} @if (hint(); as t) {<small>{{ t }}</small>}</span></div>
      }
  `,
  styles: `
    :host { display: grid; gap: var(--sp-3); min-width: 0; }
    .subject { color: var(--text-2); overflow-wrap: anywhere; }
    .msg { color: var(--text-2); }
    dl { margin: 0; display: grid; grid-template-columns: 72px minmax(0, 1fr); gap: var(--sp-2) var(--sp-3); font-size: var(--fs-sm); align-items: start; }
    dt { color: var(--text-3); padding-top: 2px; }
    dd { margin: 0; min-width: 0; }
    dd.mono { font: var(--fw-regular) 12px / 1.4 var(--font-mono); overflow-wrap: anywhere; }
    .hash { display: flex; gap: var(--sp-2); align-items: flex-start; }
    .hash code { flex: 1; font: var(--fw-regular) 12px / 1.5 var(--font-mono); overflow-wrap: anywhere; }
    .sel { user-select: text; cursor: text; }
    .line { height: 56px; }
    .wait { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .err { display: flex; gap: var(--sp-2); color: var(--bad); }
    .err small { display: block; color: var(--text-2); }
  `,
})
export class HashSearchDialog implements DialogContent<true>, OnInit {
  /** Which program's file to read. */
  readonly source = input<HashSource | null>(null);
  /** The program's name, for the heading. */
  readonly name = input('');
  protected readonly store = inject(LookupStore);
  protected readonly size = computed(() => formatBytes(this.store.hash()?.bytes ?? 0));
  protected readonly hint = computed(() => {
    const e = this.store.error();
    return e ? errorHint(e.code) : null;
  });

  /**
   * Copy the hash, Close, and Open VirusTotal. A hash that could not be read is the action's failure: pressing it reads the
   * file again, and the frame says why with Try again.
   */
  readonly dialogActions = computed<readonly DialogAction<true>[]>(() => {
    const h = this.store.hash();
    return [
      { kind: 'run', id: 'copy', label: 'Copy hash', role: 'secondary', lead: true, enabled: h !== null, run: () => this.copy(h?.sha256 ?? '') },
      { kind: 'dismiss', id: 'close', label: 'Close' },
      { kind: 'answer', id: 'open', label: 'Open VirusTotal', role: 'primary', enabled: (h !== null || this.store.error() !== null) && !this.store.busy(), answer: () => this.open() },
    ];
  });

  ngOnInit(): void {
    this.store.reset();
    void this.load();
  }

  private load(): Promise<void> {
    const source = this.source();
    return source ? this.store.loadHash(source) : Promise.resolve();
  }

  private async open(): Promise<true> {
    if (!this.store.hash()) await this.load();
    const h = this.store.hash();
    if (!h) throw new DialogActionError(this.store.error()?.message ?? 'The file could not be read.');
    if (!(await this.store.openVirusTotal(h.sha256))) throw new DialogActionError(this.store.error()?.message ?? 'The browser could not be opened.');
    return true;
  }

  private async copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      throw new DialogActionError('Copying is not allowed here. Select the hash and copy it.');
    }
  }
}
