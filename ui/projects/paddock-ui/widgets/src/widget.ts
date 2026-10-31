import { Directive, inject, input, InjectionToken, TemplateRef, type Signal } from '@angular/core';
import type { Placement } from '../generated/widgets';

/** What a kind's template is given: the value (null while loading), whether it is loading, and whether it is a stale reading. */
export interface PdWidgetContext<V> {
  /** The ready value; `null` while the widget is loading (the template draws its skeleton twin). */
  readonly $implicit: V | null;
  /** True while there is no value yet. */
  readonly loading: boolean;
  /** True when the value is an older reading (the frame's head says so). */
  readonly stale: boolean;
}

/**
 * The app's template for one widget kind: `<ng-template pdWidget="cpuMemory" let-value>...</ng-template>` or
 * `*pdWidget="'cpuMemory'; let value"`. The same template draws the ready widget and its loading twin (the value is `null`
 * and its parts marked `pdSkeleton` turn into blocks), so the loading height is the ready height.
 *
 * `calm: true` marks a kind whose empty state means "all good" (no active alarms): the frame draws it as one button row
 * (success icon, text, chevron) that emits the empty state's `next` action.
 */
@Directive({ selector: 'ng-template[pdWidget]', exportAs: 'pdWidget' })
export class PdWidget<V = unknown> {
  /** The kind's wire name (`KindInfo.kind`). */
  readonly pdWidget = input.required<string>();
  /** The kind's empty state means "all good". */
  readonly pdWidgetCalm = input(false);
  /**
   * A type witness, never read: bind a typed value (`[pdWidgetOf]="cpuValue"`, a property declared `cpuValue: CpuValue | null = null`)
   * and the template's `let-v` is typed `CpuValue | null`, so no cast is needed.
   */
  readonly pdWidgetOf = input<V | null>(null);
  readonly template = inject<TemplateRef<PdWidgetContext<V>>>(TemplateRef);

  static ngTemplateContextGuard<V>(_directive: PdWidget<V>, _context: unknown): _context is PdWidgetContext<V> {
    return true;
  }
}

/** Set by the frame for the template it draws: true while the loading twin is drawn. */
export const PD_WIDGET_LOADING = new InjectionToken<Signal<boolean>>('PD_WIDGET_LOADING');

/**
 * Marks a part of a kind's template that has no value yet: while the widget is loading, the part is drawn as a
 * `--pd-skeleton-raised` block (its text hidden, its size kept) and is hidden from assistive technology. Outside a frame it does nothing.
 */
@Directive({
  selector: '[pdSkeleton]',
  host: { '[class.pd-sk]': 'loading()', '[attr.aria-hidden]': 'loading() ? "true" : null' },
})
export class PdSkeleton {
  protected readonly loading = inject(PD_WIDGET_LOADING, { optional: true }) ?? (() => false);
}

/** The context of an option template of the chooser. */
export interface PdWidgetOptionContext<W> {
  /** The placement the option belongs to. */
  readonly $implicit: Placement<W>;
}

/**
 * The app's option control for one kind in the chooser (a volume select, say): `*pdWidgetOption="'diskSpace'; let placement"`.
 * The app sends its own `replace` edit to the engine; the chooser only places the control in the row.
 */
@Directive({ selector: 'ng-template[pdWidgetOption]' })
export class PdWidgetOption<W = unknown> {
  readonly pdWidgetOption = input.required<string>();
  /** A type witness, never read: bind a `W | null` (`[pdWidgetOptionOf]="sample"`) and the template's `let-p` is a `Placement<W>`, with no cast. */
  readonly pdWidgetOptionOf = input<W | null>(null);
  readonly template = inject<TemplateRef<PdWidgetOptionContext<W>>>(TemplateRef);

  static ngTemplateContextGuard<W>(_directive: PdWidgetOption<W>, _context: unknown): _context is PdWidgetOptionContext<W> {
    return true;
  }
}

/**
 * The kind (wire name) of a widget value: the string itself, the `kind` field of a tagged object, or the only key of an
 * externally tagged one (serde's two common shapes). An app with another shape passes its own `kindOf`.
 */
export function defaultKindOf(widget: unknown): string {
  if (typeof widget === 'string') return widget;
  if (widget !== null && typeof widget === 'object') {
    const kind = (widget as { kind?: unknown }).kind;
    if (typeof kind === 'string') return kind;
    const keys = Object.keys(widget);
    if (keys.length === 1) return keys[0] as string;
  }
  return '';
}
