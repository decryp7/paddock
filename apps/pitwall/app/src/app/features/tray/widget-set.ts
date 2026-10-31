import { ChangeDetectionStrategy, Component, computed, inject, output, viewChildren } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { MatTooltip } from '@angular/material/tooltip';
import { PdSkeleton, PdWidget } from '@decryp7/paddock-ui/widgets';
import type {
  EngineHealth,
  MemoryLevel,
  MeterTone,
  ModelRowState,
  PopoverValue,
  PopoverWidget,
  ServerState,
  WidgetActionId,
} from '../../core/generated/widgets';
import type { PopoverState } from '../../core/widgets.types';
import { ago, bytes, int } from '../../shared/format';
import { injectNow } from '../../shared/now';
import { TrayWidgetsStore } from './tray-widgets.store';

type Of<K extends PopoverValue['kind']> = Extract<PopoverValue, { kind: K }>;

/** The words and icons of the server's state: status is never colour alone, so every state has both. */
const SERVER: Record<ServerState, { icon: string; word: string }> = {
  serving: { icon: 'pw:success', word: 'Serving' },
  paused: { icon: 'pw:pause', word: 'Paused' },
  stopped: { icon: 'pw:stop', word: 'Stopped' },
  unreachable: { icon: 'pw:error', word: 'No answer' },
};

const MODEL: Record<ModelRowState, { icon: string; word: string }> = {
  ready: { icon: 'pw:success', word: 'Ready' },
  busy: { icon: 'pw:bolt', word: 'Busy' },
  loading: { icon: 'pw:refresh', word: 'Loading' },
  releasing: { icon: 'pw:release', word: 'Releasing' },
  failed: { icon: 'pw:error', word: 'Failed' },
};

const PRESSURE: Record<MemoryLevel, string | null> = { normal: null, elevated: 'Memory is tight', critical: 'Memory is critical' };

/**
 * Pitwall's templates for the popup's widget kinds (family ADR 0035): one `ng-template` per kind, drawn by `pd-widget-area`
 * inside a `pd-widget-frame`, which owns the head and the loading, working, empty, error and stale states. The same set draws
 * the popup and the Settings page's preview. A template draws the engine's value and decides nothing: which row comes first,
 * which tone a meter has and whether a state is calm are all the engine's; what a button does is said as a [`WidgetActionId`]
 * on `run`, and the host runs it.
 */
