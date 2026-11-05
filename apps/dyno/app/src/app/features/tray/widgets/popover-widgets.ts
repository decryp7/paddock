import { PdViewportFollowsSize } from '@decryp7/paddock-ui/table';
import { CdkFixedSizeVirtualScroll, CdkVirtualForOf, CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, ViewEncapsulation, input, output, viewChildren } from '@angular/core';
import { PdSkeleton, PdWidget, relativeAgo } from '@decryp7/paddock-ui/widgets';
import { PdGlyph, PdStatusGlyph, type PdStatus } from '@decryp7/paddock-ui/icons';
import type { AlarmRow, AppRow, BackupValue, CounterRow, IssueValue, Level, Page, PopoverValue, VolumeRow, WidgetActionId } from '../../../core/generated/widgets';
import { FullTextTip } from '../../../shared/full-text-tip';
import { formatBytes, formatPercent, formatRate, splitValue } from '../../../shared/format';
import { Sparkline } from '../../../shared/sparkline';
import { formatCounter } from '../../counters/counters-model';

type Value<K extends string> = Extract<PopoverValue, Record<K, unknown>>[K];

const LEVEL_STATUS: Record<Level, PdStatus> = { info: 'info', warning: 'warning', critical: 'error' };
const LEVEL_WORD: Record<Level, string> = { info: 'Info', warning: 'Warning', critical: 'Critical' };

/** Shapes drawn while a widget is loading: the same rows as a ready one, so the heights agree (their text is hidden). */
const SK_ALARMS: AlarmRow[] = [1, 2].map((id) => ({ id, title: 'Memory pressure is high', level: 'warning', since: 0, acknowledged: false }));
const SK_VOLUMES: VolumeRow[] = [1, 2, 3].map((n) => ({ id: `${n}`, name: 'Macintosh HD', usedPercent: 50, freeBytes: 1, totalBytes: 2, tone: 'ok', reachable: true }));
const SK_COUNTERS: CounterRow[] = [1, 2, 3, 4].map((n) => ({ path: `${n}`, name: 'GPU memory in use', value: 1, unit: 'bytes', percent: null, tone: 'ok' }));
const SK_APPS: AppRow[] = [1, 2, 3].map((n) => ({ name: `App ${n}`, cpuPercent: 10 }));
const SK_ISSUE: IssueValue = { title: 'Disk write errors on disk4', app: '', kind: '', level: 'warning', at: 0, days: 7 };
const SK_BACKUP: BackupValue = { name: 'Time Machine', state: 'healthy', lastBackup: 0, summary: '', tools: 1 };

/**
 * Dyno's template for each popup widget kind (`*pdWidget`), drawn inside `pd-widget-frame`. One set serves the popup and the
 * Settings preview (`pd-widget-area [templates]`). What a widget shows, its order and its tones come from the engine's
 * values; these templates only lay them out. A row that opens a page is one real button with a chevron.
 */
