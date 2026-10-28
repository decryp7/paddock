import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { DiagnosticsStore } from '../../state/diagnostics.store';
import { middlePath, redactionRows, redactionTotal, sizeInKb } from './diagnostics-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatTooltip } from '@angular/material/tooltip';

const NEVER = [
  'The contents of your documents or any file outside Dyno',
  'Browser data: history, cookies, saved logins',
  'Passwords, keys and tokens',
  'Command lines of the programs you run',
];

/** The diagnostics bundle: a preview of what a bug-report zip contains and one button that asks where to save it. Nothing is sent anywhere. */
@Component({
  selector: 'dy-diagnostics-page',
  imports: [MatTooltip, MatButton, MatIconButton, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Diagnostics bundle</h1>
          <p>A zip for a bug report. Made only when you ask, saved where you choose, never sent anywhere.</p>
        </div>
        <button type="button" matButton="filled" [disabled]="store.saving() || !store.preview()" (click)="store.save()">
          <dy-icon name="file" />{{ store.saving() ? 'Saving…' : 'Save bundle...' }}
        </button>
      </header>

      @if (store.saveError(); as e) {
        <div class="note bad" role="alert">
          <dy-icon name="alert" />
          <span class="msg">{{ e.message }}</span>
          <button type="button" matButton="outlined" (click)="store.save()"><dy-icon name="refresh" /> Retry</button>
        </div>
      }
      @if (store.saved(); as s) {
        <div class="note ok" role="status">
          <dy-icon name="check" />
          <span>Saved {{ plural(s.files, 'file') }} ({{ bytes(s.bytes) }}) to</span>
          <code class="path" tabindex="0">{{ s.path }}</code>
          <button type="button" matIconButton aria-label="Copy the saved path" matTooltip="Copy path" (click)="copy(s.path)"><dy-icon name="copy" /></button>
        </div>
      }

      <dy-async [loaded]="!!store.preview()" [error]="store.previewError()" [empty]="files().length === 0 && !!store.preview()" emptyTitle="Nothing to include" emptyText="Dyno has no files to put in a bundle yet." what="the bundle preview" [rows]="5" (retry)="store.loadPreview()">
        @if (store.preview(); as p) {
          <ul class="strip" aria-label="Bundle summary">
            <li><strong class="num">{{ plural(p.files.length, 'file') }}</strong></li>
            <li><strong class="num">{{ bytes(p.totalBytes) }}</strong> before compression</li>
            <li><strong class="num">{{ total() }}</strong> {{ total() === 1 ? 'thing' : 'things' }} redacted</li>
          </ul>

          <div class="cols">
            <section class="card files" aria-label="Files in the bundle">
              <div class="card-head">
                <h2>Files</h2>
                <button type="button" matIconButton class="refresh" aria-label="Refresh the preview" matTooltip="Refresh" (click)="store.loadPreview()"><dy-icon name="refresh" /></button>
              </div>
              <table class="table">
                <caption class="visually-hidden">Files in the diagnostics bundle</caption>
                <colgroup><col class="c-path" /><col /><col class="c-size" /></colgroup>
                <thead><tr><th>Path</th><th>What it holds</th><th class="r">Size (KB)</th></tr></thead>
                <tbody>
                  @for (f of p.files; track f.path) {
                    <tr>
                      <td><span class="mono ell" [matTooltip]="f.path">{{ short(f.path) }}</span></td>
                      <td><span class="ell" [matTooltip]="f.description">{{ f.description }}</span></td>
                      <td class="r">{{ kb(f.bytes) }}</td>
                    </tr>
                  }
                </tbody>
              </table>
            </section>

            <aside class="side">
              <section class="card" aria-label="Redactions">
                <div class="card-head"><h2>Removed before saving</h2></div>
                <ul class="list">
                  @for (r of redactions(); track r.key) {
                    <li [class.zero]="r.count === 0"><span>{{ r.label }}</span><span class="num">{{ r.count }}</span></li>
                  }
                </ul>
              </section>
              <section class="card" aria-label="What is never included">
                <div class="card-head"><h2>What is never included</h2></div>
                <ul class="never">
                  @for (n of never; track n) {
                    <li><dy-icon name="close" />{{ n }}</li>
                  }
                </ul>
                <p class="look">Look through the files before you share the zip. Redaction is thorough but a person should have the last word.</p>
              </section>
            </aside>
          </div>
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .page-head .mat-mdc-button-base { align-self: start; }
    .note { display: flex; align-items: center; gap: var(--sp-2); padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); border-left: 3px solid var(--ok); background: var(--surface-2); font-size: var(--fs-sm); min-width: 0; }
    .note.bad { border-left-color: var(--bad); background: var(--bad-soft); }
    .note > dy-icon { --icon-size: 15px; color: var(--ok); flex: none; }
    .note.bad > dy-icon { color: var(--bad); }
    .note .msg { flex: 1; min-width: 0; }
    .path { font: 12px/1.4 var(--font-mono); user-select: all; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .strip { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: var(--sp-2) var(--sp-6); color: var(--text-2); font-size: var(--fs-base); }
    .strip strong { color: var(--text-1); font-weight: var(--fw-semibold); }
    .num { font-variant-numeric: tabular-nums; white-space: nowrap; }
    .cols { display: grid; grid-template-columns: minmax(0, 1fr) 320px; gap: var(--sp-4); align-items: start; }
    @media (max-width: 1180px) { .cols { grid-template-columns: minmax(0, 1fr); } }
    .files .card-head .refresh { margin-left: auto; }
    .files .table { table-layout: fixed; margin-top: var(--sp-2); }
    .c-path { width: 42%; }
    .c-size { width: 104px; }
    .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .mono { font: 12px/1.4 var(--font-mono); }
    .side { display: grid; gap: var(--sp-4); }
    .list, .never { list-style: none; margin: 0; padding: var(--sp-2) var(--sp-4) var(--sp-3); display: grid; }
    .list li { display: flex; justify-content: space-between; padding: var(--sp-2) 0; border-top: 1px solid var(--border-subtle); font-size: var(--fs-base); }
    .list li:first-child { border-top: 0; }
    .list li.zero { color: var(--text-3); }
    .never li { display: flex; gap: var(--sp-2); align-items: flex-start; padding: var(--sp-1) 0; color: var(--text-2); font-size: var(--fs-sm); }
    .never dy-icon { --icon-size: 13px; color: var(--text-3); margin-top: 2px; flex: none; }
    .look { margin: 0; padding: var(--sp-3) var(--sp-4); border-top: 1px solid var(--border-subtle); color: var(--text-2); font-size: var(--fs-sm); }
  `,
})
export class DiagnosticsPage implements OnInit {
  protected readonly store = inject(DiagnosticsStore);
  protected readonly never = NEVER;
  protected readonly plural = plural;
  protected readonly bytes = formatBytes;
  protected readonly short = middlePath;
  protected readonly kb = sizeInKb;
  protected readonly files = computed(() => this.store.preview()?.files ?? []);
  protected readonly redactions = computed(() => {
    const p = this.store.preview();
    return p ? redactionRows(p.redactions) : [];
  });
  protected readonly total = computed(() => {
    const p = this.store.preview();
    return p ? redactionTotal(p.redactions) : 0;
  });

  ngOnInit(): void {
    void this.store.loadPreview();
  }

  protected async copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // A clipboard that is not available leaves the path selectable on screen.
    }
  }
}
