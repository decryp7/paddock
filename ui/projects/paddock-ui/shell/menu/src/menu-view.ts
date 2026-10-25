import { CdkContextMenuTrigger, CdkMenu, CdkMenuBar, CdkMenuItem, CdkMenuItemCheckbox, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, computed, DestroyRef, Directive, ElementRef, inject, input } from '@angular/core';
import { AppMenu } from './app-menu';
import { ariaKeyShortcuts, formatKey } from './keys';
import { KEY_PLATFORM } from './keymap';
import { itemsAt, type MenuNode } from './menu-model';

/**
 * Keeps a menu open when its trigger is clicked right after hovering opened it. CDK opens menus on hover (submenus, and
 * the next menu of the bar while one is open) and toggles them on click, so "hover, then click" closed the menu again.
 * A menu opened by a click still closes on the next click. Capture listeners on the target run before CDK's own.
 */
@Directive({ selector: '[pdMenuTrigger]' })
export class PdMenuTriggerGuard {
  constructor() {
    const trigger = inject(CdkMenuTrigger);
    const el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    let clicking = false;
    let openedByHover = false;
    const onClick = (e: MouseEvent) => {
      if (openedByHover && trigger.isOpen()) {
        openedByHover = false;
        e.stopImmediatePropagation();
        return;
      }
      clicking = true;
      queueMicrotask(() => (clicking = false));
    };
    const opened = trigger.opened.subscribe(() => (openedByHover = !clicking));
    const closed = trigger.closed.subscribe(() => (openedByHover = false));
    el.addEventListener('click', onClick, true);
    inject(DestroyRef).onDestroy(() => {
      el.removeEventListener('click', onClick, true);
      opened.unsubscribe();
      closed.unsubscribe();
    });
  }
}

/*
 * One menu list template for the bar and context menus: menus are looked up by path on every render (CDK keeps the data
 * a menu was first opened with), so an open menu follows check marks and enabled states as they change.
 */
const LIST = `
  <ng-template #list let-path let-label="label">
    <div cdkMenu class="pd-menu" [attr.aria-label]="label" [attr.data-menu]="label">
      @for (node of nodesAt(path); track $index) {
        @switch (node.kind) {
          @case ('separator') {
            <div role="separator" class="pd-menu-sep"></div>
          }
          @case ('submenu') {
            <button type="button" cdkMenuItem pdMenuTrigger class="pd-menu-item" [cdkMenuitemTypeaheadLabel]="node.label"
              [cdkMenuTriggerFor]="list" [cdkMenuTriggerData]="{ $implicit: path.concat(node.id), label: node.label }">
              <span class="pd-menu-check" aria-hidden="true"></span><span class="pd-menu-label">{{ node.label }}</span><span class="pd-menu-chevron" aria-hidden="true">›</span>
            </button>
          }
          @default {
            @if (node.checked === undefined) {
              <button type="button" cdkMenuItem class="pd-menu-item" [cdkMenuItemDisabled]="!node.enabled" [attr.data-command]="node.id"
                [cdkMenuitemTypeaheadLabel]="node.label" [attr.aria-keyshortcuts]="keys(node.keybinding)" (cdkMenuItemTriggered)="run(node.id)">
                <span class="pd-menu-check" aria-hidden="true"></span><span class="pd-menu-label">{{ node.label }}</span><span class="pd-menu-hint" aria-hidden="true">{{ hint(node.keybinding) }}</span>
              </button>
            } @else {
              <button type="button" cdkMenuItemCheckbox class="pd-menu-item" [cdkMenuItemDisabled]="!node.enabled" [cdkMenuItemChecked]="node.checked" [attr.data-command]="node.id"
                [cdkMenuitemTypeaheadLabel]="node.label" [attr.aria-keyshortcuts]="keys(node.keybinding)" (cdkMenuItemTriggered)="run(node.id)">
                <span class="pd-menu-check" aria-hidden="true">{{ node.checked ? '✓' : '' }}</span><span class="pd-menu-label">{{ node.label }}</span><span class="pd-menu-hint" aria-hidden="true">{{ hint(node.keybinding) }}</span>
              </button>
            }
          }
        }
      }
    </div>
  </ng-template>
`;

