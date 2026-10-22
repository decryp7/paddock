import { CdkDrag, CdkDragDrop, CdkDragHandle, CdkDropList } from '@angular/cdk/drag-drop';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { Widget, WidgetKind, WidgetMetric } from '../../core/api.types';
import { Icon } from '../../shared/icon';
import { DashboardLayoutStore } from '../../state/dashboard-layout.store';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { HealthStore } from '../../state/health.store';
import { SettingsStore } from '../../state/settings.store';
import { WidgetView } from './widget-view';
import { KINDS, KIND_HINT, KIND_LABEL, METRICS, SIZES, SIZE_COLUMNS, SIZE_LABEL, accepts, defaultMetric, metricLabel } from './widget-model';

/** The dashboard as widgets: view mode, or edit mode with a gallery, resize, drag to arrange, remove, presets and reset. */
@Component({
  selector: 'dy-widget-board',
  imports: [WidgetView, Icon, CdkDropList, CdkDrag, CdkDragHandle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="bar">
      @if (!store.editing()) {
        <span class="faint">{{ widgetCount() }}</span>
        <span class="spacer"></span>
        <button type="button" class="btn" (click)="store.startEdit()"><dy-icon name="settings" /> Edit dashboard</button>
      } @else {
        <label class="field inline">
          <span class="visually-hidden">Preset</span>
          <select class="select sm" aria-label="Preset" (change)="usePreset($event)">
            <option value="" selected disabled>Preset…</option>
            @for (p of store.presets(); track p.id) {
              <option [value]="p.id">{{ p.name }}</option>
            }
          </select>
        </label>
        <button type="button" class="btn sm" [attr.aria-expanded]="gallery()" [disabled]="!store.canAdd()" (click)="gallery.set(!gallery())"><dy-icon name="plus" /> Add widget</button>
        <button type="button" class="btn sm ghost" (click)="store.reset()"><dy-icon name="restore" /> Reset</button>
        <span class="spacer"></span>
        <button type="button" class="btn sm" (click)="store.cancel()">Cancel</button>
        <button type="button" class="btn sm primary" [disabled]="!store.dirty() || settings.saving()" (click)="store.save()">Save dashboard</button>
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
            <div class="edit" role="toolbar" [attr.aria-label]="'Edit ' + w.id">
              <button type="button" class="btn ghost icon sm grab" cdkDragHandle aria-label="Drag to move"><dy-icon name="more" /></button>
              <button type="button" class="btn ghost icon sm" aria-label="Move earlier" [disabled]="i === 0" (click)="store.move(i, i - 1)"><dy-icon name="chevron-left" /></button>
              <button type="button" class="btn ghost icon sm" aria-label="Move later" [disabled]="i === store.widgets().length - 1" (click)="store.move(i, i + 1)"><dy-icon name="chevron-right" /></button>
              <span class="seg sm" role="group" aria-label="Size">
                @for (s of sizes; track s) {
                  <button type="button" [attr.aria-pressed]="w.size === s" [title]="sizeLabel(s)" (click)="store.resize(w.id, s)">{{ s.toUpperCase() }}</button>
                }
              </span>
              @if (w.kind !== 'health' && w.kind !== 'issues') {
                <select class="select sm" [attr.aria-label]="'Metric of ' + w.id" (change)="metric(w, $event)">
                  @for (m of metricsFor(w); track m.label) {
                    <option [value]="m.value" [selected]="m.label === metricName(w)">{{ m.label }}</option>
                  }
                </select>
              }
              <span class="spacer"></span>
              <button type="button" class="btn ghost icon sm danger" aria-label="Remove widget" (click)="store.remove(w.id)"><dy-icon name="trash" /></button>
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
    .edit { display: flex; flex-wrap: wrap; align-items: center; gap: 2px; padding: 3px; border: 1px solid var(--border); border-radius: var(--r-md); background: var(--overlay-bg); box-shadow: var(--shadow-2); }
    .edit .seg { margin: 0 2px; } .edit .select { width: 120px; max-width: 100%; }
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
  protected readonly gallery = signal(false);
  protected readonly kinds = KINDS;
  protected readonly sizes = SIZES;
  protected readonly widgetCount = computed(() => `${this.store.widgets().length.toString()} widgets`);
  protected readonly kindLabel = (k: WidgetKind) => KIND_LABEL[k];
  protected readonly hint = (k: WidgetKind) => KIND_HINT[k];
  protected readonly sizeLabel = (s: 's' | 'm' | 'l') => SIZE_LABEL[s];
  protected readonly span = (w: Widget) => SIZE_COLUMNS[w.size];
  protected readonly icon = (k: WidgetKind) => ICONS[k];
  protected readonly metricName = (w: Widget) => metricLabel(w.metric);
  protected readonly metricsFor = (w: Widget) => METRICS.filter((m) => accepts(w.kind, m.metric)).map((m) => ({ value: m.metric as string, label: m.label }));

  constructor() {
    // The cards below read these; load them once when the board opens.
    inject(HealthStore).loadReport().catch(() => undefined);
    void inject(DiagnosisStore).loadAnomalies();
  }

  protected add(kind: WidgetKind): void {
    this.store.add(kind, defaultMetric(kind));
    this.gallery.set(false);
  }
  protected drop(e: CdkDragDrop<unknown>): void {
    this.store.move(e.previousIndex, e.currentIndex);
  }
  protected usePreset(e: Event): void {
    const el = e.target as HTMLSelectElement;
    this.store.usePreset(el.value);
    el.value = '';
  }
  protected metric(w: Widget, e: Event): void {
    this.store.setMetric(w.id, (e.target as HTMLSelectElement).value as WidgetMetric);
  }
}

const ICONS: Record<WidgetKind, 'trend' | 'gauge' | 'reports' | 'cpu' | 'processes' | 'alert' | 'health'> = {
  graph: 'trend',
  bigNumber: 'gauge',
  bars: 'reports',
  ring: 'gauge',
  heatmap: 'cpu',
  topProcesses: 'processes',
  issues: 'alert',
  health: 'health',
};
