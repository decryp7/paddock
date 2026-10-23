import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import type { HashSource } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { DialogFrame } from '../../shared/dialog';
import { formatBytes } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { LookupStore } from '../../state/lookup.store';

/** What the hash dialog is opened with. */
export interface HashSearchData {
  source: HashSource;
  /** The program's name, for the subtitle. */
  name: string;
}

/**
 * "Search this file's hash on VirusTotal": Dyno reads the file on this computer, shows its SHA-256, and only then, on a click,
 * opens VirusTotal's page for that hash in the browser. The file is never uploaded; only the hash goes into the address.
 */
@Component({
  selector: 'dy-hash-search-dialog',
  imports: [DialogFrame, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="Search the file's hash on VirusTotal" [subtitle]="data.name" icon="shield">
      <p class="msg">Dyno computed this hash here, on your computer. Nothing is uploaded: the button opens VirusTotal's page for the hash in your browser.</p>
      @if (store.hash(); as h) {
        <dl>
          <dt>File</dt><dd class="mono sel" [title]="h.path">{{ h.path }}</dd>
          <dt>Size</dt><dd>{{ size() }}</dd>
          <dt>SHA-256</dt>
          <dd class="hash">
            <code class="mono sel" aria-label="SHA-256 hash">{{ h.sha256 }}</code>
            <button type="button" class="btn ghost icon sm" aria-label="Copy the hash" title="Copy" (click)="copy(h.sha256)"><dy-icon name="copy" /></button>
          </dd>
        </dl>
      } @else if (!store.error()) {
        <div class="skeleton line" aria-busy="true" aria-label="Reading the file"></div>
        <p class="wait">Reading the file…</p>
      }
      @if (store.error(); as e) {
        <div class="err" role="alert"><dy-icon name="alert" /><span>{{ e.message }} @if (hint(); as t) {<small>{{ t }}</small>}</span></div>
      }
      <ng-container footer>
        @if (store.error()) {
          <button type="button" class="btn" (click)="load()">Try again</button>
        }
        <button type="button" class="btn" (click)="ref.close()">Cancel</button>
        <button type="button" class="btn primary" [disabled]="!store.hash() || store.busy()" (click)="open()"><dy-icon name="external" /> Open VirusTotal</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    :host { --dialog-w: 560px; }
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
export class HashSearchDialog implements OnInit {
  protected readonly data = inject<HashSearchData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
  protected readonly store = inject(LookupStore);
  private readonly notifier = inject(Notifier);
  protected readonly size = computed(() => formatBytes(this.store.hash()?.bytes ?? 0));
  protected readonly hint = computed(() => {
    const e = this.store.error();
    return e ? errorHint(e.code) : null;
  });

  ngOnInit(): void {
    this.store.reset();
    void this.load();
  }

  protected load(): Promise<void> {
    return this.store.loadHash(this.data.source);
  }

  protected async open(): Promise<void> {
    const h = this.store.hash();
    if (h && (await this.store.openVirusTotal(h.sha256))) this.ref.close(true);
  }

  protected async copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.notifier.show('Copied');
    } catch {
      this.notifier.show('Copying is not allowed here.', 'bad');
    }
  }
}
