import { ChangeDetectionStrategy, Component, ElementRef, afterNextRender, computed, effect, inject, signal, viewChild } from '@angular/core';
import { DomSanitizer, type SafeHtml } from '@angular/platform-browser';
import * as echarts from 'echarts/core';
import { LineChart, ScatterChart } from 'echarts/charts';
import { GridComponent, LegendComponent, MarkLineComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import {
  SU_ACCENTS,
  SU_DENSITIES,
  SU_THEMES,
  SuCard,
  SuEmptyState,
  SuErrorState,
  SuMetric,
  SuSegmentedControl,
  SuSkeleton,
  SuJob,
  type SuJobSnapshot,
  SuSparkline,
  SuStatusPill,
  SuTrayPopover,
  applyPaddockAppearance,
  areaGradient,
  eventLaneFragment,
  readPaddockTokens,
  registerPaddockTheme,
  type SuAccent,
  type SuDensity,
  type SuTheme,
} from '@decryp7/paddock-ui';
import { PdPathText } from '@decryp7/paddock-ui';
import { PD_ICON_PATHS, PdGlyph, pdIconSvg, type PdIconName, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import { PdSelect } from '@decryp7/paddock-ui/select';
import { GalData } from './data-sections';
import { GalMaterial } from './material-section';
import { GalView } from './view-section';
import { GalWidgetGrid } from './widget-grid-section';
import { GalWidgets } from './widgets-section';

echarts.use([LineChart, ScatterChart, GridComponent, LegendComponent, TooltipComponent, MarkLineComponent, CanvasRenderer]);

function param<T extends string>(name: string, allowed: readonly T[], fallback: T): T {
  const v = new URLSearchParams(location.search).get(name);
  return allowed.find((a) => a === v) ?? fallback;
}

const CPU = [22, 25, 31, 28, 40, 52, 47, 38, 35, 44, 61, 58, 49, 42, 39, 45];
const MEM = [60, 61, 61, 62, 64, 66, 66, 67, 69, 70, 70, 71, 71, 72, 72, 72];
const NET = [4, 6, 5, 9, 22, 18, 12, 9, 7, 8, 30, 26, 14, 10, 8, 9];

@Component({
  selector: 'gal-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdStatusGlyph, SuCard, SuStatusPill, SuMetric, SuSparkline, SuEmptyState, SuErrorState, SuSkeleton, SuJob, SuSegmentedControl, SuTrayPopover, PdGlyph, PdPathText, PdSelect, GalData, GalView, GalWidgets, GalWidgetGrid, GalMaterial],
  template: `
    <main class="gal-page">
      <header class="gal-bar">
        <h1>paddock-ui gallery</h1>
        <pd-segmented-control label="Theme" [options]="themes" [(value)]="theme" />
        <pd-segmented-control label="Accent" [options]="accents" [(value)]="accent" />
        <pd-segmented-control label="Density" [options]="densities" [(value)]="density" />
      </header>

      <section class="gal-section" data-testid="card" aria-labelledby="h-card">
        <h2 id="h-card">Card / tile</h2>
        <div class="gal-grid">
          <pd-card heading="CPU">
            <svg slot="icon" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><rect x="4" y="4" width="8" height="8" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.5" /></svg>
            <pd-status-pill slot="status" status="success" label="Normal" />
            <pd-metric value="45" unit="%" [delta]="3" deltaUnit=" pts" deltaLabel="vs last hour" goodDirection="down" />
            <pd-sparkline slot="sparkline" label="CPU, last hour" [values]="cpu" [area]="true" scale="zero" [max]="100" />
            <span slot="caption">peak 61% at 14:20</span>
          </pd-card>
          <pd-card heading="Memory">
            <pd-status-pill slot="status" status="warning" label="High" />
            <pd-metric [value]="17.1" unit="GB" [decimals]="1" />
            <pd-sparkline slot="sparkline" label="Memory, last hour" [values]="mem" scale="zero" [max]="100" />
            <span slot="caption">72% of 24 GB</span>
          </pd-card>
          <pd-card heading="Network" [interactive]="true" (activated)="clicks.set(clicks() + 1)">
            <pd-metric [value]="5.02" unit="MB/s" [decimals]="2" />
            <pd-sparkline slot="sparkline" label="Down, last hour" [values]="net" />
            <span slot="caption">Up 0.4 MB/s · down 4.6 MB/s</span>
            <span class="gal-sr" data-testid="activations">activated {{ clicks() }}</span>
          </pd-card>
          <pd-card heading="Runners">
            <pd-status-pill slot="status" status="danger" label="1 failing" />
            <pd-metric [value]="12408" size="display" />
            <span slot="caption">jobs this week</span>
          </pd-card>
        </div>
      </section>

      <section class="gal-section" data-testid="status-pill" aria-labelledby="h-pill">
        <h2 id="h-pill">Status pill</h2>
        <div class="gal-row">
          <pd-status-pill status="success" label="Healthy" />
          <pd-status-pill status="warning" label="Needs attention" />
          <pd-status-pill status="danger" label="Failing" />
          <pd-status-pill status="info" label="Update available" />
          <pd-status-pill status="neutral" label="Paused" />
        </div>
      </section>

      <section class="gal-section" data-testid="metric" aria-labelledby="h-metric">
        <h2 id="h-metric">Metric</h2>
        <div class="gal-row">
          <div class="gal-metric"><pd-metric [value]="74" unit="/100" /><span class="gal-cap">Health score</span></div>
          <div class="gal-metric"><pd-metric [value]="1234567" /><span class="gal-cap">Events indexed</span></div>
          <div class="gal-metric"><pd-metric [value]="3.2" unit="%" size="title" [delta]="-1.4" deltaUnit="%" deltaLabel="vs last week" goodDirection="down" /><span class="gal-cap">Error rate</span></div>
          <div class="gal-metric"><pd-metric [value]="86" unit="%" size="title" [delta]="12" deltaUnit="%" goodDirection="down" /><span class="gal-cap">Disk used</span></div>
          <div class="gal-metric"><pd-metric [value]="18.4" unit="ms" size="body" [decimals]="1" [delta]="0" /><span class="gal-cap">Median latency</span></div>
          <div class="gal-metric"><pd-metric [value]="null" /><span class="gal-cap">No data yet</span></div>
        </div>
      </section>

      <section class="gal-section" data-testid="sparkline" aria-labelledby="h-spark">
        <h2 id="h-spark">Sparkline</h2>
        <div class="gal-row">
          <div class="gal-box gal-skel-card"><pd-sparkline label="CPU" [values]="cpu" [area]="true" /></div>
          <div class="gal-box gal-skel-card"><pd-sparkline label="Memory" [values]="mem" [height]="24" /></div>
          <div class="gal-box gal-skel-card"><pd-sparkline label="Gap" [values]="gap" /></div>
        </div>
      </section>

      <section class="gal-section" data-testid="empty-state" aria-labelledby="h-empty">
        <h2 id="h-empty">Empty state</h2>
        <div class="gal-box">
          <pd-empty-state heading="No runners yet" icon="folder" description="Register a runner and its jobs appear here with a live timer.">
            <button slot="action" type="button" class="gal-btn gal-btn--primary">Register runner</button>
          </pd-empty-state>
        </div>
      </section>

      <section class="gal-section" data-testid="error-state" aria-labelledby="h-error">
        <h2 id="h-error">Error state</h2>
        <div class="gal-box">
          <pd-error-state heading="Can't reach the Gitea server" cause="Check the URL in Settings, then retry." secondaryLabel="Open logs" (retry)="retries.set(retries() + 1)" />
          <p class="gal-sr" data-testid="retries">Retried {{ retries() }}</p>
        </div>
      </section>

      <section class="gal-section" data-testid="skeleton" aria-labelledby="h-skel">
        <h2 id="h-skel">Skeleton</h2>
        <div class="gal-row">
          <div class="gal-box gal-skel-card">
            <pd-skeleton width="40%" />
            <pd-skeleton shape="block" height="28" width="60%" />
            <pd-skeleton shape="block" height="32" />
            <pd-skeleton width="80%" />
          </div>
          <pd-skeleton shape="circle" width="40" height="40" />
        </div>
      </section>

      <section class="gal-section" data-testid="job" aria-labelledby="h-job">
        <h2 id="h-job">Job / long operation</h2>
        <div class="gal-grid">
          @for (j of jobs; track j.id) {
            <pd-job [job]="j" [stalled]="j.id === 4" [retryable]="true" />
          }
        </div>
      </section>

      <section class="gal-section" data-testid="segmented-control" aria-labelledby="h-seg">
        <h2 id="h-seg">Segmented control</h2>
        <div class="gal-row">
          <pd-segmented-control label="Time range" [options]="ranges" [(value)]="range" />
          <pd-segmented-control label="Level" [options]="levels" [(value)]="level" />
          <span class="gal-sr" data-testid="selected">Selected: {{ range() }} / {{ level() }}</span>
        </div>
      </section>

      <section class="gal-section" data-testid="tray-popover" aria-labelledby="h-tray">
        <h2 id="h-tray">Tray popover</h2>
        <div class="gal-tray-stage">
          <pd-tray-popover [label]="appName()" platform="macos">
            <div slot="header" class="gal-inline">
              <strong>{{ appName() }}</strong>
              <pd-status-pill status="success" label="Good" />
            </div>
            <div class="gal-tray-tiles">
              <pd-card heading="CPU" [flat]="true"><pd-metric [value]="45" unit="%" size="title" /><pd-sparkline slot="sparkline" label="CPU" [values]="cpu" [height]="24" /></pd-card>
              <pd-card heading="Memory" [flat]="true"><pd-metric [value]="72" unit="%" size="title" /><pd-sparkline slot="sparkline" label="Memory" [values]="mem" [height]="24" /></pd-card>
            </div>
            <ul class="gal-list">
              <li><span class="grow">rustc</span><span class="gal-num">42 %</span></li>
              <li><span class="grow">WindowServer</span><span class="gal-num">11 %</span></li>
              <li><span class="grow">Safari</span><span class="gal-num">6 %</span></li>
            </ul>
            <div slot="footer" class="gal-inline">
              <button type="button" class="gal-btn gal-btn--primary">Open {{ appName() }}</button>
              <button type="button" class="gal-btn gal-btn--icon" aria-label="Pause monitoring">II</button>
            </div>
          </pd-tray-popover>
        </div>
      </section>


      <section class="gal-section" data-testid="icons" aria-labelledby="h-icons">
        <h2 id="h-icons">Icons (pd:)</h2>
        <p>One 24 px grid, round caps and joins, currentColor. First row: pd-glyph at 16 px (the UI size, 1.5 px on screen) and 24 px. Second row: the registered literal, exactly what mat-icon svgIcon="pd:name" draws, at 16 and 24 px.</p>
        <ul class="gal-icons">
          @for (n of iconNames; track n) {
            <li class="gal-icon">
              <span class="gal-icon-pair" data-row="glyph"><pd-glyph [name]="n" [size]="16" /><pd-glyph [name]="n" [size]="24" /></span>
              <!-- eslint-disable-next-line paddock/template-no-inner-html -- fixed SVG literals of this package, bound exactly as <mat-icon svgIcon> inserts them, to compare with pd-glyph -->
              <span class="gal-icon-pair" data-row="literal"><span class="mat-icon gal-mat gal-mat--16" [innerHTML]="literal(n)"></span><span class="mat-icon gal-mat gal-mat--24" [innerHTML]="literal(n)"></span></span>
              <span class="gal-icon-name">{{ n }}</span>
            </li>
          }
        </ul>
      </section>

      <section class="gal-section" data-testid="controls" aria-labelledby="h-controls">
        <h2 id="h-controls">Controls</h2>
        <div class="gal-row gal-row--center">
          <button type="button" class="pd-btn pd-btn--primary"><pd-glyph name="plus" /> Add runner</button>
          <button type="button" class="pd-btn"><pd-glyph name="refresh" /> Refresh</button>
          <button type="button" class="pd-btn pd-btn--ghost">Cancel</button>
          <button type="button" class="pd-btn pd-btn--danger"><pd-glyph name="trash" /> Delete</button>
          <button type="button" class="pd-btn pd-btn--sm">Small button</button>
          <button type="button" class="pd-btn pd-btn--icon pd-btn--ghost" aria-label="More"><pd-glyph name="more" /></button>
          <button type="button" class="pd-btn" disabled>Disabled</button>
        </div>
        <div class="gal-row">
          <div class="pd-field"><label for="g-name">Name</label><input id="g-name" class="pd-input" value="builder-01" /><span class="pd-field__hint">Shown in the sidebar.</span></div>
          <div class="pd-field"><label for="g-bad">Port</label><input id="g-bad" class="pd-input" value="99999" aria-invalid="true" aria-describedby="g-bad-error" /><span id="g-bad-error" class="pd-field__error"><pd-status-glyph status="error" [size]="14" /> Use a port from 1 to 65535.</span></div>
          <div class="pd-field"><label for="g-native">Native select</label><select id="g-native" class="pd-select"><option>Standard</option><option>Compact</option></select></div>
          <pd-select label="Themed select" inputId="g-themed" [options]="selectOptions" [value]="selected()" (valueChange)="selected.set($event)" />
          <div class="pd-field pd-field--check"><span class="pd-field__label" aria-hidden="true">&nbsp;</span><label class="pd-check"><input type="checkbox" checked /> Start with the system</label></div>
        </div>
        <div class="gal-row gal-row--center">
          <div class="pd-chips"><span class="pd-chip">linux <button type="button" aria-label="Remove linux"><pd-glyph name="close" [size]="14" /></button></span><span class="pd-chip">x64</span></div>
          <div class="pd-segmented" role="group" aria-label="View"><button type="button" aria-pressed="true">List</button><button type="button" aria-pressed="false">Grid</button></div>
          <div class="pd-tabs" role="tablist"><button type="button" role="tab" aria-selected="true">General</button><button type="button" role="tab" aria-selected="false">Network</button></div>
          <span>Press <kbd class="pd-kbd">Ctrl</kbd><kbd class="pd-kbd">K</kbd></span>
        </div>
      </section>


      <section class="gal-section" data-testid="states" aria-labelledby="h-states">
        <h2 id="h-states">Control states</h2>
        <p>Each control at rest, hover, focus and disabled (forced by the review sheet), on the page, on a card (surface-1 with a hairline) and on an inset (surface-2).</p>
        @for (surface of ['page', 'card', 'inset']; track surface) {
          <div class="gal-states" [class.gal-states--card]="surface === 'card'" [class.gal-states--inset]="surface === 'inset'" [attr.data-surface]="surface">
            <div class="gal-states-row" data-group="buttons">
              <button type="button" class="pd-btn pd-btn--primary" data-state="rest">Primary</button>
              <button type="button" class="pd-btn pd-btn--primary" data-state="hover">Hover</button>
              <button type="button" class="pd-btn pd-btn--primary" data-state="focus">Focus</button>
              <button type="button" class="pd-btn pd-btn--primary" disabled>Disabled</button>
              <button type="button" class="pd-btn" data-state="rest">Secondary</button>
              <button type="button" class="pd-btn" data-state="hover">Hover</button>
              <button type="button" class="pd-btn" data-state="focus">Focus</button>
              <button type="button" class="pd-btn" disabled>Disabled</button>
            </div>
            <div class="gal-states-row" data-group="fields">
              <input class="pd-input" value="rest" aria-label="States: input rest" data-state="rest" />
              <input class="pd-input" value="hover" aria-label="States: input hover" data-state="hover" />
              <input class="pd-input" value="focus" aria-label="States: input focus" data-state="focus" />
              <input class="pd-input" value="disabled" aria-label="States: input disabled" disabled />
              <select class="pd-select" aria-label="States: native rest" data-state="rest"><option>Standard</option></select>
              <select class="pd-select" aria-label="States: native focus" data-state="focus"><option>Focus</option></select>
              <select class="pd-select" aria-label="States: native disabled" disabled><option>Disabled</option></select>
            </div>
            <div class="gal-states-row" data-group="themed-select">
              <pd-select ariaLabel="States: themed rest" [options]="selectOptions" value="standard" data-state="rest" />
              <pd-select ariaLabel="States: themed focus" [options]="selectOptions" value="compact" data-state="focus" />
              <pd-select ariaLabel="States: themed disabled" [options]="selectOptions" value="standard" [disabled]="true" />
              <input class="pd-input" value="Same height" aria-label="States: input beside" />
            </div>
            <div class="gal-states-row" data-group="toggles">
              <label class="pd-check"><input class="pd-switch" type="checkbox" role="switch" /> Off</label>
              <label class="pd-check"><input class="pd-switch" type="checkbox" role="switch" checked /> On</label>
              <label class="pd-check"><input class="pd-switch" type="checkbox" role="switch" checked disabled /> On, disabled</label>
              <label class="pd-check"><input type="checkbox" /> Unchecked</label>
              <label class="pd-check"><input type="checkbox" checked /> Checked</label>
              <label class="pd-check"><input type="checkbox" data-state="focus" /> Focus</label>
              <label class="pd-check"><input type="checkbox" checked disabled /> Disabled</label>
              <label class="pd-check"><input type="radio" name="g-r-{{ surface }}" /> Radio</label>
              <label class="pd-check"><input type="radio" name="g-r-{{ surface }}" checked /> Selected</label>
            </div>
            <div class="gal-states-row" data-group="small-icons">
              @for (n of ['alert', 'info', 'check', 'close', 'more', 'keyboard']; track n) {
                <span class="gal-icon-pair"><pd-glyph [name]="n" [size]="14" /><pd-glyph [name]="n" [size]="16" /><pd-glyph [name]="n" [size]="24" /></span>
              }
            </div>
            <div class="gal-states-row" data-group="choice">
              <div class="pd-segmented" role="group" aria-label="States: view"><button type="button" aria-pressed="true">List</button><button type="button" aria-pressed="false">Grid</button><button type="button" aria-pressed="false">Map</button></div>
              <pd-segmented-control label="States: level" [options]="levels" value="all" />
              <div class="pd-tabs" role="tablist"><button type="button" role="tab" aria-selected="true">General</button><button type="button" role="tab" aria-selected="false">Network</button></div>
              <span>Press <kbd class="pd-kbd">Ctrl</kbd><kbd class="pd-kbd">K</kbd></span>
            </div>
          </div>
        }
      </section>

      <section class="gal-section" data-testid="path-text" aria-labelledby="h-path">
        <h2 id="h-path">Path text</h2>
        <div class="gal-row gal-row--center">
          <div class="gal-box gal-path"><pd-path-text value="/Users/lin/Library/Application Support/Builder/logs/builder.service.log" [copy]="true" /></div>
          <div class="gal-box gal-path gal-path--narrow"><pd-path-text value="C:\\ProgramData\\Builder\\Logs\\Builder.Service.2026-10-24.log" /></div>
          <div class="gal-path gal-path--plain240" data-testid="path-win-240"><pd-path-text value="C:\\ProgramData\\Builder\\Logs\\Builder.Service.2026-10-24.log" /></div>
          <div class="gal-box gal-path gal-path--240" data-testid="path-240"><pd-path-text value="/Users/lin/Library/Application Support/Builder/logs/builder.service.log" /></div>
        </div>
      </section>

      <section class="gal-section" data-testid="chart-theme" aria-labelledby="h-chart">
        <h2 id="h-chart">ECharts theme</h2>
        <div class="gal-box"><div #chart class="gal-chart" role="img" aria-label="CPU and memory, last 16 samples, with events"></div></div>
        <div class="gal-swatches">
          @for (n of [1, 2, 3, 4, 5, 6, 7, 8]; track n) { <span class="gal-swatch gal-swatch--{{ n }}"></span> }
        </div>
        <div class="gal-swatches">
          @for (n of ['s1', 's2', 's3', 's4', 's5', 's6']; track n) { <span class="gal-swatch gal-swatch--{{ n }}"></span> }
        </div>
      </section>

      <gal-data />
      <gal-view />
      <gal-widgets />
      <gal-widget-grid />
      @defer (on viewport) {
        <gal-material />
      } @placeholder {
        <div class="gal-material-placeholder" data-testid="material-placeholder"></div>
      }
    </main>
  `,
})
export class Gallery {
  protected readonly jobs: SuJobSnapshot[] = [
    { id: 1, kind: 'import', title: 'Import logs', subject: '3 folders', state: 'running', seq: 9, elapsedMs: 4000, cancellable: true,
      phase: { id: 'walk', label: 'Finding files', unit: 'items', total: null, done: 12345 } },
    { id: 2, kind: 'download', title: 'Download model', subject: 'model.gguf', state: 'running', seq: 9, elapsedMs: 30000, etaMs: 90000, cancellable: true,
      phase: { id: 'download', label: 'Downloading', unit: 'bytes', total: 123731968, done: 44040192 } },
    { id: 3, kind: 'save', title: 'Save project', state: 'running', seq: 9, elapsedMs: 9000, cancellable: false, notCancellableReason: 'Writing the database',
      phase: { id: 'save', label: 'Saving', unit: 'items', total: 4, done: 3 } },
    { id: 4, kind: 'scan', title: 'Scan disk', state: 'running', seq: 9, elapsedMs: 120000, cancellable: true,
      phase: { id: 'scan', label: 'Reading folders', unit: 'items', total: null, done: 880000 } },
    { id: 5, kind: 'export', title: 'Export events', state: 'done', seq: 9, elapsedMs: 3000, cancellable: false },
    { id: 6, kind: 'export', title: 'Export events', state: 'failed', message: 'The disk is full. Free some space and retry.', seq: 9, elapsedMs: 3000, cancellable: false },
  ];
  protected readonly iconNames = Object.keys(PD_ICON_PATHS);
  protected readonly selectOptions = [
    { value: 'standard', label: 'Standard' },
    { value: 'compact', label: 'Compact' },
    { value: 'comfortable', label: 'Comfortable' },
  ];
  protected readonly selected = signal('compact');
  protected readonly cpu = CPU;
  protected readonly mem = MEM;
  protected readonly net = NET;
  protected readonly gap = [10, 14, 12, Number.NaN, Number.NaN, 30, 28, 33, 29];
  protected readonly themes = SU_THEMES.map((value) => ({ value, label: value }));
  protected readonly accents = SU_ACCENTS.map((value) => ({ value, label: value }));
  protected readonly densities = SU_DENSITIES.map((value) => ({ value, label: value }));
  protected readonly ranges = [
    { value: '1h', label: '1 h' },
    { value: '24h', label: '24 h' },
    { value: '7d', label: '7 days' },
  ];
  protected readonly levels = [
    { value: 'all', label: 'All', count: 1_204 },
    { value: 'warn', label: 'Warnings', count: 31 },
    { value: 'err', label: 'Errors', count: 6 },
    { value: 'off', label: 'Off', disabled: true },
  ];

  private readonly sanitizer = inject(DomSanitizer);
  /** The registered `pd:` literal of a glyph, as `<mat-icon svgIcon>` inserts it (fixed content from the package). */
  protected literal(name: string): SafeHtml {
    return this.sanitizer.bypassSecurityTrustHtml(pdIconSvg(name as PdIconName));
  }

  protected readonly theme = signal<string>(param<SuTheme>('theme', SU_THEMES, 'system'));
  protected readonly accent = signal<string>(param<SuAccent>('accent', SU_ACCENTS, 'debrief'));
  protected readonly density = signal<string>(param<SuDensity>('density', SU_DENSITIES, 'standard'));
  protected readonly range = signal('24h');
  protected readonly level = signal('all');
  protected readonly clicks = signal(0);
  /** The app the selected accent belongs to: the tray popover is named after it (accents never meet another app's name). */
  protected readonly appName = computed(() => ({ debrief: 'Debrief', pitcrew: 'Pitcrew', pitwall: 'Pitwall', dyno: 'Dyno', spotter: 'Spotter', pacenotes: 'Pacenotes', sighter: 'Sighter' })[this.accent()] ?? 'Shell demo');
  protected readonly retries = signal(0);

  private readonly chartEl = viewChild.required<ElementRef<HTMLElement>>('chart');
  private chart: echarts.ECharts | null = null;

  constructor() {
    effect(() => {
      applyPaddockAppearance(document.documentElement, {
        theme: this.theme() as SuTheme,
        accent: this.accent() as SuAccent,
        density: this.density() as SuDensity,
      });
      // Appearance changed: draw the chart again with re-read tokens.
      queueMicrotask(() => this.draw());
    });
    afterNextRender(() => this.draw());
  }

  private draw(): void {
    const el = this.chartEl().nativeElement;
    const tokens = readPaddockTokens();
    const name = registerPaddockTheme(echarts, tokens);
    this.chart?.dispose();
    const chart = echarts.init(el, name, { renderer: 'canvas' });
    this.chart = chart;
    const t0 = Date.UTC(2026, 9, 22, 8, 0, 0);
    const times = CPU.map((_, i) => t0 + i * 15 * 60_000);
    const lane = eventLaneFragment(
      [
        { at: times[3] as number, kind: 'throttle', label: 'Throttled' },
        { at: times[6] as number, kind: 'cap', label: 'Memory cap' },
        { at: times[10] as number, kind: 'throttle', label: 'Throttled' },
        { at: times[13] as number, kind: 'release', label: 'Released' },
      ],
      tokens,
      1,
      { shapes: { throttle: 'triangle', cap: 'rect', release: 'circle' } },
    );
    chart.setOption({
      grid: [{ left: 8, right: 16, top: 32, bottom: lane.plotBottom, containLabel: true }, lane.grid],
      legend: { data: ['CPU', 'Memory'] },
      xAxis: [{ type: 'time' }, lane.xAxis],
      yAxis: [{ type: 'value', min: 0, max: 100, interval: 50, axisLabel: { formatter: '{value}%' } }, lane.yAxis],
      series: [
        {
          name: 'CPU',
          type: 'line',
          data: CPU.map((v, i) => [times[i], v]),
          areaStyle: { color: areaGradient(tokens.palette[0] as string) },
          lineStyle: { width: 2 },
          markLine: { silent: true, data: [{ yAxis: 90, label: { formatter: 'limit 90%' } }] },
        },
        { name: 'Memory', type: 'line', data: MEM.map((v, i) => [times[i], v]) },
        lane.series,
      ],
    });
  }
}
