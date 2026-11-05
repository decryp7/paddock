import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent } from '@decryp7/paddock-ui/table';
import { FullTextTip } from '../../shared/full-text-tip';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal, viewChild } from '@angular/core';
import type { BackupStatus } from '../../core/api.types';
import { ReadingSource } from '../../core/reading-source';
import { AsyncState } from '../../shared/async-state';
import { formatClock, formatDate } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { JobProgressCard } from '../../shared/job-progress';
import { Notifier } from '../../shared/notifier';
import { injectNow } from '../../shared/now';
import { BackupStore } from '../../state/backup.store';
import { BACKUP_STATE_LABEL, type DotTone, checkedText, heroTone, hue, initial, lastBackupText, stateTone } from './backup-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatTooltip } from '@angular/material/tooltip';
import { MatMenuModule } from '@angular/material/menu';

/**
 * Backup health: whether the backups of the tools Dyno can read (Time Machine, Windows Backup, File History, restic, Borg,
 * Duplicati) are recent. Dyno only reads their state; it never runs, schedules or manages a backup. Finding no tool is a
 * normal state, not an error.
 */
@Component({
  selector: 'dy-backup-page',
  imports: [FullTextTip, AsyncState, Icon, PdTable, PdCell, MatMenuModule, MatIconButton, MatTooltip, MatButton, JobProgressCard, ReadingSource],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Backup health</h1>
          <p>Whether your backups are recent. Dyno only reads their state; it never runs or manages a backup.</p>
        </div>
        <button type="button" matButton="filled" [disabled]="store.busy() || !store.report()" (click)="store.check()">
          <dy-icon name="refresh" [class.spin]="store.busy()" /> {{ store.busy() ? 'Checking…' : 'Check now' }}
        </button>
      </header>

      @if (store.checkError(); as e) {
        <div class="note bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span><button type="button" matButton="text" (click)="store.check()">Retry</button></div>
      }

      @if (store.checkJob(); as job) {
        <dy-job-progress [progress]="job" (cancel)="store.cancelCheck()" />
      }

      <dy-async [loaded]="!!store.report()" [error]="store.error()" what="backup health" [rows]="4" (retry)="store.load()">
        @if (store.report(); as r) {
          @if (r.statuses.length === 0) {
            <div class="none">
              <dy-icon name="restore" />
              <strong>No backup tool found</strong>
              <span>Dyno looks for Time Machine, Windows Backup, File History, restic, Borg and Duplicati.</span>
              <button type="button" matButton="outlined" [disabled]="store.busy()" (click)="store.check()">Check again</button>
            </div>
          } @else {
            <section class="lead" aria-label="Summary">
              <h2 [attr.data-tone]="heroColour()">{{ store.hero() }}</h2>
              <p aria-live="polite">{{ checked() }}</p>
              <dy-reading-source [via]="r.via" [canRetry]="true" (retry)="store.check()" />
            </section>

            <div class="layout" [class.with-detail]="!!selected()">
              <pd-table height="window" [toolbar]="false" label="Backup tools" [rows]="store.statuses()" [columns]="columns" [rowKey]="toolKey" [filterable]="false" [rowHeight]="36" [rowActions]="actions" [selectedKey]="selectedTool()" (rowActivate)="select($event)" (action)="run($event)" (keydown.escape)="escape($event)">
                <ng-template pdCell="tool" let-s><span class="tool"><span class="tile" aria-hidden="true" [style.background]="tile(s.name)">{{ initial(s.name) }}</span><strong class="nm" [dyFullText]="s.name">{{ s.name }}</strong></span></ng-template>
                <ng-template pdCell="state" let-s><span class="dot" [attr.data-tone]="dot(s)" aria-hidden="true"></span>{{ label(s) }}</ng-template>
                <ng-template pdCell="when" let-s><span class="num muted" [attr.title]="fullDate(s)">{{ last(s) }}</span></ng-template>
              </pd-table>

              @if (selected(); as s) {
                <aside class="detail" tabindex="-1" aria-label="Backup details" >
                  <div class="dhead">
                    <strong>{{ s.name }}</strong>
                    <button type="button" matIconButton matTooltip="Close details" aria-label="Close details" (click)="close()"><dy-icon name="close" /></button>
                  </div>
                  <p class="state-line"><span class="dot" [attr.data-tone]="dot(s)" aria-hidden="true"></span>{{ label(s) }}</p>
                  <dl>
                    <dt>Last backup</dt><dd>{{ fullDate(s) }}</dd>
                    <dt>What Dyno saw</dt><dd class="sel">{{ s.summary }}</dd>
                  </dl>
                  @if (s.nextStep) {
                    <h3>What to do next</h3>
                    <p class="next sel">{{ s.nextStep }}</p>
                  }
                  <div class="dacts">
                    <button type="button" matButton="outlined" (click)="copy(s)"><dy-icon name="copy" /> Copy</button>
                    <button type="button" matButton="text" (click)="openSettings()">Backup settings</button>
                  </div>
                </aside>
              }
            </div>
          }
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; max-width: 1440px; }
    .page-head { margin-bottom: 0; }
    dy-async { display: grid; gap: var(--sp-4); }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: center; color: var(--text-2); font-size: var(--fs-sm); }
    .note.bad { color: var(--bad); }
    .lead { display: grid; gap: var(--sp-1); }
    .lead h2 { margin: 0; font-size: 24px; font-weight: var(--fw-semibold); letter-spacing: -0.01em; }
    .lead h2[data-tone='bad'] { color: var(--bad); }
    .lead h2[data-tone='warn'] { color: var(--warn); }
    .lead p { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .layout.with-detail { grid-template-columns: minmax(0, 1fr) 340px; }
    .tool { display: flex; align-items: center; gap: var(--sp-3); min-width: 0; }
    .tool strong { font-weight: var(--fw-medium); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .tile { flex: none; display: inline-grid; place-items: center; width: 28px; height: 28px; border-radius: var(--r-md); color: #fff; font-size: var(--fs-sm); font-weight: var(--fw-semibold); }
    .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: var(--sp-2); flex: none; background: var(--border-strong); }
    .dot[data-tone='ok'] { background: var(--ok); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .dot[data-tone='bad'] { background: var(--bad); }
    .muted { color: var(--text-2); }
    .num { font-variant-numeric: tabular-nums; }
    .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .ib { display: inline-grid; place-items: center; width: 24px; height: 24px; border: 0; border-radius: var(--r-sm); background: none; color: var(--text-2); cursor: pointer; }
    .ib:hover { background: var(--surface-active); color: var(--text-1); }
    .ib dy-icon { --icon-size: 15px; }
    .none { display: grid; justify-items: center; gap: var(--sp-2); padding: var(--sp-8) var(--sp-5); text-align: center; color: var(--text-3); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); }
    .none dy-icon { --icon-size: 32px; }
    .none strong { color: var(--text-1); font-size: 15px; font-weight: var(--fw-semibold); }
    .none span { max-width: 56ch; font-size: var(--fs-sm); }
    .detail { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-3) var(--sp-4); display: grid; gap: var(--sp-2); }
    .dhead { display: flex; justify-content: space-between; align-items: flex-start; gap: var(--sp-2); }
    .state-line { margin: 0; display: flex; align-items: center; gap: var(--sp-2); font-size: var(--fs-sm); color: var(--text-2); }
    h3 { margin: var(--sp-2) 0 0; font-size: var(--fs-sm); font-weight: var(--fw-semibold); color: var(--text-2); }
    dl { margin: 0; display: grid; grid-template-columns: 92px minmax(0, 1fr); gap: 6px var(--sp-3); font-size: var(--fs-sm); }
    dt { color: var(--text-3); }
    dd { margin: 0; overflow-wrap: anywhere; }
    .next { margin: 0; font-size: var(--fs-sm); color: var(--text-1); overflow-wrap: anywhere; }
    .sel { user-select: text; cursor: text; }
    .dacts { display: flex; gap: var(--sp-2); flex-wrap: wrap; margin-top: var(--sp-1); }
    .spin { animation: spin 1s linear infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
    @media (max-width: 1100px) { .layout.with-detail { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class BackupPage implements OnInit {
  protected readonly store = inject(BackupStore);
  private readonly notifier = inject(Notifier);
  private readonly ports = inject(SHELL_PORTS);
  private readonly now = injectNow();
  private readonly grid = viewChild(PdTable);
  protected readonly selectedTool = signal<string | null>(null);
  protected readonly initial = initial;

  protected readonly selected = computed(() => this.store.statuses().find((s) => s.tool === this.selectedTool()) ?? null);
  protected readonly checked = computed(() => {
    const r = this.store.report();
    return r ? checkedText(r.checkedAt, this.store.busy(), this.now()) : '';
  });

  ngOnInit(): void {
    void this.store.load();
  }

  protected readonly heroColour = computed(() => heroTone(this.store.statuses()));
  protected dot = (s: BackupStatus): DotTone => stateTone(s.state);

  protected label = (s: BackupStatus): string => BACKUP_STATE_LABEL[s.state];
  protected tile = (name: string): string => `hsl(${hue(name)} 42% 38%)`;
  protected last = (s: BackupStatus): string => lastBackupText(s.lastBackup, this.now());
  protected fullDate = (s: BackupStatus): string => (s.lastBackup === null ? 'No backup yet' : `${formatDate(s.lastBackup, this.now())}, ${formatClock(s.lastBackup)}`);

  protected select(s: BackupStatus): void {
    this.selectedTool.set(s.tool);
  }

  protected close(): void {
    this.selectedTool.set(null);
  }

  protected readonly toolKey = (s: BackupStatus): string => s.tool;
  protected readonly columns: PdColumn<BackupStatus>[] = [
    { key: 'tool', header: 'Tool', value: (s) => s.name, required: true, width: 'minmax(0, 220px)' },
    { key: 'state', header: 'Status', value: (s) => BACKUP_STATE_LABEL[s.state], width: '150px' },
    { key: 'when', header: 'Last backup', value: (s) => s.lastBackup, text: (s) => lastBackupText(s.lastBackup, this.now()), width: '120px' },
    { key: 'summary', header: 'Summary', value: (s) => s.summary, width: 'minmax(0, 2fr)', hideBelow: 640 },
  ];
  protected readonly actions = (): PdRowAction[] => [
    { id: 'details', label: 'Details', icon: 'info', inline: true },
    { id: 'copy', label: 'Copy summary', icon: 'copy' },
  ];

  protected run({ id, row }: PdRowActionEvent<BackupStatus>): void {
    if (id === 'copy') void this.copy(row);
    else this.select(row);
  }

  /** Escape closes the details and puts the focus back on the tool's row. */
  protected escape(e: Event): void {
    const tool = this.selectedTool();
    if (!tool) return;
    e.preventDefault();
    this.close();
    this.grid()?.focusRow(tool);
  }

  protected async copy(s: BackupStatus): Promise<void> {
    const text = [`${s.name}: ${this.label(s)}`, s.summary, s.nextStep ?? ''].filter(Boolean).join('\n');
    try {
      await navigator.clipboard.writeText(text);
      this.notifier.show('Copied');
    } catch {
      this.notifier.show('Copying is not allowed here.', 'bad');
    }
  }

  /** Opens the shell's Settings window (Dyno's page is in it). */
  protected openSettings(): void {
    void this.ports.openDialog('pd.settings');
  }
}
