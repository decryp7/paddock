import { ChangeDetectionStrategy, Component, OnInit, computed, inject, input, signal } from '@angular/core';
import { DialogActionError, type DialogAction, type DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import { FormsModule } from '@angular/forms';
import { errorHint } from '../../core/command-error';
import { Icon } from '../../shared/icon';
import { LookupStore } from '../../state/lookup.store';
import { MatFormField } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';

/** The longest search text the engine accepts. */
export const MAX_QUERY = 200;

/**
 * "Search the web": the right-clicked text with private details removed (home folders, user and computer names, addresses,
 * GUIDs, serial numbers), editable. Nothing is sent anywhere until Search is clicked; then the default browser opens a search
 * for exactly the text shown here.
 */
@Component({
  selector: 'dy-web-search-dialog',
  imports: [MatFormField, MatInput, FormsModule, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
      <p class="subject">About <strong>{{ subject() }}</strong></p>
      <p class="msg">Edit the text if you like. Nothing leaves your computer until you click Search; then your browser opens a search for exactly this text.</p>
      @if (store.seed(); as s) {
        <label class="field">
          <span class="label">Search for</span>
          <mat-form-field><input matInput name="query" [ngModel]="query()" (ngModelChange)="query.set($event)" [attr.maxlength]="max" aria-describedby="removed" autocomplete="off" spellcheck="false" /></mat-form-field>
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
  `,
  styles: `
    :host { display: grid; gap: var(--sp-3); min-width: 0; }
    .subject { color: var(--text-2); overflow-wrap: anywhere; }
    .msg { color: var(--text-2); }
    .removed { margin: 0; display: flex; gap: var(--sp-2); align-items: center; color: var(--text-2); font-size: var(--fs-sm); }
    .removed dy-icon { --icon-size: 15px; color: var(--accent-text); }
    .removed.none dy-icon { color: var(--text-3); }
    .line { height: var(--control-h); }
    .err { display: flex; gap: var(--sp-2); color: var(--bad); }
    .err small { display: block; color: var(--text-2); }
  `,
})
export class WebSearchDialog implements DialogContent<true>, OnInit {
  /** What was right-clicked: a program name, an issue title. The engine redacts it before it is shown. */
  readonly text = input('');
  /** Names the thing the search is about. */
  readonly subject = input('');
  protected readonly store = inject(LookupStore);
  protected readonly max = MAX_QUERY;
  protected readonly query = signal('');
  protected readonly hint = computed(() => {
    const e = this.store.error();
    return e ? errorHint(e.code) : null;
  });
  protected readonly canSearch = computed(() => !this.store.busy() && this.store.seed() !== null && this.query().trim().length > 0);
  protected readonly removedText = computed(() => {
    const r = this.store.seed()?.removed ?? [];
    return r.length === 0 ? 'Nothing private was found in this text.' : `Removed from the text: ${r.join(', ')}.`;
  });

  /** Cancel, and Search: the browser opens on the click, with exactly the text shown. */
  readonly dialogActions = computed<readonly DialogAction<true>[]>(() => [
    { kind: 'dismiss', id: 'cancel', label: 'Cancel' },
    { kind: 'answer', id: 'search', label: 'Search', role: 'primary', enabled: this.canSearch(), answer: () => this.search() },
  ]);

  async ngOnInit(): Promise<void> {
    this.store.reset();
    await this.store.loadSeed(this.text());
    this.query.set(this.store.seed()?.text ?? '');
  }

  private async search(): Promise<true> {
    if (!(await this.store.search(this.query()))) throw new DialogActionError(this.store.error()?.message ?? 'The browser could not be opened.');
    return true;
  }
}
