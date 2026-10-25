import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { errorHint } from '../../core/command-error';
import { DialogFrame } from '../../shared/dialog';
import { Icon } from '../../shared/icon';
import { LookupStore } from '../../state/lookup.store';

/** What the search dialog is opened with. */
export interface WebSearchData {
  /** What was right-clicked: a program name, an issue title. The engine redacts it before it is shown. */
  text: string;
  /** Names the thing in the dialog's subtitle. */
  subject: string;
}

/** The longest search text the engine accepts. */
export const MAX_QUERY = 200;

/**
 * "Search the web": the right-clicked text with private details removed (home folders, user and computer names, addresses,
 * GUIDs, serial numbers), editable. Nothing is sent anywhere until Search is clicked; then the default browser opens a search
 * for exactly the text shown here.
 */
@Component({
  selector: 'dy-web-search-dialog',
  imports: [DialogFrame, FormsModule, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="Search the web" [subtitle]="data.subject" icon="search">
      <p class="msg">Edit the text if you like. Nothing leaves your computer until you click Search; then your browser opens a search for exactly this text.</p>
      @if (store.seed(); as s) {
        <label class="field">
          <span class="label">Search for</span>
          <input class="input" name="query" [ngModel]="text()" (ngModelChange)="text.set($event)" (keydown.enter)="search()" [attr.maxlength]="max" aria-describedby="removed" autocomplete="off" spellcheck="false" />
        </label>
        <p id="removed" class="removed" [class.none]="s.removed.length === 0">
          <dy-icon [name]="s.removed.length === 0 ? 'check' : 'shield-check'" />
          <span>{{ removedText() }}</span>
        </p>
      } @else if (!store.error()) {
        <div class="skeleton line" aria-busy="true" aria-label="Preparing the search text"></div>
      }
      @if (store.error(); as e) {
        <div class="err" role="alert"><dy-icon name="alert" /><span>{{ e.message }} @if (hint(); as h) {<small>{{ h }}</small>}</span></div>
      }
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close()">Cancel</button>
        <button type="button" class="btn primary" [disabled]="!canSearch()" (click)="search()"><dy-icon name="external" /> Search</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    :host { --dialog-w: 520px; }
    .msg { color: var(--text-2); }
    .removed { margin: 0; display: flex; gap: var(--sp-2); align-items: center; color: var(--text-2); font-size: var(--fs-sm); }
    .removed dy-icon { --icon-size: 15px; color: var(--accent-text); }
    .removed.none dy-icon { color: var(--text-3); }
    .line { height: var(--control-h); }
    .err { display: flex; gap: var(--sp-2); color: var(--bad); }
    .err small { display: block; color: var(--text-2); }
  `,
})
export class WebSearchDialog implements OnInit {
  protected readonly data = inject<WebSearchData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
  protected readonly store = inject(LookupStore);
  protected readonly max = MAX_QUERY;
  protected readonly text = signal('');
  protected readonly hint = computed(() => {
    const e = this.store.error();
    return e ? errorHint(e.code) : null;
  });
  protected readonly canSearch = computed(() => !this.store.busy() && this.store.seed() !== null && this.text().trim().length > 0);
  protected readonly removedText = computed(() => {
    const r = this.store.seed()?.removed ?? [];
    return r.length === 0 ? 'Nothing private was found in this text.' : `Removed from the text: ${r.join(', ')}.`;
  });

  async ngOnInit(): Promise<void> {
    this.store.reset();
    await this.store.loadSeed(this.data.text);
    this.text.set(this.store.seed()?.text ?? '');
  }

  protected async search(): Promise<void> {
    if (!this.canSearch()) return;
    if (await this.store.search(this.text())) this.ref.close(true);
  }
}
