import { ChangeDetectionStrategy, Component, inject, signal, viewChildren } from '@angular/core';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { SuSparkline, SuStatusPill, SuTrayPopover } from '@decryp7/paddock-ui';
import { MatIconButton } from '@angular/material/button';
import { MatMenu, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { PdGlyph, PdStatusGlyph, registerPaddockIcons } from '@decryp7/paddock-ui/icons';
import { PdSelect } from '@decryp7/paddock-ui/select';
import {
  PdSkeleton,
  PdWidget,
  PdWidgetArea,
  PdWidgetChooser,
  PdWidgetFrame,
  PdWidgetOption,
  type CatalogueInfo,
  type Edit,
  type KindInfo,
  type LayoutView,
  type Placement,
  type Preset,
  type Progress,
  type Span,
  type SurfaceValues,
  type WidgetState,
} from '@decryp7/paddock-ui/widgets';

/** The sample catalogue's widget: a kind and, for disk space, the volume shown. */
interface Sample {
  readonly kind: string;
  readonly volume?: string;
}
type Act = 'retry' | 'open' | 'customize';

/** What each sample kind's ready template draws. */
interface Val {
  readonly alarms?: readonly { sev: 'Warning' | 'Info'; text: string; when: string }[];
  readonly vols?: readonly { name: string; free: string; pct: number; low?: boolean }[];
  readonly cpu?: { cpu: number; mem: number; memText: string };
  readonly net?: { down: string; up: string };
  readonly line?: { title: string; detail: string };
  readonly rows?: readonly { name: string; a: string; b?: string }[];
}

/** 14:32 local: the clock text the frame writes is local time, so the page reads the same everywhere. */
const NOW = new Date(2026, 9, 31, 14, 32).getTime();
const MIN = 60_000;

const kind = (k: string, title: string, summary: string, icon: string, spans: number[], maxInstances = 1, refresh: KindInfo<string>['refresh'] = 'live'): KindInfo<string> => ({
  kind: k,
  title,
  summary,
  icon,
  size: { spans: { spans } },
  maxInstances,
  refresh,
});
const CATALOGUE: CatalogueInfo<string, string> = {
  surface: { id: 'popup', noun: 'popup', columns: 2, maxPlacements: 6, minPlacements: 1, maxUserPresets: 3 },
  kinds: [
    kind('alarms', 'Active alarms', 'Most severe first, up to 2', 'pd:bell', [2], 1, 'onChange'),
    kind('disk', 'Disk space', 'Free space per volume', 'pd:folder', [2], 3, { every: { secs: 60 } }),
    kind('cpu', 'CPU and memory', 'Load and use now, with the last minute', 'pd:activity', [2]),
    kind('net', 'Network', 'Download and upload now', 'pd:globe', [1, 2]),
    kind('backup', 'Backup', 'When backups last ran', 'pd:shield', [1, 2], 1, 'onChange'),
    kind('apps', 'Top apps', 'The busiest apps by CPU', 'pd:window', [2]),
  ],
};
/** The spans a flow kind allows. */
const spansOf = (info: KindInfo<string>): readonly number[] => ('spans' in info.size ? info.size.spans.spans : []);
const place = (id: string, k: string, span = 2, volume?: string): Placement<Sample> => ({ id, span, widget: volume ? { kind: k, volume } : { kind: k } });
const DEFAULTS: Placement<Sample>[] = [place('alarms', 'alarms'), place('disk', 'disk', 2, 'all'), place('cpu', 'cpu'), place('net', 'net', 1), place('backup', 'backup', 1)];

const VALUES: Record<string, Val> = {
  alarms: { alarms: [{ sev: 'Warning', text: 'Memory pressure is high', when: '4 min ago' }, { sev: 'Info', text: 'New login item: Docker Desktop', when: '1 h ago' }] },
  disk: { vols: [{ name: 'Macintosh HD', free: '182 GB free of 994 GB', pct: 82 }, { name: 'Projects', free: '19 GB free of 500 GB', pct: 96, low: true }, { name: 'Time Machine', free: '1.21 TB free of 4 TB', pct: 70 }] },
  cpu: { cpu: { cpu: 76, mem: 51, memText: 'Memory 16.3 of 32 GB' } },
  net: { net: { down: '2.4', up: '310 KB/s' } },
  backup: { line: { title: 'Time Machine', detail: 'Last ran 2 h ago' } },
  apps: { rows: [{ name: 'Rust build (cargo)', a: '42%', b: '3.15 GB' }, { name: 'Google Chrome', a: '6.8%', b: '4.3 GB' }, { name: 'WindowServer', a: '4.1%', b: '305 MB' }] },
};

const ready = (k: string, staleSince: number | null = null): WidgetState<Val, Act> => ({ state: 'ready', value: VALUES[k] as Val, asOf: NOW, staleSince });
const empty = (title: string, detail: string, next: string | null, calm = false): WidgetState<Val, Act> => ({ state: 'empty', title, detail, next: next ? { action: 'open', label: next } : null, calm });
const error = (title: string, cause: string): WidgetState<Val, Act> => ({ state: 'error', title, cause, next: { action: 'retry', label: 'Retry' } });
const working = (title: string, phase: string, progress: Progress): WidgetState<Val, Act> => ({ state: 'working', title, phase, progress, last: null, cancel: null });

/** One row of the states sheet: a kind and its states (null is loading). */
const SHEET: readonly { kind: string; states: readonly (readonly [string, WidgetState<Val, Act> | null])[] }[] = [
  { kind: 'alarms', states: [['loading', null], ['ready', ready('alarms')], ['empty', empty('No active alarms', 'checked 2 min ago', 'Open alarms', true)], ['error', error("Couldn't read alarms", "The alarm store didn't answer.")], ['stale', ready('alarms', NOW - 4 * MIN)]] },
  { kind: 'disk', states: [['loading', null], ['ready', ready('disk')], ['empty', empty("See Time Machine's space when it's connected", "The disk isn't connected now.", 'Choose another volume')], ['error', error("Couldn't read volumes", "The volume list didn't answer.")], ['stale', ready('disk', NOW - 3 * MIN)]] },
  { kind: 'cpu', states: [['loading', null], ['ready', ready('cpu')], ['stale', ready('cpu', NOW - 4 * MIN)]] },
  { kind: 'net', states: [['loading', null], ['ready', ready('net')], ['empty', empty('See traffic when online', 'Offline now.', 'Open network')], ['stale', ready('net', NOW - 4 * MIN)]] },
  { kind: 'backup', states: [['loading', null], ['ready', ready('backup')], ['empty', empty('Know when backups ran', 'No backup set up.', 'Set up checks')], ['error', error("Couldn't check", 'tmutil failed.')], ['working', working('Checking', 'Time Machine', { fraction: { done: 2, total: 3, unit: '' } })]] },
  { kind: 'apps', states: [['loading', null], ['ready', ready('apps')], ['stale', ready('apps', NOW - 4 * MIN)]] },
];
const COLUMNS = ['loading', 'ready', 'empty', 'error', 'working', 'stale'] as const;

/** The popup's live values: the ready ones, then what the agent-stopped notice does to them. */
const POPUP_VALUES: SurfaceValues<Val, Act> = { notice: null, placements: DEFAULTS.map((p) => ({ id: p.id, state: ready(p.widget.kind) })) };
const STOPPED_VALUES: SurfaceValues<Val, Act> = {
  notice: { tone: 'danger', title: 'Agent stopped', detail: 'Readings paused at 14:28.', action: { action: 'retry', label: 'Restart agent' } },
  placements: DEFAULTS.map((p) => ({ id: p.id, state: ready(p.widget.kind, NOW - 4 * MIN) })),
};

const MINIMAL: Preset<Sample> = { id: 'minimal', name: 'Minimal', builtin: true, placements: [place('alarms', 'alarms'), place('cpu', 'cpu')] };
const view = (placements: Placement<Sample>[], canUndo: boolean, presets: Preset<Sample>[] = []): LayoutView<string, Sample, Span, string> => ({
  layout: { version: 1, placements, presets },
  revision: 1,
  arrangements: [],
  moves: [],
  canRedo: false,
  outcome: null,
  catalogue: CATALOGUE,
  builtinPresets: [MINIMAL],
  notes: [],
  canUndo,
  editable: true,
});

/**
 * The widgets of ADR 0035 (`@decryp7/paddock-ui/widgets`): every kind's states in a frame, the area in a tray popup with the
 * agent-stopped notice, and the chooser with a stand-in for the engine (the app's engine edits the layout; here a small
 * function does, so the page can be used and tested without one).
 */
@Component({
  selector: 'gal-widgets',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWidgetArea, PdWidgetFrame, PdWidgetChooser, PdWidget, PdWidgetOption, PdSkeleton, PdSelect, SuTrayPopover, SuSparkline, SuStatusPill, MatIconButton, MatMenu, MatMenuItem, MatMenuTrigger, PdGlyph, PdStatusGlyph],
  template: `
    <section class="gal-section" data-testid="widgets" aria-labelledby="h-widgets">
      <h2 id="h-widgets">Widgets</h2>
      <p>One frame per widget: loading is the ready template with its parts as skeleton blocks, so the height never changes. A busy source is working or stale, never an error.</p>

      <!-- The kind templates: one set serves the sheet, the popup and the chooser's preview. -->
      <ng-template pdWidget="alarms" [pdWidgetOf]="witness" let-x let-busy="loading">
        <div class="wg-rows">
          @for (r of x?.alarms ?? [{ sev: 'Warning', text: 'xxxxxxxxxxxxxxx', when: 'xxxxxx' }, { sev: 'Info', text: 'xxxxxxxxxxxxxxx', when: 'xxxxxx' }]; track $index) {
            <!-- A row that opens a page is one real button; its loading twin is a plain row of the same size. -->
            @if (busy) {
              <div class="wg-row">
                <span class="wg-badge" pdSkeleton>{{ r.sev }}</span>
                <span class="wg-grow wg-trunc" pdSkeleton>{{ r.text }}</span>
                <span class="wg-when" pdSkeleton>{{ r.when }}</span>
              </div>
            } @else {
              <button type="button" class="wg-row wg-open" (click)="acted.set('open')">
                <span class="wg-badge" [class.wg-badge--info]="r.sev === 'Info'"><pd-status-glyph [status]="r.sev === 'Info' ? 'info' : 'warning'" [size]="14" />{{ r.sev }}</span>
                <span class="wg-grow wg-trunc">{{ r.text }}</span>
                <span class="wg-when">{{ r.when }}</span>
              </button>
            }
          }
        </div>
      </ng-template>
      <ng-template pdWidget="disk" [pdWidgetOf]="witness" let-x>
        @for (d of x?.vols ?? [{ name: 'xxxxxxxxxx', free: 'xxxxxxxxxxxxxxxxxxxx', pct: 50 }, { name: 'xxxxxxxx', free: 'xxxxxxxxxxxxxxxxxxxx', pct: 50 }, { name: 'xxxxxxxxxxxx', free: 'xxxxxxxxxxxxxxxxxxxx', pct: 50 }]; track $index) {
          <div class="wg-vol">
            <span class="wg-name" pdSkeleton>{{ d.name }}@if (d.low) { <span class="wg-badge wg-badge--warn"><pd-status-glyph status="warning" [size]="14" />Low</span> }</span>
            <span class="wg-free" pdSkeleton>{{ d.free }}</span>
            <span class="wg-bar" [class.wg-bar--warn]="d.low" pdSkeleton><i class="wg-fill" [style.width.%]="d.pct"></i></span>
          </div>
        }
      </ng-template>
      <ng-template pdWidget="cpu" [pdWidgetOf]="witness" let-x>
        <div class="wg-split">
          <div class="wg-metric"><span class="wg-k" pdSkeleton>CPU</span><span class="wg-hero" pdSkeleton>{{ x?.cpu?.cpu ?? '00' }}<small>%</small></span><span class="wg-spark" pdSkeleton>@if (x) { <pd-sparkline [values]="cpuSeries" [height]="24" [area]="true" [min]="0" [max]="100" label="CPU, last minute" /> }</span></div>
          <span class="wg-div"></span>
          <div class="wg-metric"><span class="wg-k" pdSkeleton>{{ x?.cpu?.memText ?? 'Memory xxxxxxxxxxx' }}</span><span class="wg-hero" pdSkeleton>{{ x?.cpu?.mem ?? '00' }}<small>%</small></span><span class="wg-spark wg-spark--mem" pdSkeleton>@if (x) { <pd-sparkline [values]="memSeries" [height]="24" [area]="true" [min]="0" [max]="100" label="Memory, last minute" /> }</span></div>
        </div>
      </ng-template>
      <ng-template pdWidget="net" [pdWidgetOf]="witness" let-x>
        <span class="wg-hero" pdSkeleton><pd-glyph name="arrow-down" [size]="16" />{{ x?.net?.down ?? '0.0' }}<small>MB/s</small></span>
        <span class="wg-line" pdSkeleton><pd-glyph name="arrow-up" [size]="14" />{{ x?.net?.up ?? '000 KB/s' }}</span>
        <span class="wg-spark" pdSkeleton>@if (x) { <pd-sparkline [values]="netSeries" [height]="24" [area]="true" label="Network, last minute" /> }</span>
      </ng-template>
      <ng-template pdWidget="backup" [pdWidgetOf]="witness" let-x>
        <span class="wg-line wg-strong" pdSkeleton>{{ x?.line?.title ?? 'xxxxxxxx' }}</span>
        <span class="wg-line wg-muted" pdSkeleton>{{ x?.line?.detail ?? 'xxxxxxxxxxxxx' }}</span>
      </ng-template>
      <ng-template pdWidget="apps" [pdWidgetOf]="witness" let-x>
        <div class="wg-rows">
        @for (r of x?.rows ?? [{ name: 'xxxxxxxxxxxxxxxxxx', a: 'xxxx', b: 'xxxxxx' }, { name: 'xxxxxxxxxxxxxxxxxx', a: 'xxxx', b: 'xxxxxx' }, { name: 'xxxxxxxxxxxxxxxxxx', a: 'xxxx', b: 'xxxxxx' }]; track $index) {
          <div class="wg-row"><span class="wg-grow wg-trunc" pdSkeleton>{{ r.name }}</span><span class="wg-num" pdSkeleton>{{ r.a }}</span><span class="wg-num wg-muted" pdSkeleton>{{ r.b }}</span></div>
        }
        </div>
      </ng-template>

      <h3>Every state</h3>
      <div class="wg-sheet" role="group" aria-label="Widget states">
        @for (row of sheet; track row.kind) {
          @let info = kindInfo(row.kind);
          @for (col of columns; track col) {
            @let st = stateOf(row, col);
            @if (st.has) {
              <div class="wg-cell" [class.wg-cell--half]="spansOf(info)[0] === 1" [attr.data-testid]="'wgt-' + row.kind + '-' + col">
                <span class="wg-cap">{{ info.title }} · {{ col }}</span>
                <pd-widget-frame
                  [title]="info.title"
                  [icon]="info.icon"
                  [span]="spansOf(info)[0] ?? 2"
                  [state]="st.state"
                  [template]="templateOf(row.kind)"
                  [live]="info.refresh === 'live'"
                  [now]="now"
                  [metaText]="row.kind === 'disk' ? '3 volumes' : null"
                  (action)="acted.set($event)"
                />
              </div>
            }
          }
        }
      </div>

      <h3>Popup</h3>
      <div class="gal-row" role="group" aria-label="Popup controls">
        <button type="button" class="gal-btn" data-testid="wgt-stop" [attr.aria-pressed]="stopped()" (click)="stopped.set(!stopped())">Stop the agent</button>
        <button type="button" class="gal-btn" data-testid="wgt-load" [attr.aria-pressed]="loading()" (click)="loading.set(!loading())">Show loading</button>
        <button type="button" class="gal-btn" data-testid="wgt-empty" [attr.aria-pressed]="emptied()" (click)="emptied.set(!emptied())">No widgets</button>
        <span data-testid="wgt-acted">Last action: {{ acted() ?? 'none' }}</span>
      </div>
      <div class="wg-popup" data-testid="wgt-popup">
        <pd-tray-popover label="Dyno" platform="macos">
          <strong slot="header" class="wg-name-head">Dyno</strong>
          @if (!loading()) {
            <pd-status-pill slot="header" class="wg-pill" [status]="stopped() ? 'danger' : 'success'" [label]="stopped() ? 'Agent stopped' : 'All good'" />
          }
          <pd-widget-area
            [placements]="emptied() ? [] : popupPlacements"
            [catalogue]="catalogue"
            [values]="loading() ? null : stopped() ? stoppedValues : popupValues"
            [templates]="templates()"
            [now]="now"
            [meta]="meta"
            (action)="acted.set($event)"
            (customize)="acted.set('customize')"
          />
          <div slot="footer" class="wg-foot">
            <button type="button" class="gal-btn gal-btn--primary">Open Dyno</button>
            <button matIconButton type="button" class="wg-more" aria-label="More actions" [matMenuTriggerFor]="footMenu"><pd-glyph name="more" [size]="16" /></button>
            <mat-menu #footMenu="matMenu" class="wg-foot-menu" xPosition="before" yPosition="above">
              <button mat-menu-item type="button" (click)="acted.set('customize')">Customize widgets…</button>
              <button mat-menu-item type="button" (click)="acted.set('settings')">Settings…</button>
              <button mat-menu-item type="button" (click)="acted.set('quit')">Quit Dyno</button>
            </mat-menu>
          </div>
        </pd-tray-popover>
      </div>

      <h3>Chooser</h3>
      <p>The edits go to a stand-in for the engine; the chooser draws what comes back. Remove a widget for the Undo toast.</p>
      <div class="wg-chooser" data-testid="wgt-chooser">
        <pd-widget-chooser [view]="chosen()" [widgetFor]="widgetFor" [again]="{ disk: 'Add volume' }" [preview]="true" (edit)="applyEdit($event)">
          <ng-template pdWidgetOption="disk" [pdWidgetOptionOf]="sample" let-p>
            <pd-select [options]="volumes" [value]="p.widget.volume ?? 'all'" [ariaLabel]="'Volumes shown by ' + p.id" (valueChange)="applyEdit({ replace: { id: p.id, widget: { kind: 'disk', volume: $event } } })" />
          </ng-template>
          <div slot="preview">
            <pd-widget-area [placements]="chosen().layout.placements" [catalogue]="catalogue" [values]="previewValues()" [templates]="templates()" [now]="now" />
          </div>
        </pd-widget-chooser>
      </div>
    </section>
  `,
  styles: `
    h3 { margin: var(--pd-space-4) 0 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
    .wg-sheet { display: grid; grid-template-columns: repeat(auto-fill, 326px); gap: var(--pd-space-4); padding: var(--pd-space-4); background: var(--pd-surface-3); border-radius: var(--pd-radius-card); }
    .wg-cell { width: 326px; min-width: 0; align-self: start; }
    .wg-cap { display: block; margin-bottom: var(--pd-space-1); font-size: var(--pd-text-label); color: var(--pd-text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .wg-cell--half { width: 159px; }
    .wg-name-head { font-weight: var(--pd-weight-semibold); }
    .wg-pill { margin-inline-start: auto; }
    .wg-foot { display: flex; align-items: center; justify-content: flex-end; gap: var(--pd-space-1); width: 100%; }
    .wg-popup { width: 440px; padding: 24px 0; display: flex; justify-content: center; background: var(--pd-surface-1); }
    .wg-chooser { padding: var(--pd-space-4); background: var(--pd-surface-1); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); }
    /* One track that may shrink (minmax(0, 1fr)): an auto track takes the widest row's nowrap content and pushes the rows past the frame. */
    .wg-rows { display: grid; grid-template-columns: minmax(0, 1fr); gap: 2px; margin: 0 calc(-1 * var(--pd-space-2)); }
    .wg-row { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 28px; padding: 2px var(--pd-space-2); }
    .wg-open { width: 100%; border: 0; border-radius: var(--pd-radius-control); background: none; color: inherit; font: inherit; text-align: start; cursor: pointer; }
    .wg-open:hover { background: var(--pd-hover); }
    .wg-open:focus-visible { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: 2px; }
    .wg-grow { flex: 1; min-width: 0; }
    .wg-trunc { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .wg-when, .wg-muted { color: var(--pd-text-subtle); font-size: var(--pd-text-label); white-space: nowrap; }
    .wg-num { font-variant-numeric: tabular-nums; white-space: nowrap; }
    .wg-badge { display: inline-flex; align-items: center; gap: var(--pd-space-1); height: 20px; padding: 0 7px 0 5px; border-radius: var(--pd-radius-pill); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); background: var(--pd-warning-tint); color: var(--pd-warning); }
    .wg-badge--info { background: var(--pd-info-tint); color: var(--pd-info); }
    .wg-badge--warn { margin-left: var(--pd-space-2); }
    .wg-vol { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 4px var(--pd-space-2); align-items: center; padding: 3px 0; }
    .wg-name { display: inline-flex; align-items: center; }
    .wg-free { font-size: var(--pd-text-label); color: var(--pd-text-muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
    .wg-bar { grid-column: 1 / -1; display: block; height: 4px; border-radius: 2px; background: var(--pd-chart-grid); overflow: hidden; }
    .wg-fill { display: block; height: 100%; border-radius: 2px; background: var(--pd-meter-neutral); }
    .wg-bar--warn .wg-fill { background: var(--pd-warning); }
    .wg-split { display: grid; grid-template-columns: minmax(0, 1fr) 1px minmax(0, 1fr); gap: var(--pd-space-3); }
    .wg-div { background: var(--pd-border-strong); opacity: 0.5; }
    .wg-metric { display: grid; gap: 2px; min-width: 0; }
    .wg-k { font-size: var(--pd-text-label); color: var(--pd-text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .wg-hero { font-size: var(--pd-text-title); line-height: 24px; font-weight: var(--pd-weight-semibold); font-variant-numeric: tabular-nums; white-space: nowrap; justify-self: start; }
    .wg-hero small { font-size: var(--pd-text-body); font-weight: var(--pd-weight-regular); color: var(--pd-text-muted); margin-left: 2px; }
    .wg-spark--mem pd-sparkline { color: var(--pd-info); }
    .wg-spark { display: block; height: 24px; margin-top: 4px; border-bottom: 1px solid var(--pd-chart-grid); }
    .wg-line { font-size: var(--pd-text-label); line-height: var(--pd-lh-label); justify-self: start; }
    .wg-strong { font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
  `,
})
export class GalWidgets {
  /** The type witness of the kind templates: `let-x` is a `Val | null` with no cast. */
  protected readonly witness: Val | null = null;
  /** The type witness of the chooser's option templates: `let-p` is a `Placement<Sample>`. */
  protected readonly sample: Sample | null = null;
  protected readonly cpuSeries = [22, 25, 31, 28, 40, 52, 47, 38, 35, 44, 61, 58, 49, 42, 39, 76];
  protected readonly memSeries = [60, 61, 61, 62, 64, 66, 66, 67, 69, 70, 70, 71, 71, 72, 72, 72];
  protected readonly netSeries = [4, 6, 5, 9, 22, 18, 12, 9, 7, 8, 30, 26, 14, 10, 8, 9];
  protected readonly meta = (id: string): string | null => (id === 'disk' ? '3 volumes' : null);
  protected readonly catalogue = CATALOGUE;
  protected readonly sheet = SHEET;
  protected readonly columns = COLUMNS;
  protected readonly now = NOW;
  protected readonly popupPlacements = DEFAULTS;
  protected readonly popupValues = POPUP_VALUES;
  protected readonly stoppedValues = STOPPED_VALUES;
  protected readonly volumes = [{ value: 'all', label: 'All volumes' }, { value: 'macintosh-hd', label: 'Macintosh HD' }, { value: 'projects', label: 'Projects' }];
  protected readonly widgetFor = (k: string): Sample => (k === 'disk' ? { kind: k, volume: 'all' } : { kind: k });