@Component({
  selector: 'pw-widget-set',
  imports: [MatIcon, MatTooltip, PdSkeleton, PdWidget],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <ng-template pdWidget="server" [pdWidgetOf]="serverValue" let-v>
      <div class="block">
        <span class="big" [matTooltip]="v ? server(v.state).word : ''" pdSkeleton>
          <mat-icon class="ic-inline" [svgIcon]="v ? server(v.state).icon : 'pw:success'" [class]="'ic-inline ' + (v ? 'st-' + v.state : '')" aria-hidden="true" />{{ v ? server(v.state).word : 'Serving' }}
        </span>
        <span class="meta" pdSkeleton>
          <mat-icon class="ic-inline" [svgIcon]="engineIcon(v?.engine)" aria-hidden="true" />{{ engineText(v?.engine) }}
        </span>
      </div>
    </ng-template>

    <ng-template pdWidget="activity" [pdWidgetOf]="activityValue" let-v>
      <div class="block">
        <span class="big" pdSkeleton>{{ v ? int(v.tokensPerSec) : '38' }} <span class="unit">tok/s</span></span>
        <span class="meta truncate" pdSkeleton>{{ v ? int(v.running) : '1' }} running · {{ v ? int(v.waiting) : '0' }} waiting</span>
      </div>
    </ng-template>

    <ng-template pdWidget="memory" [pdWidgetOf]="memoryValue" let-v>
      <div class="meters">
        <div class="mrow">
          <span class="k" pdSkeleton>RAM</span>
          <span class="v" pdSkeleton>{{ v ? size(v.used) + ' of ' + size(v.total, 0) : '9.4 GB of 32 GB' }}@if (v && fill(v.tone); as w) {&nbsp;<span class="fill" [attr.data-tone]="v.tone">· {{ w }}</span>}</span>
          <span class="meter" [attr.data-tone]="v?.tone ?? 'neutral'" pdSkeleton><span [style.width.%]="v?.percent ?? 30"></span></span>
        </div>
        <div class="mrow">
          <span class="k" pdSkeleton>VRAM</span>
          @let g = v?.vram;
          @if (!v || g) {
            <span class="v" pdSkeleton>{{ g ? size(g.used) + ' of ' + size(g.total, 0) : '7.0 GB of 12 GB' }}@if (g && fill(g.tone); as w) {&nbsp;<span class="fill" [attr.data-tone]="g.tone">· {{ w }}</span>}</span>
            <span class="meter" [attr.data-tone]="g?.tone ?? 'neutral'" pdSkeleton><span [style.width.%]="g?.percent ?? 50"></span></span>
          } @else {
            <span class="v muted">No separate video memory</span>
          }
        </div>
      </div>
    </ng-template>

    <ng-template pdWidget="loadedModels" [pdWidgetOf]="modelsValue" let-v>
      <ul class="rows">
        @for (r of v?.rows ?? placeholders('loadedModels'); track $index) {
          <li>
            <button type="button" class="row" [disabled]="!r" [attr.aria-label]="r ? r.name + ', ' + model(r.state).word + ', ' + size(r.bytes) + ', open Models' : null" (click)="r && run.emit({ type: 'openPage', page: 'models' })">
              <mat-icon class="ic" [class]="'ic ' + (r ? 'st-' + r.state : '')" [svgIcon]="r ? model(r.state).icon : 'pw:models'" aria-hidden="true" />
              <span class="name truncate" [matTooltip]="r?.name ?? ''" pdSkeleton>{{ r?.name ?? 'Model name' }}</span>
              <span class="meta truncate" pdSkeleton>{{ r ? size(r.bytes) + ' · ' + model(r.state).word + (r.pinned ? ' · Pinned' : '') : '5.0 GB · Ready' }}</span>
              <mat-icon class="chev" svgIcon="pw:chevron-right" aria-hidden="true" />
            </button>
          </li>
        }
      </ul>
      @if (v && v.more > 0) {
        <p class="more">+{{ v.more }} more</p>
      }
    </ng-template>

    <ng-template pdWidget="downloads" [pdWidgetOf]="downloadsValue" let-v>
      <ul class="rows">
        @for (r of v?.rows ?? placeholders('downloads'); track $index) {
          <li>
            <button type="button" class="row" [disabled]="!r" [attr.aria-label]="r ? r.name + ', ' + (r.state === 'failed' ? 'failed' : 'paused') + ', open Models' : null" (click)="r && run.emit({ type: 'openPage', page: 'models' })">
              <mat-icon class="ic" [class]="'ic ' + (r?.state === 'failed' ? 'st-failed' : '')" [svgIcon]="r?.state === 'failed' ? 'pw:error' : 'pw:pause'" aria-hidden="true" />
              <span class="name truncate" [matTooltip]="r?.name ?? ''" pdSkeleton>{{ r?.name ?? 'Model name' }}</span>
              <span class="meta truncate" pdSkeleton>{{ r ? (r.state === 'failed' ? 'Failed' + (r.message ? ': ' + r.message : '') : 'Paused: resumes where it stopped') : 'Paused: resumes where it stopped' }}</span>
              <mat-icon class="chev" svgIcon="pw:chevron-right" aria-hidden="true" />
            </button>
          </li>
        }
      </ul>
    </ng-template>

    <ng-template pdWidget="updates" [pdWidgetOf]="updatesValue" let-v>
      <button type="button" class="block" [disabled]="!v" [attr.aria-label]="v ? count(v.count, 'model update') + ' available, open Models' : null" (click)="v && run.emit({ type: 'openPage', page: 'models' })">
        <span class="big" pdSkeleton>{{ v ? count(v.count, 'update') : '2 updates' }}</span>
        <span class="meta truncate" [matTooltip]="v?.summary ?? ''" pdSkeleton>{{ v?.summary ?? 'A better quantisation is available.' }}</span>
        <mat-icon class="chev" svgIcon="pw:chevron-right" aria-hidden="true" />
      </button>
    </ng-template>

    <ng-template pdWidget="recentErrors" [pdWidgetOf]="errorsValue" let-v>
      <ul class="rows">
        @for (r of v?.rows ?? placeholders('recentErrors'); track $index) {
          <li>
            <button type="button" class="row" [disabled]="!r" [attr.aria-label]="r ? 'Error, ' + r.message + ', ' + ago(r.atMs, now()) + ', open Requests' : null" (click)="r && run.emit({ type: 'openPage', page: 'requests' })">
              <mat-icon class="ic st-failed" svgIcon="pw:error" aria-hidden="true" />
              <span class="name truncate" [matTooltip]="r?.message ?? ''" pdSkeleton>{{ r?.message ?? 'A request failed' }}</span>
              <span class="meta truncate" pdSkeleton>{{ r ? ago(r.atMs, now()) : '12 min ago' }}</span>
              <mat-icon class="chev" svgIcon="pw:chevron-right" aria-hidden="true" />
            </button>
          </li>
        }
      </ul>
    </ng-template>
  `,
  styles: `
    :host {
      display: contents;
    }
    .rows {
      display: grid;
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .row {
      display: grid;
      grid-template-columns: 16px minmax(0, 1fr) 14px;
      column-gap: var(--pd-space-2);
      align-items: center;
      min-height: 30px;
      /* The icon sits on the widget head's left edge; the hover tint reaches past it. */
      width: calc(100% + 2 * var(--pd-space-1));
      margin: 0 calc(-1 * var(--pd-space-1));
      padding: 1px var(--pd-space-1);
      border: 0;
      border-radius: var(--pd-radius-control);
      background: transparent;
      color: var(--pd-text);
      font: inherit;
      text-align: left;
    }
    .row:not(:disabled):hover,
    button.block:not(:disabled):hover {
      background: var(--pd-hover);
    }
    .row:focus-visible,
    .block:focus-visible {
      outline: var(--pd-focus-width) solid var(--pd-focus-ring);
      outline-offset: 1px;
    }
    .ic {
      grid-row: 1 / span 2;
      width: 16px;
      height: 16px;
      color: var(--pd-text-muted);
    }
    .chev {
      grid-column: 3;
      grid-row: 1 / span 2;
      width: 14px;
      height: 14px;
      color: var(--pd-text-subtle);
    }
    .name {
      font-weight: var(--pd-weight-medium);
    }
    .meta {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .row .meta {
      grid-column: 2;
    }
    .st-ready,
    .st-serving {
      color: var(--pd-success);
    }
    .st-busy,
    .st-loading,
    .st-releasing {
      color: var(--pd-info);
    }
    .st-paused,
    .st-stopped {
      color: var(--pd-warning);
    }
    .st-failed,
    .st-unreachable {
      color: var(--pd-danger);
    }
    .more {
      margin: 0;
      padding: var(--pd-space-1);
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .block {
      display: grid;
      grid-template-columns: minmax(0, 1fr);
      gap: 0;
      width: 100%;
      padding: 0;
      border: 0;
      border-radius: var(--pd-radius-control);
      background: transparent;
      color: var(--pd-text);
      font: inherit;
      text-align: left;
    }
    button.block {
      grid-template-columns: minmax(0, 1fr) 14px;
      align-items: center;
    }
    button.block .chev {
      grid-column: 2;
      grid-row: 1 / span 2;
    }
    .block .big,
    .block .meta {
      grid-column: 1;
      min-width: 0;
    }
    /* The word never leaves its widget: at half width an unexpectedly long one is cut with an ellipsis and has a tooltip. */
    .big {
      display: block;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      font-size: var(--pd-text-title);
      font-weight: var(--pd-weight-semibold);
      white-space: nowrap;
    }
    .unit {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-regular);
    }
    .ic-inline {
      width: 14px;
      height: 14px;
      margin-right: var(--pd-space-1);
      vertical-align: -2px;
    }
    .meters {
      display: grid;
      grid-template-columns: 36px max-content minmax(40px, 1fr);
      align-items: center;
      column-gap: var(--pd-space-2);
    }
    .mrow {
      display: contents;
    }
    .mrow > * {
      min-height: 24px;
      display: flex;
      align-items: center;
    }
    .mrow > .meter {
      min-height: 6px;
      height: 6px;
      align-self: center;
    }
    .mrow .k {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-label);
    }
    .mrow .v {
      font-weight: var(--pd-weight-medium);
      white-space: nowrap;
    }
    .mrow .v.muted {
      grid-column: 2 / span 2;
      color: var(--pd-text-muted);
      font-weight: var(--pd-weight-regular);
      font-size: var(--pd-text-caption);
    }
    .fill {
      font-weight: var(--pd-weight-regular);
      font-size: var(--pd-text-caption);
      color: var(--pd-text-muted);
    }
    .fill[data-tone='bad'] {
      color: var(--pd-danger);
    }
    .meter {
      display: block;
      height: 6px;
      overflow: hidden;
      border-radius: var(--pd-radius-pill);
      background: var(--pd-pressed);
    }
    .meter > span {
      display: block;
      height: 100%;
      background: var(--pd-meter-neutral);
    }
    .meter[data-tone='warn'] > span {
      background: var(--pd-warning);
    }
    .meter[data-tone='bad'] > span {
      background: var(--pd-danger);
    }
    .truncate {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  `,
})
export class WidgetSet {
  /** What a button asks the app to do. */
  readonly run = output<WidgetActionId>();
  protected readonly now = injectNow(30_000);
  private readonly store = inject(TrayWidgetsStore);

  /** The kinds' templates, for `pd-widget-area`'s `templates` input (the popup and the preview share one set). */
  private readonly dirs = viewChildren(PdWidget);
  readonly widgets = computed(() => this.dirs() as readonly PdWidget<PopoverValue>[]);

  /** The head's meta text of a ready widget ("8.4 GB in memory", "2 new"): the engine's figures, worded. */
  readonly meta = (_id: string, state: PopoverState): string | null => {
    if (state.state !== 'ready') return null;
    const v = state.value;
    if (v.kind === 'memory') return PRESSURE[v.pressure];
    if (v.kind === 'loadedModels') return `${this.count(v.total, 'model')} · ${bytes(v.bytes)}`;
    if (v.kind === 'recentErrors') return `${v.rows.length + v.more} in the last day`;
    if (v.kind === 'downloads') return `${v.rows.length + v.more} paused or failed`;
    return null;
  };

  // Type witnesses for the templates' `let-v` (never read).
  protected readonly serverValue: Of<'server'> | null = null;
  protected readonly activityValue: Of<'activity'> | null = null;
  protected readonly memoryValue: Of<'memory'> | null = null;
  protected readonly modelsValue: Of<'loadedModels'> | null = null;
  protected readonly downloadsValue: Of<'downloads'> | null = null;
  protected readonly updatesValue: Of<'updates'> | null = null;
  protected readonly errorsValue: Of<'recentErrors'> | null = null;

  protected readonly int = int;
  protected readonly ago = ago;

  /** The placeholder rows of a kind's loading twin: as many as the engine says the ready widget draws, so a popup does not change height. */
  protected placeholders(kind: PopoverWidget): null[] {
    const rows = this.store.shape().find((s) => s.kind === kind)?.rows ?? 0;
    return Array.from({ length: rows }, () => null);
  }

  protected size(n: number, digits = 1): string {
    return bytes(n, digits);
  }

  protected server(s: ServerState): { icon: string; word: string } {
    return SERVER[s];
  }

  protected model(s: ModelRowState): { icon: string; word: string } {
    return MODEL[s];
  }

  /** The word that goes with an amber or red meter: a meter never says it by colour alone. */
  protected fill(tone: MeterTone): string | null {
    return tone === 'bad' ? 'nearly full' : tone === 'warn' ? 'getting full' : null;
  }

  protected count(n: number, noun: string): string {
    return `${int(n)} ${noun}${n === 1 ? '' : 's'}`;
  }

  protected engineIcon(e: EngineHealth | null | undefined): string {
    if (!e) return 'pw:engines';
    return e.kind === 'ready' ? 'pw:success' : e.kind === 'failed' ? 'pw:error' : e.kind === 'installing' ? 'pw:download' : 'pw:alert';
  }

  protected engineText(e: EngineHealth | null | undefined): string {
    if (!e) return 'Engines not reported';
    switch (e.kind) {
      case 'ready':
        return 'Engines ready';
      case 'installing':
        return 'Installing the engine';
      case 'missing':
        return 'No engine installed';
      case 'restarting':
        return `${this.count(e.count, 'engine')} restarting`;
      case 'failed':
        return `${this.count(e.count, 'engine')} failed`;
    }
  }
}
