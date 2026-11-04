import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { MatFormField, MatLabel } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { PdWidgetChooser, PdWidgetOption } from '@decryp7/paddock-ui/widgets';
import type { Edit, MenuBarItem, MenuBarKind, Metric, NumberFormat, Placement } from '../../core/generated/widgets';
import { Notifier } from '../../shared/notifier';
import { WidgetLayouts } from '../../state/widget-layouts';
import { optionLabel } from '../widgets/option-label';
import { WidgetChoicesStore } from '../../state/widget-choices';
import { TrayAvailabilityStore } from '../../state/tray-availability.store';

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
  imports: [MatFormField, MatLabel, MatInput, PdSelect, PdStatusGlyph, PdWidgetChooser, PdWidgetOption],
  template: `
    <!-- The engine's note when the desktop shows no tray icons (Linux without a StatusNotifier host). -->
    @if (tray.note(); as hint) {
      <p class="info warn" role="note" data-testid="tray-no-host"><pd-status-glyph status="warning" [size]="16" />{{ hint }}</p>
    }
    @if (view(); as v) {
      @if (choices.ready()) {
      <p class="sub">What the tray / menu-bar icon draws live. Add up to {{ v.catalogue.surface.maxPlacements }}, drag to reorder. Changes apply at once; Undo is in the message that follows.</p>
      @if (support().note; as why) {
        <p class="info" role="note"><pd-status-glyph status="info" [size]="16" />{{ why }}</p>
      }
      <pd-widget-chooser [view]="v" [widgetFor]="widgetFor" (edit)="edit($event)">
        @for (kind of kinds(); track kind) {
          <ng-template [pdWidgetOption]="kind" [pdWidgetOptionOf]="sample" let-p>
            <pd-select [class.narrow]="kind === 'number'" [options]="metricOptions()" [value]="metricName(p)" [ariaLabel]="label('Metric', p)" (valueChange)="setMetric(p, $event)" />
            @if (kind === 'number') {
              <span class="pair">
                <mat-form-field class="short"><mat-label>Label</mat-label><input matInput maxlength="16" [attr.aria-label]="label('Label', p)" [value]="field(p, 'label')" (change)="commit(p, 'label', $event)" /></mat-form-field>
                <mat-form-field class="short unit"><mat-label>Unit</mat-label><input matInput maxlength="8" [attr.aria-label]="label('Unit', p)" [value]="field(p, 'unit')" (change)="commit(p, 'unit', $event)" /></mat-form-field>
              </span>
              <!-- Where the tray draws one layout the note above says so once; there is no choice to show per row. -->
              @if (!support().fixed) {
                <pd-select class="format" [panelWidth]="null" [options]="formatOptions()" [value]="format(p)" [ariaLabel]="label('Format', p)" (valueChange)="setFormat(p, $event)" />
              }
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
    .unit { width: 4.5rem; }
    /* A number row also holds its label and unit: the metric picker gives way so the description keeps its width. */
    .narrow { width: 11rem; flex: none; }
    .faint { color: var(--text-3); }
    /* Wide enough for "Label over value" in the trigger and the list, on one line. */
    /* Room for "Label over value" and at least 8 px before the caret. */
    .format { width: 11.5rem; flex: none; }
    /* Label and Unit stay together when the chooser wraps the row's controls. */
    .pair { display: flex; gap: var(--pd-space-1); flex: none; }
    .sub { color: var(--text-2); font-size: var(--fs-sm); margin-block: 0 var(--sp-3); }
    .note { color: var(--text-2); font-size: var(--fs-sm); margin-block-start: var(--sp-3); }
    /* The family's info note (as the chooser's limit note): a tinted strip with the info glyph, clear of the list below it. */
    .info { display: flex; align-items: center; gap: var(--pd-space-2); margin: 0 0 var(--pd-space-3); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-info-tint); font-size: var(--pd-text-label); line-height: var(--pd-lh-label); }
    .info pd-status-glyph { color: var(--pd-info); flex: none; }
    .info.warn { background: var(--pd-warning-tint); }
    .info.warn pd-status-glyph { color: var(--pd-warning); }
  `,
})
export class MenuBarItems implements OnInit {
  protected readonly choices = inject(WidgetChoicesStore);
  protected readonly tray = inject(TrayAvailabilityStore);
  private readonly layouts = inject(WidgetLayouts);
  private readonly notifier = inject(Notifier);
  protected readonly view = this.layouts.menuBar;
  /** The type witness of the option templates (`pdWidgetOptionOf`). */
  protected readonly sample: MenuBarItem | null = null;
  /** The formats this platform's tray draws (the engine says which: `choices.number`). */
  protected readonly formatOptions = computed<PdSelectOption[]>(() => {
    const drawn = this.choices.choices()?.number.formats ?? [];
    return FORMAT_OPTIONS.filter((o) => drawn.includes(o.value as NumberFormat));
  });
  /** What the tray draws of a number here, and why anything is missing (from the engine). */
  protected readonly support = computed(() => this.choices.choices()?.number ?? { formats: [], labelInIcon: true, fixed: null, note: null });
  /** Said once there are more than four items: what each system does with items that do not fit. */
  protected readonly overflowNote =
    'More than four items may not all fit. On a Mac the menu bar hides the items that do not fit, without a sign, when the active app\'s menus need the room or a notch is in the way; the stacked number is the narrowest format. On Windows 11 icons you have not pinned to the taskbar go to the overflow flyout (the chevron); each keeps its place there.';
  protected readonly widgetFor = (kind: MenuBarKind): MenuBarItem => this.choices.menuBarDefault(kind);
  protected readonly kinds = computed(() => this.view()?.catalogue.kinds.map((k) => k.kind) ?? []);
  protected readonly metricOptions = computed<PdSelectOption[]>(() => this.choices.menuBarMetrics().map((m) => ({ value: m.title, label: m.title })));

  ngOnInit(): void {
    void this.layouts.use('menuBar');
    // Read again each time Settings opens: what the tray draws depends on the taskbar's scaling now, not when the app started.
    void this.choices.reload();
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
