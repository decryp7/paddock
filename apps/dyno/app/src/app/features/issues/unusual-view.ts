import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent } from '@decryp7/paddock-ui/table';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { UnusualItem } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { confirm } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { formatDate, formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { UnusualStore } from '../../state/unusual.store';
import { Lookups } from '../lookup/lookups';
import { hue, initial, learningText, whyText } from './issues-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatTooltip } from '@angular/material/tooltip';
import { FullTextTip } from '../../shared/full-text-tip';
import { MatMenuModule } from '@angular/material/menu';

/**
 * Programs that are new on this computer, and why some of them may be worth a look. A transparency tool: it lists reasons
 * ("unusual because it runs from a temporary folder"), never verdicts, and nothing here stops or removes a program. After
 * a quiet learning period Dyno lists only programs it has not seen before.
 */
@Component({
  selector: 'dy-unusual-view',
  imports: [AsyncState, Icon, PdTable, PdCell, MatMenuModule, FullTextTip, MatTooltip, MatButton, MatIconButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!store.overview()" [error]="store.error()" what="new programs" [rows]="4" (retry)="store.load()">
      @if (store.overview(); as o) {
        <div class="bar" role="group" aria-label="New programs">
          <span class="learn">{{ learning() }}</span>
          <span class="grow"></span>
          <button type="button" matIconButton matTooltip="More actions" aria-label="More actions" [matMenuTriggerFor]="more"><dy-icon name="more" /></button>
        </div>

        @if (!o.enabled) {
          <div class="none"><dy-icon name="eye" /><strong>Watching for new programs is off</strong><span>Turn it on in Settings and Dyno starts learning which programs are normal here.</span><button type="button" matButton="outlined" (click)="openSettings()">Open settings</button></div>
        } @else if (o.items.length === 0) {
          <div class="none">
            <dy-icon [name]="o.learning.active ? 'clock' : 'check'" />
            <strong>{{ o.learning.active ? 'Dyno is learning what is normal here' : 'Nothing new' }}</strong>
            <span>{{ o.learning.active ? 'It notes the programs on this computer without telling you about each one. After that, it lists only programs it has not seen before.' : 'Dyno knows ' + o.known + ' programs on this computer. A new one appears here, with the reasons it may be worth a look.' }}</span>
          </div>
        } @else {
          <div class="layout" [class.with-detail]="!!selected()">
            <pd-table [toolbar]="false" label="New and unusual programs" [rows]="o.items" [columns]="columns" [rowKey]="itemKey" [filterable]="false" [rowActions]="actions" [selectedKey]="selectedKey()" [maxHeight]="620" (rowActivate)="select($event)" (action)="run($event)" (keydown.escape)="selectedKey.set(null)">
              <ng-template pdCell="name" let-i><span class="prog"><span class="av" aria-hidden="true" [style.background]="tile(i.name)">{{ ini(i.name) }}</span><span class="ell">{{ i.name }}</span></span></ng-template>
              <ng-template pdCell="why" let-i><span class="why"><span class="dot" [attr.data-tone]="i.level === 'unusual' ? 'warn' : null" aria-hidden="true"></span><span class="ell">{{ why(i) }}</span></span></ng-template>
            </pd-table>

            @if (selected(); as s) {
              <aside class="detail" tabindex="-1" aria-label="Program details" (keydown.escape)="selectedKey.set(null)">
                <div class="dhead">
                  <span class="prog"><span class="av" aria-hidden="true" [style.background]="tile(s.name)">{{ ini(s.name) }}</span><strong>{{ s.name }}</strong></span>
                  <button type="button" matIconButton matTooltip="Close details" aria-label="Close details" (click)="selectedKey.set(null)"><dy-icon name="close" /></button>
                </div>
                <p class="lead">{{ why(s) }}.</p>
                @if (s.reasons.length > 1) {
                  <ul class="reasons">@for (r of s.reasons; track r) { <li>It {{ r }}.</li> }</ul>
                }
                <dl>
                  @if (s.exe) { <dt>Path</dt><dd class="sel mono" [dyFullText]="s.exe">{{ s.exe }}</dd> }
                  @if (s.publisher) { <dt>Publisher</dt><dd class="sel">{{ s.publisher }}</dd> }
                  <dt>First seen</dt><dd>{{ firstSeenFull(s) }}</dd>
                  <dt>Last seen</dt><dd>{{ s.lastSeen > 0 ? (s.running ? 'Running now' : ago(s.lastSeen)) : 'Has not run since' }}</dd>
                </dl>
                <div class="dacts">
                  @if (s.exe) { <button type="button" matButton="outlined" (click)="copy(s.exe)"><dy-icon name="copy" /> Copy path</button> }
                </div>
                <p class="hint">Dyno never ends or removes a program because of what is shown here.</p>
              </aside>
            }
          </div>
        }
      }
    </dy-async>

    <mat-menu #more="matMenu" aria-label="More actions">
      <ng-template matMenuContent>
        <button mat-menu-item class="danger" (click)="forget()"><dy-icon name="trash" /> Forget the programs Dyno knows…</button>
      </ng-template>
    </mat-menu>
  `,
  styles: `
    :host { display: grid; gap: var(--sp-3); align-content: start; }
    dy-async { display: grid; gap: var(--sp-3); }
    .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; min-height: 44px; }
    .grow { flex: 1; }
    .learn { color: var(--text-2); font-size: var(--fs-sm); }
    .layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .layout.with-detail { grid-template-columns: minmax(0, 1fr) 340px; }
    .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; background: var(--border-strong); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .prog { display: flex; align-items: center; gap: var(--sp-2); min-width: 0; }
    .av { flex: none; display: grid; place-items: center; width: 22px; height: 22px; border-radius: 6px; color: #fff; font: var(--fw-semibold) 12px / 1 var(--font-sans); }
    .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .why { display: flex; align-items: center; gap: var(--sp-2); min-width: 0; }
    .why .dot:not([data-tone]) { background: transparent; }
    .ib { display: inline-grid; place-items: center; width: 24px; height: 24px; border: 0; border-radius: var(--r-sm); background: none; color: var(--text-2); cursor: pointer; }
    .ib:hover:not(:disabled) { background: var(--surface-active); color: var(--text-1); }
    .ib:disabled { opacity: 0.35; cursor: default; }
    .ib dy-icon { --icon-size: 15px; }
    .none { display: grid; justify-items: center; gap: var(--sp-2); padding: var(--sp-8) var(--sp-5); text-align: center; color: var(--text-3); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); }
    .none dy-icon { --icon-size: 32px; }
    .none strong { color: var(--text-1); font-size: 15px; font-weight: var(--fw-semibold); }
    .none span { max-width: 52ch; }
    .detail { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-3) var(--sp-4); display: grid; gap: var(--sp-2); }
    .dhead { display: flex; justify-content: space-between; align-items: center; gap: var(--sp-2); }
    .lead { margin: 0; color: var(--text-1); font-size: var(--fs-sm); overflow-wrap: anywhere; }
    .reasons { margin: 0; padding-left: var(--sp-5); color: var(--text-2); font-size: var(--fs-sm); display: grid; gap: 2px; }
    dl { margin: 0; display: grid; grid-template-columns: 76px minmax(0, 1fr); gap: 6px var(--sp-3); font-size: var(--fs-sm); }
    dt { color: var(--text-3); }
    dd { margin: 0; overflow-wrap: anywhere; }
    dd.mono { font: var(--fw-regular) 12px / 1.4 var(--font-mono); }
    .sel { user-select: text; cursor: text; }
    .dacts { display: flex; gap: var(--sp-2); flex-wrap: wrap; }
    .hint { margin: 0; color: var(--text-3); font-size: var(--fs-xs); }
    @media (max-width: 1100px) { .layout.with-detail { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class UnusualView {
  protected readonly store = inject(UnusualStore);
  private readonly notifier = inject(Notifier);
  private readonly ports = inject(SHELL_PORTS);
  private readonly dialogs = inject(DialogService);
  private readonly lookups = inject(Lookups);
  protected readonly selectedKey = signal<string | null>(null);
  protected readonly ini = initial;
  protected readonly why = whyText;
  protected readonly selected = computed(() => this.store.overview()?.items.find((i) => i.key === this.selectedKey()) ?? null);
  private readonly now = computed(() => this.store.overview()?.at ?? 0);
  protected readonly learning = computed(() => {
    const o = this.store.overview();
    return o ? learningText(o.learning, o.known, this.now()) : '';
  });

  protected searchWeb(i: UnusualItem): void {
    void this.lookups.searchWeb(i.name, i.name);
  }

  protected searchHash(i: UnusualItem): void {
    if (i.exe) void this.lookups.searchHash({ path: i.exe }, i.name);
  }

  protected tile(name: string): string {
    return `hsl(${hue(name)} 42% 38%)`;
  }

  protected since(i: UnusualItem): string {
    return formatRelative(i.firstSeen, this.now());
  }

  protected ago(at: number): string {
    return formatRelative(at, this.now());
  }

  protected firstSeenFull(i: UnusualItem): string {
    return `${formatDate(i.firstSeen, this.now())}, ${new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit', hour12: false }).format(i.firstSeen)}`;
  }

  protected select(i: UnusualItem): void {
    this.selectedKey.set(i.key);
  }

  protected readonly itemKey = (i: UnusualItem): string => i.key;
  protected readonly columns: PdColumn<UnusualItem>[] = [
    { key: 'name', header: 'Program', value: (i) => i.name, required: true, width: 'minmax(0, 1.4fr)' },
    { key: 'why', header: 'Why', value: (i) => whyText(i), width: 'minmax(0, 2.4fr)', hideBelow: 520 },
    { key: 'firstSeen', header: 'First seen', value: (i) => i.firstSeen, text: (i) => this.since(i), width: '110px' },
    { key: 'state', header: 'State', value: (i) => (i.running ? 'Running' : 'Not running'), width: '100px', hideBelow: 640 },
  ];
  /** The row actions: nothing here changes a program, they copy or look it up. */
  protected readonly actions = (i: UnusualItem): PdRowAction[] => [
    ...(i.exe ? [{ id: 'copy', label: 'Copy path', icon: 'copy' }] : []),
    { id: 'web', label: 'Search the web…', icon: 'search' },
    ...(i.exe ? [{ id: 'hash', label: 'Search file hash on VirusTotal…', icon: 'shield' }] : []),
  ];

  protected run({ id, row }: PdRowActionEvent<UnusualItem>): void {
    this.select(row);
    if (id === 'copy' && row.exe) void this.copy(row.exe);
    else if (id === 'web') this.searchWeb(row);
    else this.searchHash(row);
  }

  protected async forget(): Promise<void> {
    const known = this.store.overview()?.known ?? 0;
    const ok = await confirm(this.dialogs, {
      heading: 'Forget the programs Dyno knows?',
      message: `Dyno forgets the ${known} programs it learned, then learns again for ${this.store.overview()?.learningDays ?? 7} days before it lists anything new.`,
      confirm: { label: 'Forget', destructive: true },
    });
    if (ok) {
      this.selectedKey.set(null);
      await this.store.forget();
    }
  }

  protected async copy(text: string): Promise<void> {
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
