import { CdkMenu, CdkMenuItem, CdkMenuItemCheckbox, CdkMenuTrigger } from '@angular/cdk/menu';
import type { ConnectedPosition } from '@angular/cdk/overlay';
import { NgTemplateOutlet } from '@angular/common';
import { afterNextRender, ChangeDetectionStrategy, Component, computed, DestroyRef, ElementRef, inject, signal } from '@angular/core';
import { PdIcon } from '@decryp7/paddock-ui/shell';
import { ariaKeyShortcuts, Keymap } from '@decryp7/paddock-ui/shell/menu';
import type { ActivityItemDef, ActivityMenuEntry } from '@decryp7/paddock-ui/workbench';
import { ActivityItems, fittingTopItems } from './activity';

/** Fly-outs open to the right of their button, top-aligned, or bottom-aligned near the window's bottom edge. */
export const FLYOUT_POSITIONS: ConnectedPosition[] = [
  { originX: 'end', originY: 'top', overlayX: 'start', overlayY: 'top', offsetX: 6 },
  { originX: 'end', originY: 'bottom', overlayX: 'start', overlayY: 'bottom', offsetX: 6 },
];

/**
 * The activity bar (plan W4; Debrief's `shell/activity-bar.ts`): one button per contributed activity item, in order, the
 * `bottom` ones pinned at the foot; items with a menu open a CDK fly-out to the right; when the window is too short the
 * last top items collapse into More. Badges are part of the accessible name ("Problems, 3").
 */
