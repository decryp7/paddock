import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatProgressBar } from '@angular/material/progress-bar';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import type { Saved } from '../generated/diagnostics';
import { DiagnosticsStore } from './diagnostics.store';
import { formatStorage, truncateMiddle } from './format';
import { DIAGNOSTICS_STYLES } from './styles';

/**
 * Report a problem (ADR 0026: a modeless form, wide, so the user can look at the app while writing). Its actions are declared:
 * Save report… (the primary, a run that starts the save job), then Done (answering the saved report), and the frame's
 * Close. The engine collects the logs, the system facts
 * and the settings, removes secrets and personal details, and the user sees **every file with its size** before choosing
 * Save; a native dialog then asks where the zip goes. Nothing is sent anywhere: the user attaches the file to their report.
 */
@Component({
  selector: 'pd-report-problem-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButton, MatProgressBar],
  template: `
    <p class="pd-diag__lede">{{ t.lede }}</p>
    <p class="pd-diag__title" role="note" data-testid="diag-public">{{ t.public }}</p>

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

    <section aria-labelledby="pd-diag-not-title" class="pd-diag__field" data-testid="diag-not-in">
      <h3 id="pd-diag-not-title">{{ t.notIn }}</h3>
      <ul class="pd-diag__muted">
        <li>{{ t.notInContents }}</li>
        <li>{{ t.notInSecrets }}</li>
        <li>{{ t.notInPersonal }}</li>
      </ul>
      <p class="pd-diag__muted">{{ t.mayRemain }}</p>
    </section>

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
                  <th scope="row" class="pd-diag__name" [title]="f.name">{{ shortName(f.name) }}</th>
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
          <span class="pd-diag__row"><button matButton="outlined" type="button" (click)="store.load()">{{ t.retry }}</button></span>
        </div>
      } @else if (store.isLoading()) {
        <p class="pd-diag__muted" role="status" data-testid="diag-loading">{{ t.collecting }}</p>
      }
    </section>

    @switch (store.saveState().kind) {
      @case ('saving') {
        <div class="pd-diag__notice" role="status" data-testid="diag-saving">
          <span class="pd-diag__title">{{ t.saving }}</span>
          <mat-progress-bar mode="indeterminate" [attr.aria-label]="t.saving" />
          <span class="pd-diag__row"><button matButton="outlined" type="button" (click)="store.cancel()">{{ t.cancelSave }}</button></span>
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
  `,
  styles: DIAGNOSTICS_STYLES,
})
export class PdReportProblemDialog implements DialogContent<Saved> {
  protected readonly t = {
    lede: $localize`:@@pd.diag.lede:Nothing is sent anywhere. This makes a file on your computer that you can look through, and attach to your report yourself.`,
    public: $localize`:@@pd.diag.public:Issue reports are public. Look through the zip first.`,
    notIn: $localize`:@@pd.diag.not-in:Not in the report`,
    notInContents: $localize`:@@pd.diag.not-in-contents:The contents of your documents and files.`,
    notInSecrets: $localize`:@@pd.diag.not-in-secrets:Passwords, tokens and keys (they are replaced where the app can recognise them).`,
    notInPersonal: $localize`:@@pd.diag.not-in-personal:Your user name, home folder, machine name, e-mail addresses and IP addresses (they are replaced), except this computer's own address (127.0.0.1) and private network addresses (10.x.x.x, 172.16-31.x.x, 192.168.x.x), which are kept for diagnosis.`,
    mayRemain: $localize`:@@pd.diag.may-remain:File names outside your home folder may remain, and so may other computers' names and text the app imported or showed.`,
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

  protected readonly total = computed(() => {
    const files = this.store.files().length;
    const bytes = formatStorage(this.store.totalBytes());
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
    const size = formatStorage(s.bytes);
    return $localize`:@@pd.diag.saved-detail:${files}:files: files, ${size}:size:`;
  });
  protected readonly saveError = computed(() => {
    const s = this.store.saveState();
    return s.kind === 'failed' ? s.message : '';
  });

  protected size(bytes: number): string {
    return formatStorage(bytes);
  }

  protected shortName(name: string): string {
    return truncateMiddle(name, 40);
  }

  protected onInput(event: Event): void {
    this.store.setDescription((event.target as HTMLTextAreaElement).value);
  }

  /** Save report… until the report is saved, then Done; the frame adds Close. */
  readonly dialogActions = computed<readonly DialogAction<Saved>[]>(() => {
    const saved = this.savedResult();
    if (saved) return [{ kind: 'answer', id: 'done', label: this.t.done, role: 'primary', answer: () => saved }];
    // Save starts the engine's save job and returns at once; its progress and Stop saving are in the body.
    return [{ kind: 'run', id: 'save', label: this.t.save, role: 'primary', enabled: this.store.isLoaded() && !this.store.saving(), run: () => this.save() }];
  });

  private save(): void {
    // The description may still be unfocused (typed, then Save by keyboard): the engine gets the store's current text.
    void this.store.save();
  }
}
