import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { MatFormField, MatLabel } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { PdWidgetChooser, PdWidgetOption } from '@decryp7/paddock-ui/widgets';
import type { Edit, MenuBarItem, MenuBarKind, Metric, NumberFormat, Placement } from '../../core/generated/widgets';
import { Notifier } from '../../shared/notifier';
import { WidgetLayouts } from '../../state/widget-layouts';
import { optionLabel } from '../widgets/option-label';
import { WidgetChoicesStore } from '../../state/widget-choices';

/** The two ways a number draws (the engine's `NumberFormat`). */
const FORMAT_OPTIONS: PdSelectOption[] = [
  { value: 'stacked', label: 'Label over value' },
  { value: 'line', label: 'One line' },
];

const metricOf = (item: MenuBarItem): Metric => Object.values(item)[0]!.metric;

/**
 * Settings > Menu bar items: the shared chooser over the menu-bar layout (family ADR 0035). Each item has a style (its kind),
 * a metric and, for a number, a short label and unit. Edits go to the engine at once.
 */
@Component({
  selector: 'dy-menu-bar-items',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatFormField, MatLabel, MatInput, PdSelect, PdWidgetChooser, PdWidgetOption],
  template: `
    @if (view(); as v) {
      @if (choices.ready()) {
      <pd-widget-chooser [view]="v" [widgetFor]="widgetFor" (edit)="edit($event)">
        @for (kind of kinds(); track kind) {
          <ng-template [pdWidgetOption]="kind" [pdWidgetOptionOf]="sample" let-p>
            <pd-select [class.narrow]="kind === 'number'" [options]="metricOptions()" [value]="metricName(p)" [ariaLabel]="label('Metric', p)" (valueChange)="setMetric(p, $event)" />
            @if (kind === 'number') {
              <mat-form-field class="short"><mat-label>Label</mat-label><input matInput maxlength="16" [attr.aria-label]="label('Label', p)" [value]="field(p, 'label')" (change)="commit(p, 'label', $event)" /></mat-form-field>
              <mat-form-field class="short"><mat-label>Unit</mat-label><input matInput maxlength="8" [attr.aria-label]="label('Unit', p)" [value]="field(p, 'unit')" (change)="commit(p, 'unit', $event)" /></mat-form-field>
              <pd-select class="format" [options]="formatOptions" [value]="format(p)" [ariaLabel]="label('Format', p)" (valueChange)="setFormat(p, $event)" />
            }
          </ng-template>
        }
      </pd-widget-chooser>
      @if (v.layout.placements.length > 4) {
        <p class="note" role="note">
          {{ overflowNote }}
        </p>
      }
      }
    } @else {
      <p class="faint" role="status">Reading the menu bar items…</p>
    }
  `,
  styles: `
    :host { display: block; }
    .short { width: 5rem; flex: none; }
    /* A number row also holds its label and unit: the metric picker gives way so the description keeps its width. */
    .narrow { width: 9.5rem; flex: none; }
    .faint { color: var(--text-3); }
    .format { width: 9rem; flex: none; }
    .note { color: var(--text-2); font-size: var(--fs-sm); margin-block-start: var(--sp-3); }
  `,
})
export class MenuBarItems implements OnInit {
  private readonly layouts = inject(WidgetLayouts);
  private readonly notifier = inject(Notifier);
  protected readonly view = this.layouts.menuBar;
  /** The type witness of the option templates (`pdWidgetOptionOf`). */
  protected readonly sample: MenuBarItem | null = null;
  protected readonly formatOptions = FORMAT_OPTIONS;
  /** Said once there are more than four items: what each system does with items that do not fit. */
  protected readonly overflowNote =
    'More than four items may not all fit. On a Mac the menu bar hides the items that do not fit, without a sign, when the active app\'s menus need the room or a notch is in the way; the stacked number is the narrowest format. On Windows 11 icons you have not pinned to the taskbar go to the overflow flyout (the chevron); each keeps its place there.';
  protected readonly choices = inject(WidgetChoicesStore);
  protected readonly widgetFor = (kind: MenuBarKind): MenuBarItem => this.choices.menuBarDefault(kind);
  protected readonly kinds = computed(() => this.view()?.catalogue.kinds.map((k) => k.kind) ?? []);
  protected readonly metricOptions = computed<PdSelectOption[]>(() => this.choices.menuBarMetrics().map((m) => ({ value: m.title, label: m.title })));

  ngOnInit(): void {
    void this.layouts.use('menuBar');
    void this.choices.load();
  }

  protected format(p: Placement<MenuBarItem>): NumberFormat {
    return typeof p.widget === 'object' && 'number' in p.widget ? p.widget.number.format : 'stacked';
  }

  protected setFormat(p: Placement<MenuBarItem>, value: string): void {
    const format: NumberFormat = value === 'line' ? 'line' : 'stacked';
    this.editRow(p.id, (now) => (typeof now.widget === 'object' && 'number' in now.widget ? { number: { ...now.widget.number, format } } : null));
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

  /** Edits of a row go one after the other, each made on the row as the engine has it after the one before. */
  private queue: Promise<void> = Promise.resolve();

  private editRow(id: string, change: (now: Placement<MenuBarItem>) => MenuBarItem | null): void {
    // A failed edit is reported by `edit` itself; whatever else throws must not stop the edits queued behind it.
    this.queue = this.queue
      .then(async () => {
        const now = this.view()?.layout.placements.find((x) => x.id === id);
        const widget = now ? change(now) : null;
        if (now && widget) await this.edit({ replace: { id, widget } });
      })
      .catch(() => undefined);
  }

  protected setMetric(p: Placement<MenuBarItem>, title: string): void {
    const metric = this.choices.menuBarMetrics().find((m) => m.title === title)?.metric;
    if (!metric) return;
    this.editRow(p.id, (now) => {
      const [kind, body] = Object.entries(now.widget)[0]!;
      return { [kind]: { ...body, metric } } as MenuBarItem;
    });
  }

  protected label(what: string, p: Placement<MenuBarItem>): string {
    return optionLabel(what, this.view(), p);
  }

  /** A typed Label or Unit is sent when the field is left or Enter is pressed (the `change` event), not on every key. */
  protected commit(p: Placement<MenuBarItem>, key: 'label' | 'unit', e: Event): void {
    this.setNumber(p, key, (e.target as HTMLInputElement).value);
  }

  protected setNumber(p: Placement<MenuBarItem>, key: 'label' | 'unit', value: string): void {
    this.editRow(p.id, (now) => (typeof now.widget === 'object' && 'number' in now.widget ? { number: { ...now.widget.number, [key]: value } } : null));
  }
}