const MENU_STYLES = `
  .pd-menu {
    min-width: 220px; max-width: min(420px, calc(100vw - 16px)); max-height: calc(100vh - 48px); overflow-y: auto; overscroll-behavior: contain;
    display: flex; flex-direction: column; padding: var(--pd-space-1); border-radius: var(--pd-radius-popover);
    background: var(--pd-surface-3); border: 1px solid var(--pd-hairline); box-shadow: var(--pd-shadow-2); color: var(--pd-text);
    font: inherit; font-size: var(--pd-text-body);
  }
  .pd-menu:focus { outline: none; }
  .pd-menu-item {
    display: flex; align-items: center; gap: var(--pd-space-2); flex: none; min-height: 28px; padding: 0 var(--pd-space-3) 0 var(--pd-space-2);
    border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); cursor: pointer; text-align: start; font: inherit; white-space: nowrap;
  }
  .pd-menu-item:hover:not([aria-disabled='true']), .pd-menu-item:focus, .pd-menu-item[aria-expanded='true'] { background: var(--pd-hover); outline: none; }
  .pd-menu-item:focus-visible { box-shadow: inset 0 0 0 2px var(--pd-focus-ring); }
  .pd-menu-item[aria-disabled='true'] { color: var(--pd-text-subtle); cursor: default; }
  .pd-menu-check { width: 14px; flex: none; text-align: center; color: var(--pd-accent); }
  .pd-menu-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .pd-menu-hint { flex: none; margin-inline-start: var(--pd-space-6); color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
  .pd-menu-item[aria-disabled='true'] .pd-menu-hint { color: var(--pd-text-subtle); }
  .pd-menu-chevron { flex: none; margin-inline-start: var(--pd-space-6); color: var(--pd-text-muted); }
  .pd-menu-sep { flex: none; height: 1px; margin: var(--pd-space-1) var(--pd-space-2); background: var(--pd-hairline); }
`;

/** What the bar and the context menu share: rendering helpers over the one menu model. */
@Directive()
abstract class MenuView {
  protected readonly app = inject(AppMenu);
  private readonly platform = inject(KEY_PLATFORM);

  /** The entries at `path` (the root menu id, then submenu ids). */
  protected abstract nodesAt(path: readonly string[]): readonly MenuNode[];

  protected hint(binding: string | undefined): string {
    return binding ? formatKey(binding, this.platform) : '';
  }

  protected keys(binding: string | undefined): string | null {
    return binding ? ariaKeyShortcuts(binding, this.platform) : null;
  }

  protected run(id: string): void {
    void this.app.run(id);
  }
}

/**
 * The in-app menu bar (Windows, Linux and the browser): the window's menus as an ARIA menubar with real submenus, built
 * on `@angular/cdk/menu` (keyboard navigation, typeahead, focus handling, placement that flips near the window edges).
 * In the macOS desktop app the menus are in the native menu bar and this renders nothing.
 */
@Component({
  selector: 'pd-menu-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CdkMenuBar, CdkMenu, CdkMenuItem, CdkMenuItemCheckbox, CdkMenuTrigger, PdMenuTriggerGuard],
  template:
    `
    @if (app.mode === 'in-app') {
      <div cdkMenuBar class="pd-bar" [attr.aria-label]="t.bar">
        @for (menu of app.menus(); track menu.id) {
          <button type="button" cdkMenuItem pdMenuTrigger class="pd-top" [cdkMenuTriggerFor]="list" [cdkMenuTriggerData]="{ $implicit: [menu.id], label: menu.label }">{{ menu.label }}</button>
        }
      </div>
    }
  ` + LIST,
  styles:
    `
    :host { display: block; min-width: 0; }
    /* One line always: a bar that wraps is clipped by the 40 px title bar; the title bar scrolls it (and truncates the app name first). */
    .pd-bar { display: flex; flex-wrap: nowrap; gap: 2px; }
    .pd-top { flex: none; white-space: nowrap; min-height: 28px; padding: 0 var(--pd-space-2); border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; font: inherit; }
    .pd-top:hover, .pd-top:focus-visible, .pd-top[aria-expanded='true'] { background: var(--pd-hover); color: var(--pd-text); outline: none; }
    .pd-top:focus-visible { box-shadow: inset 0 0 0 2px var(--pd-focus-ring); }
  ` + MENU_STYLES,
})
export class PdMenuBar extends MenuView {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    bar: $localize`:@@pd.menu.bar:Application menu`,
  };
  protected nodesAt(path: readonly string[]): readonly MenuNode[] {
    return itemsAt(this.app.menus(), path);
  }
}

/**
 * A context menu over its content: right-click (or the context menu key, Shift+F10) opens the items contributed to
 * menu `menu` (`defineMenuItem({ menu: 'pw.models.row', ... })`). Nothing opens while the menu is empty.
 */
@Component({
  selector: 'pd-context-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CdkContextMenuTrigger, CdkMenu, CdkMenuItem, CdkMenuItemCheckbox, CdkMenuTrigger, PdMenuTriggerGuard],
  template:
    `
    <div class="pd-area" [cdkContextMenuTriggerFor]="list" [cdkContextMenuDisabled]="empty()" [cdkContextMenuTriggerData]="{ $implicit: [menu()], label: label() }">
      <ng-content />
    </div>
  ` + LIST,
  styles: `:host { display: block; } .pd-area { display: contents; }` + MENU_STYLES,
})
export class PdContextMenu extends MenuView {
  /** The menu id whose items this context menu shows. */
  readonly menu = input.required<string>();
  /** The menu's accessible name. */
  readonly label = input($localize`:@@pd.menu.context:Context menu`);
  protected readonly empty = computed(() => this.app.items(this.menu()).length === 0);

  protected nodesAt(path: readonly string[]): readonly MenuNode[] {
    const [root, ...rest] = path;
    return root === undefined ? [] : itemsAt([{ id: root, label: '', items: this.app.items(root) }], [root, ...rest]);
  }
}
