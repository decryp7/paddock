import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import type { Saved } from '../generated/diagnostics';
import { DiagnosticsStore } from './diagnostics.store';
import { formatBytes, truncateMiddle } from './format';
import { DIAGNOSTICS_STYLES } from './styles';

/**
 * Report a problem (a decision: modal, with a Cancel of the dialog shell's). The engine collects the logs, the system facts
 * and the settings, removes secrets and personal details, and the user sees **every file with its size** before choosing
 * Save; a native dialog then asks where the zip goes. Nothing is sent anywhere: the user attaches the file to their report.
 */
@Component({
  selector: 'pd-report-problem-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="pd-diag__lede">{{ t.lede }}</p>

    <div class="pd-diag__field">
      <label for="pd-diag-description">{{ t.describe }}</label>
      <textarea
        id="pd-diag-description"
        data-testid="diag-description"
        maxlength="10000"
        [attr.aria-describedby]="'pd-diag-hint'"
        [disabled]="store.saving()"
        [value]="store.description()"
        (input)="onInput($event)"
        (change)="store.commitDescription()"
      ></textarea>
      <p id="pd-diag-hint" class="pd-diag__muted">{{ t.hint }}</p>
    </div>

    <section aria-labelledby="pd-diag-files-title" class="pd-diag__field">
      <h3 id="pd-diag-files-title">{{ t.contains }}</h3>
      @if (store.files().length > 0) {
        <div class="pd-diag__scroller" tabindex="0" [attr.aria-label]="t.contains">
          <table data-testid="diag-files">
            <thead>
              <tr>
                <th scope="col">{{ t.file }}</th>
                <th scope="col" class="pd-diag__what">{{ t.what }}</th>
                <th scope="col" class="pd-diag__num">{{ t.size }}</th>
              </tr>
            </thead>
            <tbody>
              @for (f of store.files(); track f.name) {
                <tr>
                  <th scope="row" [title]="f.name">{{ shortName(f.name) }}</th>
                  <td class="pd-diag__what">{{ f.summary }}</td>
                  <td class="pd-diag__num">{{ size(f.bytes) }}</td>
                </tr>
              }
            </tbody>
          </table>
        </div>
        <p class="pd-diag__muted" data-testid="diag-total">{{ total() }}</p>
        @if (store.removed().length > 0) {
          <p data-testid="diag-removed">{{ removedText() }}</p>
        }
        @if (store.notes().length > 0) {
          <ul class="pd-diag__muted" data-testid="diag-notes" [attr.aria-label]="t.notes">
            @for (n of store.notes(); track $index) {
              <li>{{ n }}</li>
            }
          </ul>
        }
      }
      @if (store.requestError(); as problem) {
        <div class="pd-diag__notice pd-diag__notice--danger" role="alert" data-testid="diag-error">
          <span class="pd-diag__title">{{ t.noPreview }}</span>
          <span class="pd-diag__detail">{{ problem }}</span>
          <span class="pd-diag__row"><button type="button" class="pd-diag__btn" (click)="store.load()">{{ t.retry }}</button></span>
        </div>
      } @else if (store.isLoading()) {
        <p class="pd-diag__muted" role="status" data-testid="diag-loading">{{ t.collecting }}</p>
      }
    </section>

    @switch (store.saveState().kind) {
      @case ('saving') {
        <div class="pd-diag__notice" role="status" data-testid="diag-saving">
          <span class="pd-diag__title">{{ t.saving }}</span>
          <progress [attr.aria-label]="t.saving"></progress>
          <span class="pd-diag__row"><button type="button" class="pd-diag__btn" (click)="store.cancel()">{{ t.cancelSave }}</button></span>
        </div>
      }
      @case ('saved') {
        <div class="pd-diag__notice" role="status" data-testid="diag-saved">
          <span class="pd-diag__title">{{ t.saved }}</span>
          <span class="pd-diag__path" [title]="savedPath()" data-testid="diag-saved-path">{{ savedPathShort() }}</span>
          <span class="pd-diag__detail">{{ savedDetail() }}</span>
          <span class="pd-diag__muted">{{ t.attach }}</span>
        </div>
      }
      @case ('dismissed') {
        <p class="pd-diag__muted" role="status" data-testid="diag-dismissed">{{ t.dismissed }}</p>
      }
      @case ('cancelled') {
        <p class="pd-diag__muted" role="status" data-testid="diag-cancelled">{{ t.cancelled }}</p>
      }
      @case ('failed') {
        <div class="pd-diag__notice pd-diag__notice--danger" role="alert" data-testid="diag-save-error">
          <span class="pd-diag__title">{{ t.notSaved }}</span>
          <span class="pd-diag__detail">{{ saveError() }}</span>
        </div>
      }
    }

    <div class="pd-diag__actions">
      @if (savedResult(); as saved) {
        <button type="button" class="pd-diag__btn pd-diag__btn--primary" data-default data-testid="diag-done" (click)="ref.close(saved)">{{ t.done }}</button>
      } @else {
        <button type="button" class="pd-diag__btn pd-diag__btn--primary" data-default data-testid="diag-save" [disabled]="!store.isLoaded() || store.saving()" (click)="save()">{{ t.save }}</button>
      }
    </div>
  `,
  styles: DIAGNOSTICS_STYLES,
})
export class PdReportProblemDialog {
  protected readonly t = {
    lede: $localize`:@@pd.diag.lede:Nothing is sent anywhere. This makes a file on your computer that you can look through, and attach to your report yourself.`,
    describe: $localize`:@@pd.diag.describe:What went wrong? (optional)`,
    hint: $localize`:@@pd.diag.hint:What you were doing, what you expected, what happened instead. It goes into the report as you write it.`,
    contains: $localize`:@@pd.diag.contains:What the report contains`,
    file: $localize`:@@pd.diag.file:File`,
    what: $localize`:@@pd.diag.what:What it is`,
    size: $localize`:@@pd.diag.size:Size`,
    notes: $localize`:@@pd.diag.notes:Left out of the report`,
    collecting: $localize`:@@pd.diag.collecting:Putting the report together…`,
    noPreview: $localize`:@@pd.diag.no-preview-title:The report could not be put together.`,
    retry: $localize`:@@pd.diag.retry:Retry`,
    save: $localize`:@@pd.diag.save:Save report…`,
    done: $localize`:@@pd.diag.done:Done`,
    saving: $localize`:@@pd.diag.saving:Saving the report…`,
    cancelSave: $localize`:@@pd.diag.cancel-save:Stop saving`,
    saved: $localize`:@@pd.diag.saved:The report is saved.`,
    attach: $localize`:@@pd.diag.attach:Attach this file to your bug report.`,
    dismissed: $localize`:@@pd.diag.dismissed:Nothing was saved. Choose Save report… when you are ready.`,
    cancelled: $localize`:@@pd.diag.cancelled:Saving was stopped. Nothing was written.`,
    notSaved: $localize`:@@pd.diag.not-saved:The report was not saved.`,
  };

  protected readonly store = inject(DiagnosticsStore);
  protected readonly ref = inject<DialogRef<Saved>>(DIALOG_REF);

  protected readonly total = computed(() => {
    const files = this.store.files().length;
    const bytes = formatBytes(this.store.totalBytes());
    return $localize`:@@pd.diag.total:${files}:files: files, ${bytes}:bytes: before compression`;
  });
  protected readonly removedText = computed(() => {
    const list = this.store.removed().join(', ');
    return $localize`:@@pd.diag.removed:Removed from the files: ${list}:list:.`;
  });

  /** The saved report, once saved (the dialog then offers Done instead of Save). */
  protected readonly savedResult = computed<Saved | null>(() => {
    const s = this.store.saveState();
    return s.kind === 'saved' ? s.saved : null;
  });
  protected readonly savedPath = computed(() => this.savedResult()?.path ?? '');
  protected readonly savedPathShort = computed(() => truncateMiddle(this.savedPath(), 56));
  protected readonly savedDetail = computed(() => {
    const s = this.savedResult();
    if (!s) return '';
    const files = s.files;
    const size = formatBytes(s.bytes);
    return $localize`:@@pd.diag.saved-detail:${files}:files: files, ${size}:size:`;
  });
  protected readonly saveError = computed(() => {
    const s = this.store.saveState();
    return s.kind === 'failed' ? s.message : '';
  });

  protected size(bytes: number): string {
    return formatBytes(bytes);
  }

  protected shortName(name: string): string {
    return truncateMiddle(name, 40);
  }

  protected onInput(event: Event): void {
    this.store.setDescription((event.target as HTMLTextAreaElement).value);
  }

  protected save(): void {
    // The description may still be unfocused (typed, then Save by keyboard): the engine gets the store's current text.
    void this.store.save();
  }
}
