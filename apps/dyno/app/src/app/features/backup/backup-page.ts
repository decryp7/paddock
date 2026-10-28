import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import type { BackupStatus } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatClock, formatDate } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { injectNow } from '../../shared/now';
import { BackupStore } from '../../state/backup.store';
import { BACKUP_STATE_LABEL, type DotTone, checkedText, heroTone, hue, initial, lastBackupText, stateTone } from './backup-model';

/**
 * Backup health: whether the backups of the tools Dyno can read (Time Machine, Windows Backup, File History, restic, Borg,
 * Duplicati) are recent. Dyno only reads their state; it never runs, schedules or manages a backup. Finding no tool is a
 * normal state, not an error.
 */
@Component({
  selector: 'dy-backup-page',
  imports: [AsyncState, Icon, CdkContextMenuTrigger, CdkMenu, CdkMenuItem],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(keydown)': 'key($event)' },
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Backup health</h1>
          <p>Whether your backups are recent. Dyno only reads their state; it never runs or manages a backup.</p>
        </div>
        <button type="button" class="btn primary" [disabled]="store.busy() || !store.report()" (click)="store.check()">
          <dy-icon name="refresh" [class.spin]="store.busy()" /> {{ store.busy() ? 'Checking…' : 'Check now' }}
        </button>
      </header>

      @if (store.checkError(); as e) {
        <div class="note bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span><button type="button" class="btn sm ghost" (click)="store.check()">Retry</button></div>
      }

      <dy-async [loaded]="!!store.report()" [error]="store.error()" what="backup health" [rows]="4" (retry)="store.load()">
        @if (store.report(); as r) {
          @if (r.statuses.length === 0) {
            <div class="none">
              <dy-icon name="restore" />
              <strong>No backup tool found</strong>
              <span>Dyno looks for Time Machine, Windows Backup, File History, restic, Borg and Duplicati. It only reads their state; it never runs or manages a backup.</span>
              <button type="button" class="btn" [disabled]="store.busy()" (click)="store.check()">Check again</button>
            </div>
          } @else {
            <section class="lead" aria-label="Summary">
              <h2 [attr.data-tone]="heroColour()">{{ store.hero() }}</h2>
              <p aria-live="polite">{{ checked() }}</p>
            </section>

            <div class="layout" [class.with-detail]="!!selected()">
              <div class="tablewrap">
                <table class="table backups" aria-label="Backup tools">
                  <thead>
                    <tr><th class="tool">Tool</th><th class="state">Status</th><th class="when">Last backup</th><th>Summary</th><th class="acts"><span class="visually-hidden">Actions</span></th></tr>
                  </thead>
                  <tbody>
                    @for (s of store.statuses(); track s.tool) {
                      <tr class="row" tabindex="0" [attr.data-tool]="s.tool" [class.sel]="selectedTool() === s.tool" [cdkContextMenuTriggerFor]="menu" [cdkContextMenuTriggerData]="{ status: s }" (click)="select(s)" (keydown.enter)="select(s)" (keydown.shift.f10)="select(s)" (contextmenu)="select(s)">
                        <td class="tool"><span class="tile" aria-hidden="true" [style.background]="tile(s.name)">{{ initial(s.name) }}</span><strong>{{ s.name }}</strong></td>
                        <td class="state">
                          <span class="dot" [attr.data-tone]="dot(s)" aria-hidden="true"></span>
                          @if (s.state === 'healthy') {
                            <span class="visually-hidden">{{ label(s) }}</span>
                          } @else {
                            {{ label(s) }}
                          }
                        </td>
                        <td class="when num muted" [title]="fullDate(s)">{{ last(s) }}</td>
                        <td class="muted"><span class="ell" [title]="s.summary">{{ s.summary }}</span></td>
                        <td class="acts"><button type="button" class="ib" [attr.aria-label]="'Details of ' + s.name" title="Details" (click)="select(s); $event.stopPropagation()"><dy-icon name="info" /></button></td>
                      </tr>
                    }
                  </tbody>
                </table>
              </div>

              @if (selected(); as s) {
                <aside class="detail" tabindex="-1" aria-label="Backup details" >
                  <div class="dhead">
                    <strong>{{ s.name }}</strong>
                    <button type="button" class="ib" aria-label="Close details" (click)="close()"><dy-icon name="close" /></button>
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
                    <button type="button" class="btn sm" (click)="copy(s)"><dy-icon name="copy" /> Copy</button>
                    <button type="button" class="btn sm ghost" (click)="openSettings()">Backup settings</button>
                  </div>
                </aside>
              }
            </div>
          }
        }
      </dy-async>
    </div>

    <ng-template #menu let-status="status">
      <div class="menu" cdkMenu aria-label="Backup actions">
        <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="select(status)"><dy-icon name="info" /> Details</button>
        <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="copy(status)"><dy-icon name="copy" /> Copy summary</button>
      </div>
    </ng-template>
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
    .tablewrap { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); overflow: auto; min-width: 0; }
    .backups { table-layout: fixed; }
    .backups td { height: 48px; overflow: hidden; text-overflow: ellipsis; }
    .backups th.tool, .backups td.tool { width: 220px; }
    .backups th.state, .backups td.state { width: 150px; }
    .backups th.when, .backups td.when { width: 120px; }
    .backups th.acts, .backups td.acts { width: 44px; padding: 0 4px; text-align: right; }
    td.tool { display: table-cell; }
    .tile { display: inline-grid; place-items: center; width: 28px; height: 28px; margin-right: var(--sp-3); border-radius: var(--r-md); color: #fff; font-size: var(--fs-sm); font-weight: var(--fw-semibold); vertical-align: middle; }
    td.tool strong { font-weight: var(--fw-medium); vertical-align: middle; }
    td.state .dot { margin-right: var(--sp-2); vertical-align: 1px; }
    .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    tr.row { cursor: default; }
    tr.row:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
    tr.row.sel td { background: var(--accent-soft); }
    tr.row.sel td:first-child { box-shadow: inset 2px 0 0 var(--accent); }
    tbody tr:last-child td { border-bottom: 0; }
    td.acts { opacity: 0; transition: opacity var(--dur-fast) var(--ease-out); }
    tr.row:hover td.acts, tr.row:focus-within td.acts, tr.row.sel td.acts { opacity: 1; }
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

  /** Up and Down move between rows; Escape closes the details and puts focus back on the row. */
  protected key(e: KeyboardEvent): void {
    if (e.key === 'Escape' && this.selectedTool()) {
      const tool = this.selectedTool();
      this.close();
      (e.currentTarget as HTMLElement).querySelector<HTMLElement>(`tr.row[data-tool="${tool}"]`)?.focus();
      return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const rows = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('tr.row')];
    const next = rows[rows.indexOf(document.activeElement as HTMLElement) + (e.key === 'ArrowDown' ? 1 : -1)];
    if (next) {
      e.preventDefault();
      next.focus();
    }
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