  protected readonly acted = signal<string | null>(null);
  protected readonly stopped = signal(false);
  protected readonly loading = signal(false);
  protected readonly emptied = signal(false);
  /** The stand-in engine's layout and its one level of undo. */
  protected readonly chosen = signal(view(DEFAULTS, false));
  private previous: Placement<Sample>[] | null = null;
  private count = 0;

  protected readonly templates = viewChildren(PdWidget);

  constructor() {
    // The gallery registers the generic glyphs as an app does (the kind icons are `pd:` names).
    registerPaddockIcons(inject(MatIconRegistry), inject(DomSanitizer));
  }

  protected readonly spansOf = spansOf;
  protected kindInfo(k: string): KindInfo<string> {
    return CATALOGUE.kinds.find((i) => i.kind === k) as KindInfo<string>;
  }
  protected templateOf(k: string) {
    return this.templates().find((t) => t.pdWidget() === k)?.template ?? null;
  }
  protected stateOf(row: (typeof SHEET)[number], col: string): { has: boolean; state: WidgetState<Val, Act> | null } {
    const hit = row.states.find(([name]) => name === col);
    return { has: hit !== undefined, state: hit?.[1] ?? null };
  }
  protected previewValues(): SurfaceValues<Val, Act> {
    return { notice: null, placements: this.chosen().layout.placements.map((p) => ({ id: p.id, state: ready(p.widget.kind) })) };
  }

