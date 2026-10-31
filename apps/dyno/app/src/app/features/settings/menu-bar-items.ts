import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatFormField, MatLabel } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { PdWidgetChooser, PdWidgetOption } from '@decryp7/paddock-ui/widgets';
import type { Edit, MenuBarItem, MenuBarKind, Metric, Placement } from '../../core/generated/widgets';
import { Notifier } from '../../shared/notifier';
import { WidgetLayouts } from '../../state/widget-layouts';
import { METRIC_CHOICES } from '../widgets/metric-reading';

/** A new menu-bar item of `kind` (CPU); the engine validates it. */
export function newMenuBarItem(kind: MenuBarKind): MenuBarItem {
  const metric: Metric = 'cpuTotal';
  return kind === 'number' ? { number: { metric, unit: '%', label: 'CPU' } } : ({ [kind]: { metric } } as MenuBarItem);
}

const metricOf = (item: MenuBarItem): Metric => Object.values(item)[0]!.metric;

/**
 * Settings > Menu bar items: the shared chooser over the menu-bar layout (family ADR 0035). Each item has a style (its kind),
 * a metric and, for a number, a short label and unit. Edits go to the engine at once.
 */
@Component({
  selector: 'dy-menu-bar-items',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [FormsModule, MatFormField, MatLabel, MatInput, PdSelect, PdWidgetChooser, PdWidgetOption],
  template: `
    @if (view(); as v) {
      <pd-widget-chooser [view]="v" [widgetFor]="widgetFor" (edit)="edit($event)">
        @for (kind of kinds; track kind) {
          <ng-template [pdWidgetOption]="kind" let-p>
            <pd-select [options]="metricOptions" [value]="metricName(p)" [ariaLabel]="'Metric of ' + p.id" (valueChange)="setMetric(p, $event)" />
            @if (kind === 'number') {
              <mat-form-field class="short"><mat-label>Label</mat-label><input matInput maxlength="16" [ngModel]="field(p, 'label')" (ngModelChange)="setNumber(p, 'label', $event)" /></mat-form-field>
              <mat-form-field class="short"><mat-label>Unit</mat-label><input matInput maxlength="8" [ngModel]="field(p, 'unit')" (ngModelChange)="setNumber(p, 'unit', $event)" /></mat-form-field>
            }
          </ng-template>
        }
      </pd-widget-chooser>
    } @else {
      <p class="faint" role="status">Reading the menu bar items…</p>
    }
  `,
  styles: `
    :host { display: block; }
    .short { width: 7rem; flex: none; }
    .faint { color: var(--text-3); }
  `,
})
export class MenuBarItems implements OnInit {
  private readonly layouts = inject(WidgetLayouts);
  private readonly notifier = inject(Notifier);
  protected readonly view = this.layouts.menuBar;
  protected readonly widgetFor = newMenuBarItem;
  protected readonly kinds: readonly MenuBarKind[] = ['lineGraph', 'verticalBars', 'horizontalMeter', 'ring', 'number', 'graphAndNumber'];
  protected readonly metricOptions: PdSelectOption[] = METRIC_CHOICES.map((m) => ({ value: m.label, label: m.label }));

  ngOnInit(): void {
    void this.layouts.use('menuBar');
  }

  protected metricName(placement: Placement<unknown>): string {
    const p = placement as Placement<MenuBarItem>;
    const m = metricOf(p.widget);
    return METRIC_CHOICES.find((c) => c.metric === m)?.label ?? '';
  }

  protected field(placement: Placement<unknown>, key: 'label' | 'unit'): string {
    const p = placement as Placement<MenuBarItem>;
    return typeof p.widget === 'object' && 'number' in p.widget ? p.widget.number[key] : '';
  }

  protected async edit(e: Edit<MenuBarItem>): Promise<void> {
    const failed = await this.layouts.edit('menuBar', e);
    if (failed) this.notifier.error(failed);
  }

  protected setMetric(placement: Placement<unknown>, label: string): void {
    const p = placement as Placement<MenuBarItem>;
    const metric = METRIC_CHOICES.find((m) => m.label === label)?.metric;
    if (!metric) return;
    const [kind, body] = Object.entries(p.widget)[0]!;
    void this.edit({ replace: { id: p.id, widget: { [kind]: { ...body, metric } } as MenuBarItem } });
  }

  protected setNumber(placement: Placement<unknown>, key: 'label' | 'unit', value: string): void {
    const p = placement as Placement<MenuBarItem>;
    if (typeof p.widget !== 'object' || !('number' in p.widget)) return;
    void this.edit({ replace: { id: p.id, widget: { number: { ...p.widget.number, [key]: value } } } });
  }
}
