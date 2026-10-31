import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatFormField, MatLabel } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { PdWidgetChooser, PdWidgetOption } from '@decryp7/paddock-ui/widgets';
import type { Edit, MenuBarItem, MenuBarKind, Metric, Placement } from '../../core/generated/widgets';
import { Notifier } from '../../shared/notifier';
import { WidgetLayouts } from '../../state/widget-layouts';
import { WidgetChoicesStore } from '../../state/widget-choices';

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
      @if (choices.ready()) {
      <pd-widget-chooser [view]="v" [widgetFor]="widgetFor" (edit)="edit($event)">
        @for (kind of kinds(); track kind) {
          <ng-template [pdWidgetOption]="kind" [pdWidgetOptionOf]="sample" let-p>
            <pd-select [options]="metricOptions()" [value]="metricName(p)" [ariaLabel]="'Metric of ' + p.id" (valueChange)="setMetric(p, $event)" />
            @if (kind === 'number') {
              <mat-form-field class="short"><mat-label>Label</mat-label><input matInput maxlength="16" [ngModel]="field(p, 'label')" (ngModelChange)="setNumber(p, 'label', $event)" /></mat-form-field>
              <mat-form-field class="short"><mat-label>Unit</mat-label><input matInput maxlength="8" [ngModel]="field(p, 'unit')" (ngModelChange)="setNumber(p, 'unit', $event)" /></mat-form-field>
            }
          </ng-template>
        }
      </pd-widget-chooser>
      }
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
  /** The type witness of the option templates (`pdWidgetOptionOf`). */
  protected readonly sample: MenuBarItem | null = null;
  protected readonly choices = inject(WidgetChoicesStore);
  protected readonly widgetFor = (kind: MenuBarKind): MenuBarItem => this.choices.menuBarDefault(kind);
  protected readonly kinds = computed(() => this.view()?.catalogue.kinds.map((k) => k.kind) ?? []);
  protected readonly metricOptions = computed<PdSelectOption[]>(() => this.choices.menuBarMetrics().map((m) => ({ value: m.title, label: m.title })));

  ngOnInit(): void {
    void this.layouts.use('menuBar');
    void this.choices.load();
  }

  protected metricName(p: Placement<MenuBarItem>): string {
    return this.choices.title(metricOf(p.widget));
  }

  protected field(p: Placement<MenuBarItem>, key: 'label' | 'unit'): string {
    return typeof p.widget === 'object' && 'number' in p.widget ? p.widget.number[key] : '';
  }

  protected async edit(e: Edit<MenuBarItem>): Promise<void> {
    const failed = await this.layouts.edit('menuBar', e);
    if (failed) this.notifier.error(failed);
  }

  protected setMetric(p: Placement<MenuBarItem>, title: string): void {
    const metric = this.choices.menuBarMetrics().find((m) => m.title === title)?.metric;
    if (!metric) return;
    const [kind, body] = Object.entries(p.widget)[0]!;
    void this.edit({ replace: { id: p.id, widget: { [kind]: { ...body, metric } } as MenuBarItem } });
  }

  protected setNumber(p: Placement<MenuBarItem>, key: 'label' | 'unit', value: string): void {
    if (typeof p.widget !== 'object' || !('number' in p.widget)) return;
    void this.edit({ replace: { id: p.id, widget: { number: { ...p.widget.number, [key]: value } } } });
  }
}