@Component({
  selector: 'pd-activity-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdIcon, NgTemplateOutlet, CdkMenu, CdkMenuItem, CdkMenuItemCheckbox, CdkMenuTrigger],
  template: `
    @for (item of shown(); track item.id) {
      <ng-container *ngTemplateOutlet="button; context: { $implicit: item }" />
    }
    @if (overflow().length) {
      <button
        type="button"
        class="pd-act"
        data-activity-id="pd.more"
        [attr.aria-label]="t.more"
        [title]="t.more"
        [cdkMenuTriggerFor]="more"
        [cdkMenuPosition]="positions"
        #moreTrigger="cdkMenuTriggerFor"
        [class.pd-on]="moreTrigger.isOpen()"
        (keydown.arrowright)="openWithKeyboard(moreTrigger, $event)"
      >
        <pd-icon name="pd:more" />
      </button>
    }
    <div class="pd-act-spacer"></div>
    @for (item of items.bottom(); track item.id) {
      <ng-container *ngTemplateOutlet="button; context: { $implicit: item }" />
    }

    <ng-template #button let-item>
      @if (items.hasMenu(item)) {
        <button
          type="button"
          class="pd-act"
          [attr.data-activity-id]="item.id"
          [attr.aria-label]="ariaLabel(item)"
          [title]="tooltip(item)"
          [attr.aria-keyshortcuts]="keys(item)"
          [cdkMenuTriggerFor]="flyout"
          [cdkMenuTriggerData]="{ $implicit: item }"
          [cdkMenuPosition]="positions"
          #trigger="cdkMenuTriggerFor"
          [class.pd-on]="trigger.isOpen() || items.isActive(item)"
          (keydown.arrowright)="openWithKeyboard(trigger, $event)"
        >
          <ng-container *ngTemplateOutlet="face; context: { $implicit: item }" />
        </button>
      } @else {
        <button
          type="button"
          class="pd-act"
          [attr.data-activity-id]="item.id"
          [attr.aria-label]="ariaLabel(item)"
          [title]="tooltip(item)"
          [attr.aria-keyshortcuts]="keys(item)"
          [attr.aria-pressed]="item.toggle ? items.isActive(item) : null"
          [attr.aria-disabled]="items.disabledReason(item) ? 'true' : null"
          [class.pd-on]="items.isActive(item)"
          (click)="items.activate(item)"
        >
          <ng-container *ngTemplateOutlet="face; context: { $implicit: item }" />
        </button>
      }
    </ng-template>

    <ng-template #face let-item>
      <pd-icon class="pd-act-icon" [name]="item.icon" />
      @if (items.isOpen(item) && !items.isActive(item)) {
        <span class="pd-act-open" aria-hidden="true" data-testid="activity-open"></span>
      }
      @if (items.pulse(item)) {
        <span class="pd-act-pulse" aria-hidden="true"></span>
      }
      @if (items.badge(item); as badge) {
        <span class="pd-act-badge" aria-hidden="true">{{ badge }}</span>
      }
    </ng-template>

    <!-- Entries are built on every opening: CDK keeps the data a menu was first opened with. -->
    <ng-template #flyout let-item>
      <div cdkMenu class="pd-popmenu pd-act-menu" [attr.aria-label]="item.title.text" [attr.data-activity-menu]="item.id">
        @for (entry of items.menu(item); track entry.id) {
          @if (entry.checked === undefined) {
            <button type="button" cdkMenuItem class="pd-popmenu__item" [cdkMenuitemTypeaheadLabel]="entry.label" [attr.aria-keyshortcuts]="keys(entry)" (cdkMenuItemTriggered)="items.run(entry.run)">
              <ng-container *ngTemplateOutlet="entryFace; context: { $implicit: entry, checked: false }" />
            </button>
          } @else {
            <button type="button" cdkMenuItemCheckbox class="pd-popmenu__item" [cdkMenuItemChecked]="entry.checked" [cdkMenuitemTypeaheadLabel]="entry.label" [attr.aria-keyshortcuts]="keys(entry)" (cdkMenuItemTriggered)="items.run(entry.run)">
              <ng-container *ngTemplateOutlet="entryFace; context: { $implicit: entry, checked: entry.checked }" />
            </button>
          }
        } @empty {
          <div class="pd-popmenu__item" aria-disabled="true">{{ t.empty }}</div>
        }
      </div>
    </ng-template>

    <ng-template #more>
      <div cdkMenu class="pd-popmenu pd-act-menu" [attr.aria-label]="t.more" data-activity-menu="pd.more">
        @for (item of overflow(); track item.id) {
          @if (items.hasMenu(item)) {
            <button type="button" cdkMenuItem class="pd-popmenu__item" [cdkMenuitemTypeaheadLabel]="item.title.text" [cdkMenuTriggerFor]="flyout" [cdkMenuTriggerData]="{ $implicit: item }">
              <ng-container *ngTemplateOutlet="entryFace; context: { $implicit: asEntry(item), checked: false, submenu: true }" />
            </button>
          } @else {
            <button
              type="button"
              cdkMenuItemCheckbox
              class="pd-popmenu__item"
              [cdkMenuItemChecked]="items.isActive(item)"
              [cdkMenuItemDisabled]="!!items.disabledReason(item)"
              [cdkMenuitemTypeaheadLabel]="item.title.text"
              [title]="tooltip(item)"
              (cdkMenuItemTriggered)="items.activate(item)"
            >
              <ng-container *ngTemplateOutlet="entryFace; context: { $implicit: asEntry(item), checked: items.isActive(item) }" />
            </button>
          }
        }
      </div>
    </ng-template>

    <ng-template #entryFace let-entry let-checked="checked" let-submenu="submenu">
      <span class="pd-popmenu__icon pd-act-check" aria-hidden="true">
        @if (checked) {
          <pd-icon name="pd:check" />
        }
      </span>
      <span class="pd-popmenu__icon" aria-hidden="true">
        @if (entry.icon) {
          <pd-icon [name]="entry.icon" />
        }
      </span>
      <span class="pd-popmenu__label">{{ entry.label }}</span>
      @if (submenu) {
        <pd-icon class="pd-popmenu__hint" name="pd:chevron-right" />
      } @else if (items.keyHint(entry); as h) {
        <span class="pd-popmenu__hint" aria-hidden="true">{{ h }}</span>
      }
    </ng-template>
  `,
  styles: `
    :host { width: 48px; flex: none; display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 8px 0; min-height: 0; background: var(--pd-surface-1); border-inline-end: 1px solid var(--pd-hairline); overflow: hidden; }
    .pd-act { position: relative; width: 36px; height: 36px; flex: none; border-radius: var(--pd-radius-card); border: 0; background: transparent; color: var(--pd-text-muted); display: flex; align-items: center; justify-content: center; cursor: pointer; }
    .pd-act-icon { transform: scale(1.125); }
    .pd-act:hover { color: var(--pd-text); background: var(--pd-hover); }
    .pd-act:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; color: var(--pd-text); }
    .pd-act.pd-on { background: var(--pd-accent-tint); color: var(--pd-text); }
    /* The selected item's indicator: a 2x20 px bar on the bar's inner edge, outside the rounded tile (review P2-5). */
    .pd-act.pd-on::before { content: ''; position: absolute; inset-inline-start: -6px; top: 8px; width: 2px; height: 20px; border-radius: 1px; background: var(--pd-accent-fill); }
    .pd-act-open { position: absolute; bottom: 3px; left: 50%; width: 4px; height: 4px; margin-left: -2px; border-radius: 50%; background: var(--pd-text-muted); }
    .pd-act[aria-disabled='true'] { opacity: 0.45; cursor: default; }
    .pd-act-spacer { flex: 1; min-height: 0; }
    .pd-act-pulse { position: absolute; top: 6px; inset-inline-end: 6px; width: 7px; height: 7px; border-radius: 50%; background: var(--pd-accent); animation: pd-act-pulse 1.6s ease-in-out infinite; }
    @keyframes pd-act-pulse { 0%, 100% { opacity: 1; transform: scale(1); } 50% { opacity: 0.35; transform: scale(0.7); } }
    @media (prefers-reduced-motion: reduce) { .pd-act-pulse { animation: none; } }
    .pd-act-badge { position: absolute; top: 1px; inset-inline-end: 0; box-shadow: 0 0 0 1.5px var(--pd-surface-1); min-width: 14px; height: 14px; padding: 0 3px; box-sizing: border-box; border-radius: 7px; background: var(--pd-accent-fill); color: var(--pd-on-accent); font-size: var(--pd-text-caption); font-weight: var(--pd-weight-semibold); line-height: 14px; text-align: center; }
    .pd-act-menu { min-width: 220px; max-width: min(360px, calc(100vw - 16px)); max-height: calc(100vh - 32px); overflow-y: auto; overscroll-behavior: contain; }
    .pd-act-menu:focus { outline: none; }
    .pd-act-check { color: var(--pd-accent); }
  `,
})
export class PdActivityBar {
  protected readonly items = inject(ActivityItems);
  private readonly keymap = inject(Keymap);
  protected readonly positions = FLYOUT_POSITIONS;
  protected readonly t = {
    more: $localize`:@@pd.wb.activity.more:More`,
    empty: $localize`:@@pd.wb.activity.empty:Nothing here`,
  };
  /** The bar's height in px (0 until measured). */
  protected readonly barHeight = signal(0);
  private readonly fitting = computed(() => fittingTopItems(this.items.top().length, this.items.bottom().length, this.barHeight()));
  protected readonly shown = computed(() => this.items.top().slice(0, this.fitting()));
  protected readonly overflow = computed(() => this.items.top().slice(this.fitting()));