  /** The stand-in engine (a mock: the real one is `paddock-widgets`). */
  protected applyEdit(edit: Edit<Sample>): void {
    const now = this.chosen().layout.placements;
    const presets = this.chosen().layout.presets;
    let nextPresets = presets;
    let next = [...now];
    let undoable = false;
    if (edit === 'undo') {
      next = this.previous ?? now;
      this.previous = null;
    } else if (edit === 'redo') {
      // The sample list has one level of undo, no redo.
      next = now;
    } else if (edit === 'resetToDefaults') {
      this.previous = now;
      next = [...DEFAULTS];
      undoable = true;
    } else if ('applyPreset' in edit) {
      const found = [MINIMAL, ...presets].find((p) => p.id === edit.applyPreset.id);
      this.previous = now;
      next = (found?.placements ?? now).map((p) => ({ ...p }));
      undoable = true;
    } else if ('savePreset' in edit) {
      undoable = this.chosen().canUndo;
      nextPresets = [...presets, { id: `user-${++this.count}`, name: edit.savePreset.name, builtin: false, placements: now }];
    } else if ('deletePreset' in edit) {
      undoable = this.chosen().canUndo;
      nextPresets = presets.filter((p) => p.id !== edit.deletePreset.id);
    } else if ('add' in edit) {
      const span = edit.add.span ?? spansOf(this.kindInfo(edit.add.widget.kind))[0] ?? 2;
      next.push({ id: `${edit.add.widget.kind}-${++this.count}`, span, widget: edit.add.widget });
      this.previous = null;
    } else if ('remove' in edit) {
      this.previous = now;
      next = now.filter((p) => p.id !== edit.remove.id);
      undoable = true;
    } else if ('move' in edit) {
      const from = now.findIndex((p) => p.id === edit.move.id);
      const [moved] = next.splice(from, 1);
      next.splice(edit.move.to, 0, moved as Placement<Sample>);
      this.previous = null;
    } else if ('resize' in edit) {
      next = now.map((p) => (p.id === edit.resize.id ? { ...p, span: edit.resize.span } : p));
      this.previous = null;
    } else if ('replace' in edit) {
      next = now.map((p) => (p.id === edit.replace.id ? { ...p, widget: edit.replace.widget } : p));
      this.previous = null;
    }
    this.chosen.set(view(next, undoable, nextPresets));
  }
}
