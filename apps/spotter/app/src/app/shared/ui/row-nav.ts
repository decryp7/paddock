import { FocusKeyManager, type FocusableOption } from '@angular/cdk/a11y';
import { CdkContextMenuTrigger } from '@angular/cdk/menu';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { InjectionToken, Directive, forwardRef, ElementRef, Injector, Signal, computed, contentChildren, inject, output, signal } from '@angular/core';

const ROW_LIST = new InjectionToken<RowListPort>('ROW_LIST');

/** What a row needs of its list (an interface keeps the two classes from depending on each other's types). */
interface RowListPort {
  readonly current: Signal<object | null>;
  focused(row: object): void;
}

/**
 * A row of a feed or table: focusable (one row at a time is in the tab order), Enter activates it, and the context-menu
 * key (Shift+F10 or Menu) opens the row's context menu where it has one. Put it on the `li` or `tr`; the keys are
 * handled only when the row itself has the focus, so buttons and links inside keep their own behaviour.
 */
@Directive({
  selector: '[spRowItem]',
  host: {
    class: 'row-item',
    '[attr.tabindex]': 'tabindex()',
    '(focus)': 'list?.focused(this)',
    '(keydown.enter)': 'activate($event)',
    '(keydown.shift.f10)': 'openMenu($event)',
    '(keydown.contextmenu)': 'openMenu($event)',
  },
})
export class RowItem implements FocusableOption {
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  // The list is optional so a row also works outside one (it is then always in the tab order).
  protected readonly list: RowListPort | null = inject<RowListPort>(ROW_LIST, { optional: true });
  /** Enter on the row. */
  readonly spRowActivate = output<void>();

  constructor() {
    // A closed context menu gives the focus back to its row: when it ended up nowhere or on a button of the row (CDK
    // returns it to the nearest menu trigger, which is the row's "..." button), not when the user went elsewhere.
    inject(CdkContextMenuTrigger, { optional: true })
      ?.closed.pipe(takeUntilDestroyed())
      .subscribe(() =>
        setTimeout(() => {
          const active = this.el.ownerDocument.activeElement;
          if (!active || active === this.el.ownerDocument.body || this.el.contains(active)) this.focus();
        }),
      );
  }

  /** The roving tab stop: only the current row of a list is reachable with Tab. */
  protected readonly tabindex = computed<number>(() => (this.list === null || this.list.current() === this ? 0 : -1));

  focus(): void {
    this.el.focus();
  }

  /** Whether `target` is this row and not something inside it. */
  owns(target: EventTarget | null): boolean {
    return target === this.el;
  }

  protected activate(e: Event): void {
    if (e.target !== this.el) return;
    e.preventDefault();
    this.spRowActivate.emit();
  }

  /** The browser's own context-menu event does not come from the keyboard everywhere, so the key asks for it. */
  protected openMenu(e: Event): void {
    if (e.target !== this.el) return;
    e.preventDefault();
    const r = this.el.getBoundingClientRect();
    this.el.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + Math.min(48, r.width / 2), clientY: r.top + r.height / 2 }),
    );
  }
}

/**
 * The container of rows (`ul`, `tbody`): Up/Down move between them, Home/End go to the first and last (CDK
 * `FocusKeyManager`). The focused row is visibly outlined (`.row-item:focus-visible` in the global styles).
 */
@Directive({
  selector: '[spRowList]',
  providers: [{ provide: ROW_LIST, useExisting: forwardRef(() => RowList) }],
  host: { '(keydown)': 'onKey($event)' },
})
export class RowList {
  private readonly rows = contentChildren<RowItem>(RowItem, { descendants: true });
  private readonly manager = new FocusKeyManager<RowItem>(this.rows, inject(Injector)).withVerticalOrientation().withHomeAndEnd();
  private readonly active = signal<RowItem | null>(null);

  /** The row in the tab order: the one last focused, else the first. */
  readonly current: Signal<object | null> = computed<object | null>(() => {
    const rows = this.rows();
    const a = this.active();
    return a && rows.includes(a) ? a : (rows[0] ?? null);
  });

  /** A row took the focus (Tab, a click, the keys). */
  focused(row: object): void {
    if (!(row instanceof RowItem)) return;
    this.active.set(row);
    this.manager.updateActiveItem(row);
  }

  protected onKey(e: KeyboardEvent): void {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const row = this.rows().find((r) => r.owns(e.target));
    if (!row) return;
    this.manager.updateActiveItem(row);
    this.manager.onKeydown(e);
  }
}