  constructor() {
    const host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      this.barHeight.set(host.clientHeight);
      if (typeof ResizeObserver !== 'function') return;
      const observer = new ResizeObserver(() => this.barHeight.set(host.clientHeight));
      observer.observe(host);
      destroyRef.onDestroy(() => observer.disconnect());
    });
  }

  /** ArrowRight on a fly-out button opens it with the first entry focused (Enter, Space and ArrowDown do too). */
  protected openWithKeyboard(trigger: CdkMenuTrigger, event: Event): void {
    event.preventDefault();
    trigger.open();
    trigger.getMenu()?.focusFirstItem('keyboard');
  }

  /** An item as a menu entry (in More). */
  protected asEntry(item: ActivityItemDef): { label: string; icon: string } {
    return { label: item.title.text, icon: item.icon };
  }

  /**
   * The button's name with its badge ("Problems, 3"): the count shown is part of the name (WCAG 2.5.3); not in
   * parentheses, which name-in-label checks ignore. A badge label wins when the item gives one.
   */
  protected ariaLabel(item: ActivityItemDef): string {
    const badge = this.items.badge(item);
    if (badge === null) return item.title.text;
    const label = this.items.badgeLabel(item);
    return label ? `${item.title.text}, ${label}` : $localize`:@@pd.wb.activity.label-badge:${item.title.text}:label:, ${badge}:badge:`;
  }

  /** "Label (Ctrl+B)", with the reason when the item is inert. */
  protected tooltip(item: ActivityItemDef): string {
    const keys = this.items.keyHint(item);
    const label = keys ? $localize`:@@pd.wb.activity.tooltip-keys:${item.title.text}:label: (${keys}:keys:)` : item.title.text;
    const reason = this.items.disabledReason(item);
    return reason ? $localize`:@@pd.wb.activity.tooltip-reason:${label}:label: — ${reason}:reason:` : label;
  }

  protected keys(item: ActivityItemDef | ActivityMenuEntry): string | null {
    const binding = this.items.keybinding(item);
    return binding ? ariaKeyShortcuts(binding, this.keymap.platform) : null;
  }
}
