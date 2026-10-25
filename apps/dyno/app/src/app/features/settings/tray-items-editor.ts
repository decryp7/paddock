import { CdkDrag, CdkDragDrop, CdkDragHandle, CdkDropList } from '@angular/cdk/drag-drop';
import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';
import { FormsModule } from '@angular/forms';
import type { TrayItem, WidgetMetric } from '../../core/api.types';
import { formatPercent } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Sparkline } from '../../shared/sparkline';
import { LiveStore } from '../../state/live.store';
import { MAX_TRAY_ITEMS, METRICS, TRAY_STYLES, type TrayStyleId, metricLabel, moveWidget, newTrayItem, readMetric, seriesFor, styleFromId, trayStyleId } from '../widgets/widget-model';

/**
 * The menu-bar / tray items: up to four, each a metric drawn in a style, reordered by dragging, with a live preview of
 * the menu bar above the list. Edits go up as a new array; saving is the Settings page's Save.
 */
@Component({
  selector: 'dy-tray-items-editor',
  imports: [FormsModule, Icon, Sparkline, CdkDropList, CdkDrag, CdkDragHandle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="preview" role="img" [attr.aria-label]="previewLabel()">
      <span class="logo"><dy-icon name="dashboard" /></span>
      @for (it of items(); track it.id) {
        <span class="item" [attr.data-style]="styleId(it)">
          @switch (styleId(it)) {
            @case ('lineGraph') {
              <span class="spark"><dy-sparkline [values]="vals(it)" [max]="max(it)" color="currentColor" [fill]="false" /></span>
            }
            @case ('graphAndNumber') {
              <span class="spark"><dy-sparkline [values]="vals(it)" [max]="max(it)" color="currentColor" [fill]="false" /></span>
              <span class="n num">{{ read(it).text[0] }}</span>
            }
            @case ('verticalBars') {
              <span class="vbars"><i [style.height.%]="level(it)"></i></span>
            }
            @case ('horizontalMeter') {
              <span class="meter"><i [style.width.%]="level(it)"></i></span>
            }
            @case ('ring') {
              <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true"><circle cx="10" cy="10" r="7" fill="none" stroke="currentColor" stroke-opacity="0.25" stroke-width="3" /><circle cx="10" cy="10" r="7" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" [attr.stroke-dasharray]="ringDash(it)" transform="rotate(-90 10 10)" /></svg>
            }
            @default {
              <span class="n num">{{ numberLabel(it) }}{{ read(it).text[0] }}{{ numberUnit(it) || read(it).text[1] }}</span>
            }
          }
        </span>
      } @empty {
        <span class="faint">No items: only the Dyno icon shows.</span>
      }
    </div>

    <ul cdkDropList (cdkDropListDropped)="drop($event)" aria-label="Menu bar items">
      @for (it of items(); track it.id; let i = $index) {
        <li cdkDrag>
          <button type="button" class="btn ghost icon sm grab" cdkDragHandle aria-label="Drag to reorder"><dy-icon name="more" /></button>
          <button type="button" class="btn ghost icon sm" aria-label="Move up" [disabled]="i === 0" (click)="move(i, i - 1)"><dy-icon name="arrow-up" /></button>
          <button type="button" class="btn ghost icon sm" aria-label="Move down" [disabled]="i === items().length - 1" (click)="move(i, i + 1)"><dy-icon name="arrow-down" /></button>
          <select class="select sm" [attr.aria-label]="'Metric of item ' + (i + 1)" (change)="setMetric(it, $event)">
            @for (m of metrics; track m.label) {
              <option [value]="m.metric" [selected]="m.label === metricName(it)">{{ m.label }}</option>
            }
          </select>
          <select class="select sm" [attr.aria-label]="'Style of item ' + (i + 1)" (change)="setStyle(it, $event)">
            @for (s of styles; track s.id) {
              <option [value]="s.id" [selected]="s.id === styleId(it)">{{ s.label }}</option>
            }
          </select>
          @if (styleId(it) === 'number') {
            <input class="input sm short" aria-label="Short label" placeholder="Label" maxlength="16" [ngModel]="numberLabel(it)" (ngModelChange)="setNumber(it, 'label', $event)" />
            <input class="input sm short" aria-label="Unit" placeholder="Unit" maxlength="8" [ngModel]="numberUnit(it)" (ngModelChange)="setNumber(it, 'unit', $event)" />
          }
          <span class="spacer"></span>
          <button type="button" class="btn ghost icon sm danger" aria-label="Remove item" (click)="remove(it)"><dy-icon name="trash" /></button>
        </li>
      }
    </ul>
    <button type="button" class="btn sm" [disabled]="items().length >= max_()" (click)="add()"><dy-icon name="plus" /> Add item</button>
    @if (items().length >= max_()) {
      <small class="faint">The menu bar holds up to {{ max_() }} items.</small>
    }
  `,
  styles: `
    :host { display: grid; gap: var(--sp-3); justify-items: start; }
    .preview { display: flex; align-items: center; gap: var(--sp-4); align-self: stretch; height: 30px; padding: 0 var(--sp-3); border-radius: var(--r-md); background: linear-gradient(90deg, color-mix(in srgb, var(--text-1) 8%, var(--surface-inset)), var(--surface-inset)); border: 1px solid var(--border); color: var(--text-1); }
    .logo { display: grid; color: var(--text-2); } .logo dy-icon { --icon-size: 14px; }
    .item { display: inline-flex; align-items: center; gap: 5px; height: 20px; font-size: var(--fs-sm); }
    .spark { display: inline-block; width: 34px; height: 14px; --spark-h: 14px; }
    .vbars { display: inline-flex; align-items: flex-end; width: 6px; height: 16px; border-radius: 2px; background: color-mix(in srgb, currentColor 22%, transparent); overflow: hidden; }
    .vbars i { display: block; width: 100%; background: currentColor; }
    .meter { display: inline-block; width: 34px; height: 6px; border-radius: 3px; background: color-mix(in srgb, currentColor 22%, transparent); overflow: hidden; }
    .meter i { display: block; height: 100%; background: currentColor; }
    .n { font-weight: var(--fw-medium); }
    .faint { color: var(--text-3); font-size: var(--fs-sm); }
    ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); align-self: stretch; }
    li { display: flex; align-items: center; gap: var(--sp-2); padding: 4px var(--sp-2); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); }
    .spacer { flex: 1; } .short { width: 84px; } .grab { cursor: grab; }
    .cdk-drag-preview { box-shadow: var(--shadow-3); border-radius: var(--r-md); background: var(--overlay-bg); display: flex; align-items: center; gap: var(--sp-2); padding: 4px var(--sp-2); }
    .cdk-drag-placeholder { opacity: 0.35; }
  `,
})
export class TrayItemsEditor {
  readonly items = input.required<TrayItem[]>();
  readonly itemsChange = output<TrayItem[]>();
  private readonly live = inject(LiveStore);
  protected readonly metrics = METRICS;
  protected readonly styles = TRAY_STYLES;
  protected readonly max_ = () => MAX_TRAY_ITEMS;
  protected readonly styleId = trayStyleId;
  protected readonly metricName = (it: TrayItem) => metricLabel(it.metric);
  protected readonly previewLabel = computed(() => `Menu bar preview: ${this.items().map((i) => metricLabel(i.metric)).join(', ') || 'no items'}`);

  protected read(it: TrayItem) {
    return readMetric(it.metric, this.live.system());
  }
  protected vals(it: TrayItem): number[] {
    return seriesFor(it.metric, this.live.series())?.values ?? [];
  }
  protected max(it: TrayItem): number | null {
    return this.read(it).percent !== null ? 100 : null;
  }
  protected level(it: TrayItem): number {
    return this.read(it).percent ?? 0;
  }
  protected ringDash(it: TrayItem): string {
    const c = 2 * Math.PI * 7;
    return `${((this.level(it) / 100) * c).toFixed(1)} ${c.toFixed(1)}`;
  }
  protected numberLabel(it: TrayItem): string {
    return typeof it.style === 'object' ? it.style.number.label : '';
  }
  protected numberUnit(it: TrayItem): string {
    return typeof it.style === 'object' ? it.style.number.unit : '';
  }
  protected readonly pct = formatPercent;

  private emit(items: TrayItem[]): void {
    this.itemsChange.emit(items);
  }
  protected add(): void {
    if (this.items().length < MAX_TRAY_ITEMS) this.emit([...this.items(), newTrayItem('cpuTotal', 'lineGraph', this.items())]);
  }
  protected remove(it: TrayItem): void {
    this.emit(this.items().filter((i) => i.id !== it.id));
  }
  protected move(from: number, to: number): void {
    this.emit(moveWidget(this.items(), from, to));
  }
  protected drop(e: CdkDragDrop<unknown>): void {
    this.move(e.previousIndex, e.currentIndex);
  }
  protected setMetric(it: TrayItem, e: Event): void {
    this.emit(this.items().map((i) => (i.id === it.id ? { ...i, metric: (e.target as HTMLSelectElement).value as WidgetMetric } : i)));
  }
  protected setStyle(it: TrayItem, e: Event): void {
    const id = (e.target as HTMLSelectElement).value as TrayStyleId;
    this.emit(this.items().map((i) => (i.id === it.id ? { ...i, style: styleFromId(id, this.numberUnit(i), this.numberLabel(i)) } : i)));
  }
  protected setNumber(it: TrayItem, field: 'unit' | 'label', value: string): void {
    this.emit(this.items().map((i) => (i.id === it.id ? { ...i, style: { number: { unit: field === 'unit' ? value : this.numberUnit(i), label: field === 'label' ? value : this.numberLabel(i) } } } : i)));
  }
}