@Component({
  selector: 'dy-popover-widgets',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [PdWidget, PdSkeleton, PdGlyph, PdStatusGlyph, Sparkline, FullTextTip, CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf, PdViewportFollowsSize],
  template: `
    <ng-template pdWidget="activeAlarms" [pdWidgetOf]="witness" let-value>
      @let a = alarms(value);
      @let rows = a?.rows ?? skAlarms;
      <cdk-virtual-scroll-viewport pdViewportFollowsSize class="dy-pw-viewport" [itemSize]="alarmRowPx" role="list" tabindex="0" aria-label="Active alarms" [style.--dy-n]="rows.length">
        <div *cdkVirtualFor="let r of rows; trackBy: alarmId; let n = index" class="dy-pw-item" role="listitem" [attr.aria-setsize]="rows.length" [attr.aria-posinset]="n + 1">
          <button type="button" class="dy-pw-row dy-pw-alarm" (click)="open('alarms')">
            <span class="dy-pw-lvl" [attr.data-level]="r.level" pdSkeleton><pd-status-glyph [status]="status(r.level)" [size]="14" />{{ word(r.level) }}</span>
            <span class="dy-pw-grow" [dyFullText]="r.title" pdSkeleton>{{ r.title }}</span>
            <span class="dy-pw-muted" pdSkeleton>{{ ago(r.since) }}</span>
            <pd-glyph class="dy-pw-chev" name="chevron-right" [size]="14" />
          </button>
        </div>
      </cdk-virtual-scroll-viewport>
      @if (a && a.more > 0) {
        <p class="dy-pw-muted dy-pw-note">Showing the {{ a.rows.length }} most severe of {{ a.rows.length + a.more }}.</p>
        <button type="button" class="dy-pw-more" (click)="open('alarms')">Open alarms</button>
      }
    </ng-template>

    <ng-template pdWidget="diskSpace" [pdWidgetOf]="witness" let-value>
      @let d = disks(value);
      <ul class="dy-pw-list dy-pw-scroll" tabindex="0" aria-label="Volumes">
        @for (v of d?.rows ?? skVolumes; track v.id) {
          <li class="dy-pw-vol" [attr.data-tone]="v.tone">
            <span class="dy-pw-vname"><span class="dy-pw-grow" pdSkeleton>{{ v.name }}</span>
              @if (v.tone !== 'ok') { <span class="dy-pw-flag" [attr.data-tone]="v.tone"><pd-status-glyph [status]="v.tone === 'bad' ? 'error' : 'warning'" [size]="14" />{{ v.tone === 'bad' ? 'Full' : 'Low' }}</span> }
              <span class="dy-pw-muted" pdSkeleton>{{ !v.reachable ? 'Not answering' : v.freeBytes === null ? 'Unavailable' : free(v) }}</span></span>
            <span class="dy-pw-meter" role="img" [attr.aria-label]="v.usedPercent === null ? 'Used space unavailable' : pct(v.usedPercent) + ' used'" pdSkeleton><i [style.width.%]="v.usedPercent ?? 0"></i></span>
          </li>
        }
      </ul>
      @if (d && d.more > 0) {
        <p class="dy-pw-muted dy-pw-note">Showing the first {{ d.rows.length }} of {{ d.rows.length + d.more }} volumes.</p>
      }
    </ng-template>

    <ng-template pdWidget="cpuMemory" [pdWidgetOf]="witness" let-value>
      @let c = cpuMem(value);
      <div class="dy-pw-two">
        <div class="dy-pw-half" [attr.data-tone]="c?.cpuTone ?? 'ok'">
          <span class="dy-pw-muted" pdSkeleton>CPU</span>
          <span class="dy-pw-big" pdSkeleton>{{ big(c?.cpuPercent ?? 0) }}<small>%</small></span>
          <dy-sparkline [values]="c?.cpuMinute ?? []" [max]="100" color="var(--accent)" label="CPU over the last minute" style="--spark-h: 24px" />
        </div>
        <div class="dy-pw-half dy-pw-half--next" [attr.data-tone]="c?.memoryTone ?? 'ok'">
          <span class="dy-pw-muted" pdSkeleton>Memory {{ memText(c) }}</span>
          <span class="dy-pw-big" pdSkeleton>{{ big(c?.memoryPercent ?? 0) }}<small>%</small></span>
          <span class="dy-pw-meter dy-pw-tall" role="img" [attr.aria-label]="'Memory in use ' + big(c?.memoryPercent ?? 0) + '%'" pdSkeleton><i [style.width.%]="c?.memoryPercent ?? 0"></i></span>
        </div>
      </div>
    </ng-template>

    <ng-template pdWidget="network" [pdWidgetOf]="witness" let-value>
      @let n = net(value);
      <button type="button" class="dy-pw-plain" (click)="open('network')">
        <span class="dy-pw-big" pdSkeleton>↓ {{ rate(n?.rxBps ?? 0)[0] }}<small>{{ rate(n?.rxBps ?? 0)[1] }}</small></span>
        <span class="dy-pw-muted" pdSkeleton>↑ {{ rate(n?.txBps ?? 0).join(' ') }}</span>
        <dy-sparkline [values]="n?.rxMinute ?? []" [values2]="n?.txMinute ?? null" color2="var(--text-3)" label="Network over the last minute" style="--spark-h: 28px" />
      </button>
    </ng-template>

    <ng-template pdWidget="backupHealth" [pdWidgetOf]="witness" let-value>
      @let b = backup(value) ?? skBackup;
      <button type="button" class="dy-pw-plain" (click)="open('backups')">
        <span class="dy-pw-flag" [attr.data-state]="b.state" pdSkeleton><pd-status-glyph [status]="b.state === 'healthy' ? 'success' : 'warning'" [size]="14" />{{ b.state === 'healthy' ? 'OK' : b.state === 'stale' ? 'Out of date' : 'No backup yet' }}</span>
        <span class="dy-pw-strong" pdSkeleton>{{ backupLine(backup(value)) }}</span>
        <span class="dy-pw-muted" pdSkeleton>{{ b.name }}</span>
      </button>
    </ng-template>

    <ng-template pdWidget="lastIssue" [pdWidgetOf]="witness" let-value>
      @let i = issue(value) ?? skIssue;
      <button type="button" class="dy-pw-row" (click)="open('issues')">
        <span class="dy-pw-lvl" [attr.data-level]="i.level" pdSkeleton><pd-status-glyph [status]="status(i.level)" [size]="14" />{{ word(i.level) }}</span>
        <span class="dy-pw-grow" pdSkeleton>{{ i.title }}</span>
        <span class="dy-pw-muted" pdSkeleton>{{ ago(i.at) }}</span>
        <pd-glyph class="dy-pw-chev" name="chevron-right" [size]="14" />
      </button>
    </ng-template>

    <ng-template pdWidget="pinnedCounters" [pdWidgetOf]="witness" let-value>
      @let p = counters(value);
      <ul class="dy-pw-list" aria-label="Pinned counters">
        @for (r of p?.rows ?? skCounters; track r.path) {
          <li class="dy-pw-line" [attr.data-tone]="r.tone"><span class="dy-pw-grow" pdSkeleton>{{ r.name }}</span>
            <span class="dy-pw-val" [class.dy-pw-muted]="r.value === null" pdSkeleton>
              @if (r.tone !== 'ok') { <pd-status-glyph [status]="r.tone === 'bad' ? 'error' : 'warning'" [size]="14" /> }
              {{ r.value === null ? 'Unavailable' : counter(r) }}</span></li>
        }
      </ul>
    </ng-template>

    <ng-template pdWidget="topApps" [pdWidgetOf]="witness" let-value>
      @let t = apps(value);
      <ul class="dy-pw-list" aria-label="Busiest apps">
        @for (r of t?.rows ?? skApps; track r.name) {
          <li class="dy-pw-line"><span class="dy-pw-grow" pdSkeleton>{{ r.name }}</span><span pdSkeleton>{{ r.cpuPercent === null ? 'Unavailable' : pct(r.cpuPercent) }}</span></li>
        }
      </ul>
    </ng-template>
  `,
  styles: `
    /* One track that may shrink: an auto track takes the widest row's nowrap content and pushes the rows past the card. */
    .dy-pw-list { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 4px; }
    .dy-pw-row, .dy-pw-plain, .dy-pw-more { all: unset; box-sizing: border-box; cursor: pointer; border-radius: var(--pd-radius-control); }
    .dy-pw-row { display: flex; align-items: center; gap: 6px; width: 100%; min-width: 0; max-width: 100%; min-height: 24px; color: var(--pd-text); font: inherit; }
    .dy-pw-list > li { min-width: 0; }
    /* The alarm list scrolls inside a viewport as tall as its rows, up to 208 px (the stopped-agent popup must still fit the 520 px cap, tray-fit e2e); the popover stays as tall as that and no taller, and the wheel passes on at the ends. */
    .dy-pw-viewport { height: min(calc(var(--dy-n, 1) * 44px), 208px); min-height: 44px; }
    .dy-pw-viewport:focus-visible, .dy-pw-scroll:focus-visible { outline-offset: -2px; }
    .dy-pw-item { box-sizing: border-box; height: 44px; padding-bottom: 4px; }
    .dy-pw-scroll { max-height: 132px; overflow-y: auto; }
    /* An alarm is two lines: its level and age on the first, its title across the whole width on the second, cut to one line (a title squeezed beside the level wrapped to a column of words). */
    .dy-pw-alarm { height: 40px; display: grid; grid-template-columns: auto 1fr auto auto; grid-template-areas: 'lvl . ago chev' 'title title title title'; row-gap: 2px; align-items: center; }
    .dy-pw-alarm > .dy-pw-lvl { grid-area: lvl; justify-self: start; }
    .dy-pw-alarm > .dy-pw-muted { grid-area: ago; }
    .dy-pw-alarm > .dy-pw-chev { grid-area: chev; }
    .dy-pw-alarm > .dy-pw-grow { grid-area: title; }
    .dy-pw-chev { flex: none; color: var(--pd-text-muted); }
    .dy-pw-flag[data-tone='warn'] { background: var(--pd-warning-tint); } .dy-pw-flag[data-tone='bad'] { background: var(--pd-danger-tint); }
    .dy-pw-plain { display: grid; gap: 4px; width: 100%; }
    .dy-pw-row:focus-visible, .dy-pw-plain:focus-visible, .dy-pw-more:focus-visible { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: 2px; }
    .dy-pw-more { display: block; margin-top: 2px; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .dy-pw-grow { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .dy-pw-muted { color: var(--pd-text-muted); font-size: var(--pd-text-label); white-space: nowrap; }
    .dy-pw-note { margin: 4px 0 0; }
    .dy-pw-strong { font-weight: var(--pd-weight-semibold); }
    .dy-pw-lvl, .dy-pw-flag { display: inline-flex; align-items: center; gap: 4px; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); padding: 0 6px; border-radius: var(--pd-radius-pill, 999px); white-space: nowrap; }
    .dy-pw-lvl[data-level='info'] { background: var(--pd-info-tint); } .dy-pw-lvl[data-level='warning'], .dy-pw-flag[data-state='stale'] { background: var(--pd-warning-tint); }
    .dy-pw-lvl[data-level='critical'] { background: var(--pd-danger-tint); } .dy-pw-flag[data-state='healthy'] { background: var(--pd-success-tint); }
    .dy-pw-vol { display: grid; gap: 4px; }
    .dy-pw-vname { display: flex; align-items: center; gap: 8px; }
    .dy-pw-meter { display: block; height: 4px; border-radius: 2px; background: var(--pd-surface-3); overflow: hidden; }
    .dy-pw-meter i { display: block; height: 100%; background: var(--pd-meter-neutral); }
    .dy-pw-vol[data-tone='warn'] .dy-pw-meter i { background: var(--pd-warning); } .dy-pw-vol[data-tone='bad'] .dy-pw-meter i { background: var(--pd-danger); }
    .dy-pw-half > .dy-pw-muted { overflow: hidden; text-overflow: ellipsis; }
    .dy-pw-tall { height: 4px; align-self: end; margin-bottom: 12px; }
    .dy-pw-half--next { border-inline-start: 1px solid var(--pd-hairline); padding-inline-start: 12px; }
    .dy-pw-two { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
    .dy-pw-half > .dy-pw-muted { white-space: nowrap; }
    /* The rows keep their own height when a neighbour half is taller (a sparkline beside a meter), so the big numbers share a baseline. */
    .dy-pw-half { display: grid; align-content: start; gap: 2px; min-width: 0; }
    .dy-pw-big { font-size: 22px; font-weight: var(--pd-weight-semibold); font-variant-numeric: tabular-nums; line-height: 1.2; }
    .dy-pw-big small { font-size: var(--pd-text-label); font-weight: var(--pd-weight-regular); color: var(--pd-text-muted); margin-left: 2px; }
    .dy-pw-val { display: inline-flex; align-items: center; gap: 4px; }
    .dy-pw-line[data-tone='warn'] .dy-pw-val { color: var(--pd-warning); } .dy-pw-line[data-tone='bad'] .dy-pw-val { color: var(--pd-danger); }
    .dy-pw-line { display: flex; justify-content: space-between; gap: 8px; font-variant-numeric: tabular-nums; }
  `,
})
export class DynoPopoverWidgets {
  /** The templates, for `pd-widget-area [templates]`. */
  readonly templates = viewChildren<PdWidget<PopoverValue>>(PdWidget);
  /** A widget's button (open a page). */
  readonly action = output<WidgetActionId>();
  /** The clock for relative times; null reads the system clock. */
  readonly now = input<number | null>(null);

