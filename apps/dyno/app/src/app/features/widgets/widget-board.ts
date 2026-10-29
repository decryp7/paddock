import { CdkDrag, CdkDragDrop, CdkDragHandle, CdkDropList } from '@angular/cdk/drag-drop';
import { ChangeDetectionStrategy, Component, DestroyRef, computed, effect, inject, linkedSignal, signal } from '@angular/core';
import type { Widget, WidgetKind, WidgetMetric } from '../../core/api.types';
import { Icon } from '../../shared/icon';
import { CountersStore } from '../../state/counters.store';
import { DashboardLayoutStore } from '../../state/dashboard-layout.store';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { HealthStore } from '../../state/health.store';
import { SettingsStore } from '../../state/settings.store';
import { widgetCounterPaths } from '../counters/counters-model';
import { WidgetView } from './widget-view';
import { PANEL_KINDS, KINDS, KIND_HINT, KIND_LABEL, METRICS, SIZES, SIZE_COLUMNS, SIZE_LABEL, accepts, defaultMetric, metricLabel } from './widget-model';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatFormField } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';

/** The dashboard as widgets: view mode, or edit mode with a gallery, resize, drag to arrange, remove, presets and reset. */
@Component({
  selector: 'dy-widget-board',
  imports: [SuSegmentedControl, PdSelect, MatFormField, MatInput, MatButton, MatIconButton, WidgetView, Icon, CdkDropList, CdkDrag, CdkDragHandle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="bar">
      @if (!store.editing()) {
        <span class="faint">{{ widgetCount() }}</span>
        <span class="spacer"></span>
        <button type="button" matButton="outlined" (click)="store.startEdit()"><dy-icon name="settings" /> Edit dashboard</button>
      } @else {
        <div class="field inline">
          <span class="visually-hidden">Preset</span>
          <pd-select [options]="presetOptions()" [value]="presetPick()" placeholder="Preset…" ariaLabel="Preset" (valueChange)="usePreset($event)" />
        </div>
        <button type="button" matButton="outlined" [attr.aria-expanded]="gallery()" [disabled]="!store.canAdd()" (click)="gallery.set(!gallery())"><dy-icon name="plus" /> Add widget</button>
        <button type="button" matButton="text" (click)="store.reset()"><dy-icon name="restore" /> Reset to default</button>
        @if (naming()) {
          <mat-form-field><input matInput type="text" maxlength="64" aria-label="Preset name" placeholder="Preset name" [value]="presetName()" (input)="presetName.set($any($event.target).value)" (keydown.enter)="savePreset()" /></mat-form-field>
          <button type="button" matButton="outlined" [disabled]="!presetName().trim() || settings.saving()" (click)="savePreset()">Save preset</button>
        } @else {
          <button type="button" matButton="text" (click)="naming.set(true)"><dy-icon name="plus" /> Save as preset</button>
        }
        <span class="spacer"></span>
        <button type="button" matButton="outlined" (click)="store.cancel()">Cancel</button>
        <button type="button" matButton="filled" [disabled]="!store.dirty() || settings.saving()" (click)="store.save()">Save dashboard</button>
      }
    </div>

    @if (store.editing() && gallery()) {
      <section class="gallery card" aria-label="Widget gallery">
        <div class="card-head"><h2>Add a widget</h2></div>
        <div class="tiles">
          @for (k of kinds; track k) {
            <button type="button" class="gtile" (click)="add(k)">
              <span class="pv"><dy-icon [name]="icon(k)" /></span>
              <strong>{{ kindLabel(k) }}</strong>
              <small>{{ hint(k) }}</small>
            </button>
          }
        </div>
      </section>
    }

    @if (store.widgets().length === 0) {
      <div class="empty card">
        <dy-icon name="dashboard" />
        <strong>No widgets yet</strong>
        <span>Choose Edit dashboard, then Add widget or pick a preset.</span>
      </div>
    }

    <div class="grid" cdkDropList cdkDropListOrientation="mixed" [cdkDropListDisabled]="!store.editing()" (cdkDropListDropped)="drop($event)" [class.editing]="store.editing()">
      @for (w of store.widgets(); track w.id; let i = $index) {
        <div class="cell" cdkDrag [cdkDragDisabled]="!store.editing()" [style.grid-column]="'span ' + span(w)" [attr.data-id]="w.id">
          <dy-widget [widget]="w" />
          @if (store.editing()) {
            <div class="edit" [class.small]="w.size === 's'" role="toolbar" [attr.aria-label]="'Edit ' + w.id">
              <button type="button" matIconButton class="grab" cdkDragHandle aria-label="Drag to move"><dy-icon name="more" /></button>
              <button type="button" matIconButton aria-label="Move earlier" [disabled]="i === 0" (click)="store.move(i, i - 1)"><dy-icon name="chevron-left" /></button>
              <button type="button" matIconButton aria-label="Move later" [disabled]="i === store.widgets().length - 1" (click)="store.move(i, i + 1)"><dy-icon name="chevron-right" /></button>
              <pd-segmented-control label="Size" [options]="sizeSegs" [value]="w.size" (valueChange)="resize(w, $event)" />
              @if (!panel(w.kind)) {
                <pd-select [options]="metricsFor(w)" [value]="metricName(w)" [ariaLabel]="'Metric of ' + w.id" (valueChange)="metric(w, $event)" />
              }
              <span class="spacer"></span>
              <button type="button" matIconButton class="pd-button-danger" aria-label="Remove widget" (click)="store.remove(w.id)"><dy-icon name="trash" /></button>
            </div>
          }
        </div>
      }
    </div>
  `,
  styles: `
    :host { display: block; }
    .bar { display: flex; align-items: center; gap: var(--sp-2); margin-bottom: var(--sp-3); flex-wrap: wrap; }
    .spacer { flex: 1; } .faint { color: var(--text-3); font-size: var(--fs-sm); }
    .field.inline { display: inline-grid; }
    .gallery { margin-bottom: var(--sp-3); padding-bottom: var(--sp-3); }
    .tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: var(--sp-2); padding: var(--sp-2) var(--sp-4) 0; }
    .gtile { display: grid; gap: 2px; justify-items: start; padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); color: var(--text-1); font: inherit; text-align: left; cursor: default; }
    .gtile:hover { border-color: var(--border-strong); background: var(--surface-hover); }
    .gtile small { color: var(--text-3); font-size: var(--fs-xs); }
    .pv { display: grid; place-items: center; width: 32px; height: 32px; margin-bottom: var(--sp-1); border-radius: var(--r-md); background: var(--surface-active); color: var(--text-2); }
    .empty { display: grid; justify-items: center; gap: var(--sp-1); padding: var(--sp-10); color: var(--text-2); }
    .empty dy-icon { --icon-size: 28px; color: var(--text-3); }
    .grid { display: grid; grid-template-columns: repeat(12, minmax(0, 1fr)); gap: var(--sp-3); align-items: stretch; }
    .cell { position: relative; min-width: 0; display: grid; grid-template-rows: 1fr auto; gap: var(--sp-1); }
    .cell dy-widget { height: 100%; }
    .editing .cell dy-widget { outline: 1px dashed var(--border-strong); outline-offset: 2px; }
    .edit { display: flex; flex-wrap: nowrap; min-width: 0; align-items: center; gap: 2px; padding: 3px; border: 1px solid var(--border); border-radius: var(--r-md); background: var(--overlay-bg); box-shadow: var(--shadow-2); }
    .edit pd-segmented-control { margin: 0 2px; } .edit pd-select { flex: 1 1 9rem; min-width: 9rem; width: auto; } .edit .spacer { flex: 0 0 0; }
    /* A small widget is narrower than the whole toolbar: its metric gets a second row of its own, full width. */
    .edit.small { flex-wrap: wrap; } .edit.small pd-select { order: 9; flex: 1 1 100%; }
    .grab { cursor: grab; }
    .cdk-drag-preview { box-shadow: var(--shadow-3); border-radius: var(--r-lg); }
    .cdk-drag-placeholder { opacity: 0.35; }
    .cdk-drag-animating { transition: transform 200ms var(--ease-out); }
    @container dash (max-width: 720px) { .cell { grid-column: span 12 !important; } }
  `,
})
export class WidgetBoard {
  protected readonly store = inject(DashboardLayoutStore);
  protected readonly settings = inject(SettingsStore);
  private readonly counters = inject(CountersStore);
  protected readonly gallery = signal(false);
  protected readonly naming = signal(false);
  protected readonly presetName = signal('');
  protected readonly kinds = KINDS;
  protected readonly panel = (k: WidgetKind) => PANEL_KINDS.includes(k);
  protected readonly sizes = SIZES;
  protected readonly sizeSegs: SuSegment[] = SIZES.map((s) => ({ value: s, label: s.toUpperCase() }));
  protected resize(w: Widget, value: string): void {
    const size = SIZES.find((s) => s === value);
    if (size) this.store.resize(w.id, size);
  }
  protected readonly widgetCount = computed(() => `${this.store.widgets().length.toString()} widgets`);
  protected readonly kindLabel = (k: WidgetKind) => KIND_LABEL[k];
  protected readonly hint = (k: WidgetKind) => KIND_HINT[k];
  protected readonly sizeLabel = (s: 's' | 'm' | 'l') => SIZE_LABEL[s];
  protected readonly span = (w: Widget) => SIZE_COLUMNS[w.size];
  protected readonly icon = (k: WidgetKind) => ICONS[k];

  protected readonly metricName = (w: Widget) => metricLabel(w.metric);
  protected readonly metricsFor = (w: Widget): PdSelectOption[] => METRICS.filter((m) => accepts(w.kind, m.metric)).map((m) => ({ value: m.label, label: m.label }));
  protected readonly presetOptions = computed<PdSelectOption[]>(() => this.store.presets().map((p) => ({ value: p.id, label: p.name })));
  /** The preset just applied, shown until the layout changes again; then the box is back to its prompt. */
  protected readonly presetPick = linkedSignal({ source: () => this.store.widgets(), computation: () => '' });

  constructor() {
    // The cards below read these; load them once when the board opens.
    inject(HealthStore).loadReport().catch(() => undefined);
    void inject(DiagnosisStore).loadAnomalies();
    // Counters are read only while a widget shows one; the engine reads them at most every two seconds.
    let release: (() => void) | null = null;
    inject(DestroyRef).onDestroy(() => release?.());
    effect(() => {
      const paths = widgetCounterPaths(this.store.widgets());
      this.counters.track(paths);
      if (paths.length > 0 && !release) release = this.counters.use();
      else if (paths.length === 0 && release) {
        release();
        release = null;
      }
    });
  }

  protected add(kind: WidgetKind): void {
    this.store.add(kind, defaultMetric(kind));
    this.gallery.set(false);
  }
  protected async savePreset(): Promise<void> {
    if (await this.store.saveAsPreset(this.presetName())) {
      this.naming.set(false);
      this.presetName.set('');
    }
  }
  protected drop(e: CdkDragDrop<unknown>): void {
    this.store.move(e.previousIndex, e.currentIndex);
  }
  protected usePreset(id: string): void {
    if (!id) return;
    this.presetPick.set(id);
    this.store.usePreset(id);
  }
  protected metric(w: Widget, label: string): void {
    const metric = METRICS.find((m) => m.label === label)?.metric;
    if (metric) this.store.setMetric(w.id, metric as WidgetMetric);
  }
}

const ICONS: Record<WidgetKind, 'trend' | 'gauge' | 'reports' | 'cpu' | 'processes' | 'alert' | 'health' | 'why' | 'bell' | 'disk' | 'calendar'> = {
  graph: 'trend',
  bigNumber: 'gauge',
  bars: 'reports',
  ring: 'gauge',
  heatmap: 'cpu',
  topProcesses: 'processes',
  issues: 'alert',
  health: 'health',
  cpu: 'cpu',
  diagnosis: 'why',
  events: 'bell',
  history: 'trend',
  consumers: 'processes',
  diskTrend: 'disk',
};
