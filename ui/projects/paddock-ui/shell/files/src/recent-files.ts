import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { PdPathText } from '@decryp7/paddock-ui';
import { FilesStore } from './files.store';

/**
 * One recent list: each entry is a button that grants the path to this window again and tells the page to open it (`choose`,
 * with the canonical path), with Reveal and Remove beside it (shown on hover and focus, 28 px targets, 16 px glyphs), and Clear
 * beneath. The path shortens in the middle with the full value as its tooltip (rule 18). A refusal ("this file is gone") is an
 * inline message with its next step, never a dialog. Needs `provideShellFiles` above it.
 */
@Component({
  selector: 'pd-recent-files',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph, PdPathText],
  template: `
    @if (entries().length === 0) {
      <p class="pd-empty" data-testid="recent-empty">{{ emptyText() }}</p>
    } @else {
      <ul class="pd-list" [attr.aria-label]="label()">
        @for (entry of entries(); track entry.path) {
          <li class="pd-row" data-testid="recent-entry">
            <button type="button" class="pd-entry" (click)="reopen(entry.path)">
              @if (entry.name) {
                <span class="pd-name">{{ entry.name }}</span>
              }
              <pd-path-text [value]="entry.path" [plain]="false" />
            </button>
            <span class="pd-actions">
              <button type="button" class="pd-icon" [attr.aria-label]="revealLabel" [attr.title]="revealLabel" (click)="reveal(entry.path)">
                <pd-glyph name="folder" [size]="16" />
              </button>
              <button type="button" class="pd-icon" [attr.aria-label]="removeLabel" [attr.title]="removeLabel" (click)="store.remove(list(), entry.path)">
                <pd-glyph name="close" [size]="16" />
              </button>
            </span>
          </li>
        }
      </ul>
      <button type="button" class="pd-clear" (click)="store.clear(list())">{{ clearLabel }}</button>
    }
    @if (store.failure(); as failure) {
      <p class="pd-failure" role="alert" data-testid="recent-failure">
        {{ failure.message }}
        @if (failure.hint) {
          <span class="pd-hint">{{ failure.hint }}</span>
        }
      </p>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; container-type: inline-size; }
    .pd-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
    .pd-row { display: flex; align-items: center; gap: var(--pd-space-1); min-width: 0; border-radius: var(--pd-radius-control); }
    .pd-row:hover { background: var(--pd-hover); }
    .pd-entry { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; flex: 1 1 auto; min-width: 0; padding: var(--pd-space-1) var(--pd-space-2); border: 0; background: transparent; color: var(--pd-text); font: inherit; text-align: left; cursor: pointer; border-radius: var(--pd-radius-control); }
    .pd-entry:focus-visible, .pd-icon:focus-visible, .pd-clear:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    .pd-entry pd-path-text { width: 100%; color: var(--pd-text-muted); }
    .pd-name { font-weight: 600; }
    .pd-actions { display: inline-flex; flex: none; opacity: 0; }
    .pd-row:hover .pd-actions, .pd-row:focus-within .pd-actions { opacity: 1; }
    @media (hover: none) { .pd-actions { opacity: 1; } }
    .pd-icon { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; }
    .pd-icon:hover { color: var(--pd-text); background: var(--pd-hover); }
    .pd-clear { align-self: flex-start; border: 0; background: transparent; color: var(--pd-text-muted); font: inherit; text-decoration: underline; cursor: pointer; padding: var(--pd-space-1) var(--pd-space-2); }
    .pd-empty { margin: 0; color: var(--pd-text-muted); }
    .pd-failure { margin: 0; color: var(--pd-danger); }
    .pd-hint { display: block; color: var(--pd-text-muted); }
  `,
})
export class PdRecentFiles {
  protected readonly store = inject(FilesStore);
  /** Which list: `files` or `projects` (or an id the app declared). */
  readonly list = input('files');
  /** What the list is called for assistive technology. */
  readonly label = input($localize`:@@pd.files.recent-label:Recent files`);
  /** Shown when the list is empty. */
  readonly emptyText = input($localize`:@@pd.files.recent-empty:Nothing opened yet.`);
  /** The user chose an entry: its canonical path, granted to this window again. */
  readonly choose = output<string>();

  protected readonly entries = computed(() => this.store.entries(this.list()));
  protected readonly revealLabel = $localize`:@@pd.files.reveal:Show in the file manager`;
  protected readonly removeLabel = $localize`:@@pd.files.remove:Remove from the list`;
  protected readonly clearLabel = $localize`:@@pd.files.clear:Clear the list`;

  protected async reopen(path: string): Promise<void> {
    const granted = await this.store.reopen(this.list(), path);
    if (granted) this.choose.emit(granted);
  }

  protected async reveal(path: string): Promise<void> {
    // A reveal needs the grant too: ask for it first (the user's earlier choice), then show it.
    const granted = await this.store.reopen(this.list(), path);
    if (granted) await this.store.reveal(granted);
  }
}