  /** The type witness of every template: what the area hands each one (`pdWidgetOf`). */
  protected readonly witness: PopoverValue | null = null;
  protected readonly skAlarms = SK_ALARMS;
  /** An alarm's row height in the virtual viewport, in px (the row, 40, and the gap below it, 4). */
  protected readonly alarmRowPx = 44;
  protected readonly alarmId = (_: number, r: AlarmRow) => r.id;
  protected readonly skVolumes = SK_VOLUMES;
  protected readonly skCounters = SK_COUNTERS;
  protected readonly skApps = SK_APPS;
  protected readonly skIssue = SK_ISSUE;
  protected readonly skBackup = SK_BACKUP;

  protected readonly pct = formatPercent;
  protected readonly bytes = formatBytes;
  protected status(l: Level): PdStatus {
    return LEVEL_STATUS[l];
  }
  protected word(l: Level): string {
    return LEVEL_WORD[l];
  }
  /** "16.3 of 32 GB": the used amount without its unit when the total has the same one. */
  protected memText(c: { memoryUsed: number; memoryTotal: number } | null): string {
    if (!c) return '16.3 of 32 GB';
    const [used, total] = [formatBytes(c.memoryUsed), formatBytes(c.memoryTotal)];
    const unit = total.split(' ')[1];
    return unit !== undefined && used.endsWith(` ${unit}`) ? `${used.slice(0, -unit.length - 1)} of ${total}` : `${used} of ${total}`;
  }
  protected backupLine(b: BackupValue | null): string {
    if (b === null) return 'Last backup 2 h ago';
    return b.lastBackup !== null ? `Last backup ${this.ago(b.lastBackup)}` : b.summary;
  }
  protected ago(ms: number): string {
    return relativeAgo(this.now() ?? Date.now(), ms);
  }
  protected big(v: number): string {
    return Math.round(v).toString();
  }
  protected rate(bps: number): [string, string] {
    return splitValue(formatRate(bps));
  }
  protected free(v: VolumeRow): string {
    return `${formatBytes(v.freeBytes)} free of ${formatBytes(v.totalBytes)}`;
  }
  protected counter(r: CounterRow): string {
    return formatCounter(r.value ?? 0, r.unit).join(' ');
  }
  protected open(page: Page): void {
    this.action.emit({ openPage: page });
  }

  private pick<K extends string>(v: PopoverValue | null, key: K): Value<K> | null {
    return v !== null && typeof v === 'object' && key in v ? (v as unknown as Record<K, Value<K>>)[key] : null;
  }
  protected readonly alarms = (v: PopoverValue | null) => this.pick(v, 'activeAlarms');
  protected readonly disks = (v: PopoverValue | null) => this.pick(v, 'diskSpace');
  protected readonly cpuMem = (v: PopoverValue | null) => this.pick(v, 'cpuMemory');
  protected readonly net = (v: PopoverValue | null) => this.pick(v, 'network');
  protected readonly backup = (v: PopoverValue | null) => this.pick(v, 'backupHealth');
  protected readonly issue = (v: PopoverValue | null) => this.pick(v, 'lastIssue');
  protected readonly counters = (v: PopoverValue | null) => this.pick(v, 'pinnedCounters');
  protected readonly apps = (v: PopoverValue | null) => this.pick(v, 'topApps');
}
