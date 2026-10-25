import {
  afterNextRender,
  ApplicationRef,
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
  EnvironmentInjector,
  inject,
  type OnDestroy,
  untracked,
  viewChild,
  ViewEncapsulation,
} from '@angular/core';
import { PdIcon, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { DOCK_NOTICE_ACTION } from './dock-notice';
import { AngularContentRenderer } from './angular-panel';
import { AutoHideItem, itemsOn } from './auto-hide';
import { bottomFlyoutInsets, Side } from './dock-geometry';
import { DockKeys } from './dock-keys';
import { DockMenu, type DockMenuItem } from './dock-menu';
import { placeMenu } from './menu-placement';
import { DOCK_STYLES } from './dock-styles';
import { DockViews } from './dock-views';
import { WorkbenchDock } from './dock.service';
import { DockSwitcher } from './switcher-overlay';

/** What a slid-out panel keeps open when it is pressed or focused: itself, its bar, menus, and overlays and dialogs it opened. */
const KEEP_OPEN = '.pd-dock-ah-flyout, .pd-dock-ah-bar, .pd-dock-menu, .cdk-overlay-container';

/** How long the pointer may be away from a slid-out (hover-opened) tool window before it hides again. */
export const HOVER_HIDE_MS = 300;

/**
 * `<pd-dock>`: hosts the dockview instance managed by `WorkbenchDock` and fills its container, with Visual Studio's
 * extras around it: the auto-hide side bars and their slide-out, the Ctrl+Tab navigator, the inline notice and the
 * popup menus. Its tabs and headers exist at once; a view's content loads inside its panel (skeleton, error with Retry).
 */
@Component({
  selector: 'pd-dock',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [DockSwitcher, PdIcon],
  host: { 'data-testid': 'pd-dock', '[attr.data-ready]': 'dock.ready() ? "" : null', '[attr.aria-busy]': 'dock.ready() ? null : "true"', '[attr.data-perspective]': 'dock.activePerspective()' },
  template: `
    @for (side of sides; track side) {
      @if (bars()[side].length) {
        <nav class="pd-dock-ah-bar pd-dock-ah-{{ side }}" [attr.aria-label]="barLabel(side)" [attr.data-testid]="'pd-dock-ah-bar-' + side">
          @for (item of bars()[side]; track item.id) {
            <button
              type="button"
              class="pd-dock-ah-tab"
              [class.open]="open()?.id === item.id"
              [attr.aria-expanded]="open()?.id === item.id"
              aria-controls="pd-dock-ah-flyout"
              [attr.data-ah-id]="item.id"
              [attr.data-panel-id]="item.id"
              (mouseenter)="hover(item.id)"
              (mouseleave)="leaveSoon()"
              (click)="click(item.id, $event)"
              (keydown.escape)="dock.autoHideEvent({ type: 'escape' })"
            >
              {{ item.title }}
            </button>
          }
        </nav>
      }
    }
    <div class="pd-dock-center">
      <div #mount class="pd-dock-mount" data-testid="pd-dock-mount"></div>
      @if (!dock.ready()) {
        <!-- Until the layout is restored (or built) the main area shows the shape of a dock, never an empty page. -->
        <div class="pd-dock-boot" data-testid="pd-dock-skeleton" aria-hidden="true">
          <div class="pd-dock-boot-strip"><span></span><span></span></div>
          <div class="pd-dock-skeleton"><span></span><span></span><span></span><span></span></div>
        </div>
      }
      @if (open(); as item) {
        <section
          id="pd-dock-ah-flyout"
          class="pd-dock-ah-flyout pd-dock-ah-flyout-{{ item.side }}"
          role="region"
          tabindex="-1"
          data-testid="pd-dock-flyout"
          [attr.data-panel-id]="item.id"
          [attr.aria-label]="item.title"
          [style.width.px]="item.side === 'bottom' ? null : item.size"
          [style.--pd-ah-size]="item.size + 'px'"
          (mouseenter)="cancelLeave()"
          (mouseleave)="leaveSoon()"
          (keydown.escape)="escape()"
          (focusout)="focusOut($event)"
        >
          <!-- The strip tab already names the panel and the pane has the room: the actions float over its corner, no header row. -->
          <header class="pd-dock-ah-head">
            <button type="button" class="pd-dock-action pd-dock-ah-pin" [attr.aria-label]="pinLabel(item.title)" aria-pressed="true" [attr.title]="pinTitle" (click)="dock.pin(item.id)">
              <pd-icon name="pd:pin" />
            </button>
            <button type="button" class="pd-dock-action" [attr.aria-label]="hideLabel(item.title)" title="Hide (Esc)" i18n-title="@@pd.wb.dock.host.hideTitle" (click)="escape()">
              <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" /></svg>
            </button>
          </header>
          <div #flyoutBody class="pd-dock-ah-body"></div>
          <div class="pd-dock-ah-grip" aria-hidden="true" (pointerdown)="startResize($event, item)"></div>
        </section>
      }
    </div>
    @if (dock.switcher()) {
      <pd-dock-switcher />
    }
    @if (menus.state(); as menu) {
      <div class="pd-dock-menu-backdrop" (pointerdown)="menus.close()"></div>
      <ul class="pd-dock-menu pd-popmenu" role="menu" data-testid="pd-dock-menu" [style.left.px]="menu.x" [style.top.px]="menu.y" (keydown)="menuKey($event, menu.items)">
        @for (item of menu.items; track item.label) {
          <li role="none">
            <button type="button" role="menuitem" class="pd-popmenu__item" [disabled]="item.disabled ?? false" (click)="menus.choose(item)"><span class="pd-popmenu__label">{{ item.label }}</span></button>
          </li>
        }
      </ul>
    }
  `,
  styles: DOCK_STYLES,
})
export class PdDock implements OnDestroy {
  protected readonly dock = inject(WorkbenchDock);
  protected readonly menus = inject(DockMenu);
  private readonly views = inject(DockViews);
  private readonly appRef = inject(ApplicationRef);
  private readonly injector = inject(EnvironmentInjector);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly keys = inject(DockKeys);
  private readonly mount = viewChild.required<ElementRef<HTMLElement>>('mount');
  private readonly flyoutBody = viewChild<ElementRef<HTMLElement>>('flyoutBody');
  protected readonly sides: readonly Side[] = ['left', 'right', 'bottom'];

  protected barLabel(side: Side): string {
    switch (side) {
      case 'left':
        return $localize`:@@pd.wb.dock.host.barLeft:Auto-hidden tool windows (left)`;
      case 'right':
        return $localize`:@@pd.wb.dock.host.barRight:Auto-hidden tool windows (right)`;
      default:
        return $localize`:@@pd.wb.dock.host.barBottom:Auto-hidden tool windows (bottom)`;
    }
  }

  protected readonly pinTitle = $localize`:@@pd.wb.dock.host.pinTitle:Pin (dock it again)`;

  protected pinLabel(title: string): string {
    return $localize`:@@pd.wb.dock.host.pinLabel:Pin ${title}:title:`;
  }

  protected hideLabel(title: string): string {
    return $localize`:@@pd.wb.dock.host.hideLabel:Hide ${title}:title:`;
  }
  private leaveTimer: ReturnType<typeof setTimeout> | null = null;
  /** Slid-out views keep their component while they stay auto-hidden. */
  private readonly renderers = new Map<string, AngularContentRenderer>();
  private readonly offKeys: () => void;

  protected readonly bars = computed<Record<Side, AutoHideItem[]>>(() => {
    const s = this.dock.autoHide();
    return { left: itemsOn(s, 'left'), right: itemsOn(s, 'right'), bottom: itemsOn(s, 'bottom') };
  });
  protected readonly open = computed(() => {
    const s = this.dock.autoHide();
    return s.items.find((i) => i.id === s.open) ?? null;
  });

  constructor() {
    afterNextRender(() => this.dock.attach(this.mount().nativeElement));
    // The dock's notices go to the shell's notification surface (toast, then the bell), never over a group header
    // (review P1-3; Debrief ADR 0056 "layout.fit", ADR 0061): posting is a side effect of a new notice.
    const shell = inject(SHELL_PORTS);
    effect(() => {
      const notice = this.dock.notice();
      if (!notice) return;
      untracked(() =>
        shell.notify({
          level: notice.key === 'popout' ? 'warning' : 'info',
          message: notice.message,
          ...(notice.short ? { short: notice.short } : {}),
          kind: `pd.wb.dock.${notice.key.replace(/[^a-z0-9-]/g, '-')}`,
          ...(notice.action ? { action: { type: DOCK_NOTICE_ACTION, key: notice.key, label: notice.action.label } } : {}),
        }),
      );
    });
    this.offKeys = this.keys.listen(document);
    // effect-state: places a menu that just opened inside the window and moves the keyboard focus into it (DOM side effects)
    effect(() => {
      const menu = this.menus.state();
      if (!menu) return;
      afterNextRender(
        () => {
          const el = this.host.nativeElement.querySelector<HTMLElement>('.pd-dock-menu');
          if (!el) return;
          placeMenu(el, menu.x, menu.y);
          el.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
        },
        { injector: this.injector },
      );
    });
    // effect-state: places a bottom slide-out over the column it belongs to, not over a full-height side column such as the
    // Assistant's (review 3 P3-3): measures the docked groups and sets the slide-out's insets (DOM side effect). Dockview sizes
    // its groups after the change that opened the slide-out (and again when a column opens or a splitter moves), so the
    // groups are watched while it is open.
    effect((onCleanup) => {
      const item = this.open();
      this.dock.layoutChanged();
      if (item?.side !== 'bottom') return;
      let frame = 0;
      const place = (): void => {
        const center = this.host.nativeElement.querySelector<HTMLElement>('.pd-dock-center');
        const flyout = this.host.nativeElement.querySelector<HTMLElement>('.pd-dock-ah-flyout-bottom');
        if (!center || !flyout) return;
        // The groups' edges are the dockview mount's (the centre also holds the auto-hide bar's room); the insets are the
        // same for both, which share their left and right edges.
        const box = (center.querySelector<HTMLElement>('.pd-dock-mount') ?? center).getBoundingClientRect();
        const groups = [...center.querySelectorAll<HTMLElement>('.dv-groupview')]
          .filter((g) => !g.closest('.dv-resize-container'))
          .map((g) => {
            const r = g.getBoundingClientRect();
            return { x: r.left, y: r.top, width: r.width, height: r.height };
          });
        const insets = bottomFlyoutInsets(groups, { x: box.left, y: box.top, width: box.width, height: box.height });
        flyout.style.left = `${insets?.left ?? 0}px`;
        flyout.style.right = `${insets?.right ?? 0}px`;
      };
      const soon = (): void => {
        cancelAnimationFrame(frame);
        frame = requestAnimationFrame(place);
      };
      const center = this.host.nativeElement.querySelector<HTMLElement>('.pd-dock-center');
      const watch = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(soon);
      if (center && watch) {
        watch.observe(center);
        center.querySelectorAll<HTMLElement>('.dv-groupview').forEach((g) => watch.observe(g));
      }
      soon();
      onCleanup(() => {
        cancelAnimationFrame(frame);
        watch?.disconnect();
      });
    });
    // Mount the slid-out view's component into the flyout body.
    // effect-state: mounts and disposes view renderers in the DOM flyout (side effect, not derived state)
    effect(() => {
      const item = this.open();
      const body = this.flyoutBody()?.nativeElement;
      const ids = new Set(this.dock.autoHide().items.map((i) => i.id));
      untracked(() => {
        for (const [id, r] of this.renderers) {
          if (!ids.has(id)) {
            r.dispose();
            this.renderers.delete(id);
          } else r.setVisible(id === item?.id && !!body);
        }
        if (!item || !body) return;
        let renderer = this.renderers.get(item.id);
        if (!renderer) {
          const view = this.views.find(item.id);
          if (!view) return;
          renderer = new AngularContentRenderer(view, this.appRef, this.injector);
          renderer.init({} as never);
          this.renderers.set(item.id, renderer);
        }
        body.replaceChildren(renderer.element);
      });
    });
    const onPointerDown = (e: PointerEvent) => {
      const t = e.target as Element | null;
      if (this.open() && !t?.closest?.(KEEP_OPEN)) this.dock.autoHideEvent({ type: 'blur' });
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    this.cleanup.push(() => document.removeEventListener('pointerdown', onPointerDown, true));
    // Escape anywhere hides a slide-out (one opened by selecting an event leaves the focus where it was), unless
    // something else (a menu, a dialog, an editor) used the key.
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.defaultPrevented && this.open()) this.dock.autoHideEvent({ type: 'escape' });
    };
    document.addEventListener('keydown', onKeyDown);
    this.cleanup.push(() => document.removeEventListener('keydown', onKeyDown));
  }

  private readonly cleanup: (() => void)[] = [];

  protected hover(id: string): void {
    this.cancelLeave();
    this.dock.autoHideEvent({ type: 'hover', id });
  }

  protected click(id: string, e: MouseEvent): void {
    this.cancelLeave();
    // A click from the keyboard (Enter/Space on the focused tab) has no pointer position.
    const keyboard = e.detail === 0;
    this.dock.autoHideEvent(keyboard ? { type: 'activate', id } : { type: 'click', id });
    if (keyboard) afterNextRender(() => this.host.nativeElement.querySelector<HTMLElement>('.pd-dock-ah-flyout')?.focus(), { injector: this.injector });
  }

  protected leaveSoon(): void {
    this.cancelLeave();
    this.leaveTimer = setTimeout(() => this.dock.autoHideEvent({ type: 'leave' }), HOVER_HIDE_MS);
  }

  protected cancelLeave(): void {
    if (this.leaveTimer !== null) clearTimeout(this.leaveTimer);
    this.leaveTimer = null;
  }

  protected escape(): void {
    const id = this.open()?.id;
    this.dock.autoHideEvent({ type: 'escape' });
    if (id) afterNextRender(() => this.host.nativeElement.querySelector<HTMLElement>(`.pd-dock-ah-tab[data-ah-id="${CSS.escape(id)}"]`)?.focus(), { injector: this.injector });
  }

  protected focusOut(e: FocusEvent): void {
    const next = e.relatedTarget as Element | null;
    if (next && !next.closest(KEEP_OPEN)) this.dock.autoHideEvent({ type: 'blur' });
  }


  /** Arrow keys, Home and End move through the open menu; Escape closes it and gives the focus back. */
  protected menuKey(e: KeyboardEvent, items: readonly DockMenuItem[]): void {
    const buttons = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const to = (i: number) => {
      e.preventDefault();
      buttons[(i + buttons.length) % Math.max(1, buttons.length)]?.focus();
    };
    if (e.key === 'Escape') {
      e.preventDefault();
      this.menus.close(true);
    } else if (e.key === 'ArrowDown') to(at + 1);
    else if (e.key === 'ArrowUp') to(at < 0 ? -1 : at - 1);
    else if (e.key === 'Home') to(0);
    else if (e.key === 'End') to(-1);
    else if (e.key === 'Tab') this.menus.close(true);
    void items;
  }

  protected startResize(e: PointerEvent, item: AutoHideItem): void {
    e.preventDefault();
    const start = { x: e.clientX, y: e.clientY, size: item.size };
    const move = (m: PointerEvent) => {
      const delta = item.side === 'left' ? m.clientX - start.x : item.side === 'right' ? start.x - m.clientX : start.y - m.clientY;
      this.dock.autoHideEvent({ type: 'resize', id: item.id, size: start.size + delta });
    };
    const up = () => {
      document.removeEventListener('pointermove', move);
      document.removeEventListener('pointerup', up);
    };
    document.addEventListener('pointermove', move);
    document.addEventListener('pointerup', up);
  }

  ngOnDestroy(): void {
    this.cancelLeave();
    this.offKeys();
    this.cleanup.forEach((c) => c());
    for (const r of this.renderers.values()) r.dispose();
    this.renderers.clear();
    this.dock.detach();
  }
}
