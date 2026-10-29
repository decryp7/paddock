import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { UnusualItem } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Dialogs } from '../../shared/dialog';
import { formatDate, formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { UnusualStore } from '../../state/unusual.store';
import { Lookups } from '../lookup/lookups';
import { hue, initial, learningText, whyText } from './issues-model';

/**
 * Programs that are new on this computer, and why some of them may be worth a look. A transparency tool: it lists reasons
 * ("unusual because it runs from a temporary folder"), never verdicts, and nothing here stops or removes a program. After
 * a quiet learning period Dyno lists only programs it has not seen before.
 */
@Component({
  selector: 'dy-unusual-view',
  imports: [AsyncState, Icon, CdkContextMenuTrigger, CdkMenu, CdkMenuItem, CdkMenuTrigger, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(keydown)': 'key($event)' },
  template: `
    <dy-async [loaded]="!!store.overview()" [error]="store.error()" what="new programs" [rows]="4" (retry)="store.load()">
      @if (store.overview(); as o) {
        <div class="bar" role="group" aria-label="New programs">
          <span class="learn">{{ learning() }}</span>
          <span class="grow"></span>
          <button type="button" class="btn ghost icon" aria-label="More actions" [cdkMenuTriggerFor]="more"><dy-icon name="more" /></button>
        </div>

        @if (!o.enabled) {
          <div class="none"><dy-icon name="eye" /><strong>Watching for new programs is off</strong><span>Turn it on in Settings and Dyno starts learning which programs are normal here.</span><a class="btn" routerLink="/settings">Open settings</a></div>
        } @else if (o.items.length === 0) {
          <div class="none">
            <dy-icon [name]="o.learning.active ? 'clock' : 'check'" />
            <strong>{{ o.learning.active ? 'Dyno is learning what is normal here' : 'Nothing new' }}</strong>
            <span>{{ o.learning.active ? 'It notes the programs on this computer without telling you about each one. After that, it lists only programs it has not seen before.' : 'Dyno knows ' + o.known + ' programs on this computer. A new one appears here, with the reasons it may be worth a look.' }}</span>
          </div>
        } @else {
          <div class="layout" [class.with-detail]="!!selected()">
            <div class="tablewrap">
              <table class="table progs" aria-label="New and unusual programs">
                <thead><tr><th>Program</th><th>Why</th><th class="when">First seen</th><th class="state">State</th></tr></thead>
                <tbody>
                  @for (i of o.items; track i.key) {
                    <tr class="row" tabindex="0" [class.sel]="selectedKey() === i.key" [cdkContextMenuTriggerFor]="menu" [cdkContextMenuTriggerData]="{ item: i }" (click)="select(i)" (keydown.enter)="select(i)" (keydown.shift.f10)="select(i)" (contextmenu)="select(i)">
                      <td>
                        <span class="prog">
                          <span class="av" aria-hidden="true" [style.background]="tile(i.name)">{{ ini(i.name) }}</span>
                          <span class="ell" [title]="i.exe ?? i.name">{{ i.name }}</span>
                        </span>
                      </td>
                      <td><span class="why"><span class="dot" [attr.data-tone]="i.level === 'unusual' ? 'warn' : null" aria-hidden="true"></span><span class="ell" [title]="why(i)">{{ why(i) }}</span></span></td>
                      <td class="when muted">{{ since(i) }}</td>
                      <td class="state muted">{{ i.running ? 'Running' : 'Not running' }}</td>
                    </tr>
                  }
                </tbody>
              </table>
            </div>

            @if (selected(); as s) {
              <aside class="detail" tabindex="-1" aria-label="Program details" (keydown.escape)="selectedKey.set(null)">
                <div class="dhead">
                  <span class="prog"><span class="av" aria-hidden="true" [style.background]="tile(s.name)">{{ ini(s.name) }}</span><strong>{{ s.name }}</strong></span>
                  <button type="button" class="ib" aria-label="Close details" (click)="selectedKey.set(null)"><dy-icon name="close" /></button>
                </div>
                <p class="lead">{{ why(s) }}.</p>
                @if (s.reasons.length > 1) {
                  <ul class="reasons">@for (r of s.reasons; track r) { <li>It {{ r }}.</li> }</ul>
                }
                <dl>
                  @if (s.exe) { <dt>Path</dt><dd class="sel mono" [title]="s.exe">{{ s.exe }}</dd> }
                  @if (s.publisher) { <dt>Publisher</dt><dd class="sel">{{ s.publisher }}</dd> }
                  <dt>First seen</dt><dd>{{ firstSeenFull(s) }}</dd>
                  <dt>Last seen</dt><dd>{{ s.lastSeen > 0 ? (s.running ? 'Running now' : ago(s.lastSeen)) : 'Has not run since' }}</dd>
                </dl>
                <div class="dacts">
                  @if (s.exe) { <button type="button" class="btn sm" (click)="copy(s.exe)"><dy-icon name="copy" /> Copy path</button> }
                </div>
                <p class="hint">Dyno never ends or removes a program because of what is shown here.</p>
              </aside>
            }
          </div>
        }
      }
    </dy-async>

    <ng-template #menu let-item="item">
      <div class="menu" cdkMenu aria-label="Program actions">
        @if (item.exe) { <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="copy(item.exe)"><dy-icon name="copy" /> Copy path</button> }
        <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="searchWeb(item)"><dy-icon name="search" /> Search the web…</button>
        @if (item.exe) { <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="searchHash(item)"><dy-icon name="shield" /> Search file hash on VirusTotal…</button> }
      </div>
    </ng-template>
    <ng-template #more>
      <div class="menu" cdkMenu aria-label="More actions">
        <button type="button" class="menu-item danger" cdkMenuItem (cdkMenuItemTriggered)="forget()"><dy-icon name="trash" /> Forget the programs Dyno knows…</button>
      </div>
    </ng-template>
  `,
  styles: `
    :host { display: grid; gap: var(--sp-3); align-content: start; }
    dy-async { display: grid; gap: var(--sp-3); }
    .bar { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; min-height: 44px; }
    .grow { flex: 1; }
    .learn { color: var(--text-2); font-size: var(--fs-sm); }
    .tog { display: inline-flex; align-items: center; gap: 4px; height: var(--control-h); padding: 0 10px; border: 1px solid var(--border-strong); border-radius: var(--r-pill); background: var(--surface-2); color: var(--text-2); font: var(--fw-medium) var(--fs-sm) / 1 var(--font-sans); cursor: pointer; }
    .tog dy-icon { --icon-size: 13px; display: none; }
    .tog[aria-pressed='true'] { background: var(--accent-soft); border-color: var(--accent); color: var(--accent-text); }
    .tog[aria-pressed='true'] dy-icon { display: inline-block; }
    .layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-3); align-items: start; }
    .layout.with-detail { grid-template-columns: minmax(0, 1fr) 340px; }
    .tablewrap { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); overflow: auto; max-height: 620px; min-width: 0; }
    .progs { table-layout: fixed; }
    .progs th.when, .progs td.when { width: 110px; }
    .progs th.state, .progs td.state { width: 100px; }
    .progs td { overflow: hidden; text-overflow: ellipsis; }
    .progs th:first-child, .progs td:first-child { width: 28%; }
    .prog { display: flex; align-items: center; gap: var(--sp-2); min-width: 0; }
    .av { flex: none; display: grid; place-items: center; width: 22px; height: 22px; border-radius: 6px; color: #fff; font: var(--fw-semibold) 12px / 1 var(--font-sans); }
    .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .why { display: flex; align-items: center; gap: var(--sp-2); min-width: 0; }
    .why .dot:not([data-tone]) { background: transparent; }
    tr.row { cursor: default; }
    tr.row:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
    tr.row.sel td { background: var(--accent-soft); }
    tr.row.sel td:first-child { box-shadow: inset 2px 0 0 var(--accent); }
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
    .menu-item.danger { color: var(--bad); }
    @media (max-width: 1100px) { .layout.with-detail { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class UnusualView {
  protected readonly store = inject(UnusualStore);
  private readonly notifier = inject(Notifier);
  private readonly dialogs = inject(Dialogs);
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

  /** Up and Down move between rows. */
  protected key(e: KeyboardEvent): void {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const rows = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('tr.row')];
    const at = rows.indexOf(document.activeElement as HTMLElement);
    const next = rows[at + (e.key === 'ArrowDown' ? 1 : -1)];
    if (next) {
      e.preventDefault();
      next.focus();
    }
  }

  protected async forget(): Promise<void> {
    const known = this.store.overview()?.known ?? 0;
    const ok = await this.dialogs.confirm({
      title: 'Forget the programs Dyno knows?',
      message: `Dyno forgets the ${known} programs it learned, then learns again for ${this.store.overview()?.learningDays ?? 7} days before it lists anything new.`,
      confirmLabel: 'Forget',
      danger: true,
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
}
