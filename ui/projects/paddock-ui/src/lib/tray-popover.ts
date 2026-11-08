import { afterNextRender, ChangeDetectionStrategy, Component, computed, DestroyRef, ElementRef, inject, input, output, signal, viewChild } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatIcon } from '@angular/material/icon';
import { MatMenu, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { MatTooltip } from '@angular/material/tooltip';
import { PdGlyph } from '@decryp7/paddock-ui/icons';

export type SuTrayPlatform = 'macos' | 'windows' | 'linux';

/** One action of the tray footer's more-actions menu. */
export interface SuTrayMenuItem {
  readonly kind: 'item';
  /** What `menuAction` emits when the item is chosen. */
  readonly id: string;
  /** Sentence case, with "…" when the action asks for more (Settings…). */
  readonly label: string;
  /** A registered `MatIconRegistry` name (`dy:power`, `pd:settings`); decorative. */
  readonly icon?: string;
  /** The shortcut shown at the end of the row (⌘, or Ctrl+,): only for a key the popover window binds; decorative. */
  readonly keyHint?: string | null;
  readonly disabled?: boolean;
}

/** A hairline between two groups of the menu. */
export interface SuTrayMenuSeparator {
  readonly kind: 'separator';
}

/** The menu model: items and separators in order. */
export type SuTrayMenuEntry = SuTrayMenuItem | SuTrayMenuSeparator;

/** The entries as drawn: no separator at either end and never two in a row (an item hidden by its app leaves none behind). */
function tidy(entries: readonly SuTrayMenuEntry[]): readonly SuTrayMenuEntry[] {
  const out: SuTrayMenuEntry[] = [];
  for (const e of entries) {
    if (e.kind === 'separator' && (out.length === 0 || out[out.length - 1]!.kind === 'separator')) continue;
    out.push(e);
  }
  if (out[out.length - 1]?.kind === 'separator') out.pop();
  return out;
}

/**
 * Layout of a tray / menu-bar popup: 360 px wide, header, content fitted to its height (no empty gap, scrolls past 520 px)
 * and a footer toolbar. The window itself is frameless and transparent; this component draws the rounded surface
 * (12 px on macOS, 8 px on Windows 11), hairline border and soft shadow, one 16 px inset for header, content and footer.
 * Escape emits `dismissed`.
 *
 * The footer (family ADR 0035, the approved Dyno footer): `primaryLabel` draws the one primary action, "Open <app>" with its
 * arrow, right-aligned (`primary` emits on click); `menu` draws the ⋯ more-actions button beside it with a Material menu that
 * opens above it (`menuAction` emits the chosen item's id). The trigger is tinted and ringed while its menu is open. An app's
 * own footer content (`[slot=footer]`, e.g. a secondary "Mark all seen") stays at the start.
 *
 * Slots: `[slot=header]`, default (content), `[slot=footer]`.
 */
@Component({
  selector: 'pd-tray-popover',
  imports: [MatButton, MatIconButton, MatIcon, MatMenu, MatMenuItem, MatMenuTrigger, MatTooltip, PdGlyph],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    role: 'dialog',
    '[attr.aria-label]': 'label()',
    '[class]': '"pd-tray pd-tray--" + platform() + (translucent() ? " pd-tray--translucent" : "")',
    '(keydown.escape)': 'dismissed.emit()',
  },
  template: `
    <header class="pd-header"><ng-content select="[slot=header]" /></header>
    <div
      #content
      class="pd-content"
      [attr.tabindex]="scrolls() ? 0 : null"
      [attr.role]="scrolls() ? 'region' : null"
      [attr.aria-label]="scrolls() ? label() + ' content' : null"
      (scroll)="measure()"
    >
      <ng-content />
    </div>
    <!-- A fade over the foot of the content while there is more below (the mock's scroll cue); it ends at the bottom. -->
    <div class="pd-fade" aria-hidden="true" [class.pd-fade--on]="scrolls() && !atEnd()"></div>
    <footer class="pd-footer">
      <ng-content select="[slot=footer]" />
      @if (primaryLabel() || items().length) {
        <span class="pd-footer-spacer"></span>
        @if (primaryLabel(); as text) {
          <button type="button" matButton="filled" class="pd-tray-primary" (click)="primary.emit()"><pd-glyph name="arrow-right" [size]="18" />{{ text }}</button>
        }
        @if (items().length) {
          <button
            type="button"
            matIconButton
            class="pd-tray-more"
            [class.pd-tray-more--open]="menuOpen()"
            [matMenuTriggerFor]="moreMenu"
            [attr.aria-label]="menuLabel()"
            [matTooltip]="menuLabel()"
            matTooltipPosition="left"
            (menuOpened)="menuOpen.set(true)"
            (menuClosed)="menuOpen.set(false)"
          >
            <pd-glyph name="more" [size]="18" />
          </button>
          <mat-menu #moreMenu="matMenu" class="pd-tray-menu" yPosition="above" xPosition="before">
            @for (e of items(); track $index) {
              @if (e.kind === 'item') {
                <button mat-menu-item type="button" [disabled]="!!e.disabled" (click)="menuAction.emit(e.id)">
                  @if (e.icon) {
                    <mat-icon class="pd-tray-icon" [svgIcon]="e.icon" aria-hidden="true" />
                  }
                  <span class="pd-tray-row"><span class="pd-tray-label">{{ e.label }}</span>@if (e.keyHint) {<kbd aria-hidden="true">{{ e.keyHint }}</kbd>}</span>
                </button>
              } @else {
                <hr class="pd-tray-sep" role="separator" />
              }
            }
          </mat-menu>
        }
      }
    </footer>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      width: var(--pd-tray-width);
      max-width: 100%;
      max-height: var(--pd-tray-max-height);
      overflow: hidden;
      background: var(--pd-surface-3);
      border: 1px solid var(--pd-hairline);
      border-radius: 12px;
      box-shadow: var(--pd-shadow-2);
      color: var(--pd-text);
      font-size: var(--pd-text-body);
      animation: pd-tray-in var(--pd-duration) var(--pd-ease-enter);
    }
    :host(.pd-tray--windows) { border-radius: 8px; }
    :host(.pd-tray--linux) { border-radius: var(--pd-radius-popover); }
    /* The OS material (vibrancy, Acrylic) shows through; the solid colour is the designed fallback. */
    :host(.pd-tray--translucent) { background: color-mix(in srgb, var(--pd-surface-3) 72%, transparent); }
    .pd-header { display: flex; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-3) var(--pd-space-4); }
    .pd-content { scrollbar-width: thin; scrollbar-color: var(--pd-border-strong) transparent; }
    .pd-fade { flex: none; position: relative; height: 0; pointer-events: none; }
    .pd-fade::before { content: ''; position: absolute; inset-inline: 0; bottom: 0; height: var(--pd-space-6); background: linear-gradient(to bottom, transparent, var(--pd-surface-3)); opacity: 0; }
    .pd-fade--on::before { opacity: 1; }
    .pd-content:focus-visible { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: -2px; }
    .pd-header:empty, .pd-footer:empty { display: none; }
    .pd-content { flex: 1 1 auto; min-height: 0; overflow-y: auto; padding: 0 var(--pd-space-4) var(--pd-space-3); }
    .pd-footer { display: flex; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-4); border-top: 1px solid var(--pd-hairline); }
    .pd-footer-spacer { flex: 1; }
    /* The primary keeps its one line beside an app's own footer content. */
    .pd-tray-primary { flex: none; }
    .pd-tray-more { color: var(--pd-text-muted); }
    /* The ⋯ trigger shows while its menu is open: a tint and a ring, so it is clear what the menu belongs to. */
    .pd-tray-more--open { background: var(--pd-hover); box-shadow: inset 0 0 0 var(--pd-focus-width) var(--pd-focus-ring); }
    /* The menu's rows (drawn in the overlay, still styled here): 16 px icons, the label, a key hint at the end. */
    .mat-mdc-menu-item .pd-tray-icon { flex: none; width: 16px; height: 16px; margin: 0 var(--pd-space-3) 0 0; }
    .pd-tray-row { display: flex; align-items: center; gap: var(--pd-space-4); width: 100%; }
    .pd-tray-label { flex: 1; min-width: 0; }
    .pd-tray-sep { border: 0; border-top: 1px solid var(--pd-hairline); margin: var(--pd-space-1) 0; }
    kbd { padding: 0 var(--pd-space-1); border: 1px solid var(--pd-border-strong); border-radius: var(--pd-radius-tag); font: inherit; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    @keyframes pd-tray-in { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
    @media (prefers-reduced-motion: reduce) { :host { animation: none; } }
  `,
})
export class SuTrayPopover {
  private readonly content = viewChild.required<ElementRef<HTMLElement>>('content');
  /** True while the content scrolls (past 520 px): the region is then focusable and named, so a keyboard can scroll it (WCAG 2.1.1). */
  protected readonly scrolls = signal(false);
  /** True when scrolled to the end (the fade goes). */
  protected readonly atEnd = signal(true);

  /** Accessible name, e.g. "Dyno". */
  readonly label = input.required<string>();
  readonly platform = input<SuTrayPlatform>('macos');
  /** The shell draws an OS material behind the page; let it show through. */
  readonly translucent = input<boolean>(false);
  readonly dismissed = output<void>();
  /** The footer's primary action ("Open Dyno"); none without it. */
  readonly primaryLabel = input<string | null>(null);
  /** The more-actions menu; no ⋯ button while it has no item. */
  readonly menu = input<readonly SuTrayMenuEntry[]>([]);
  /** The ⋯ button's accessible name and tooltip. */
  readonly menuLabel = input<string>($localize`:@@pd.trayPopover.more:More actions`);
  /** The primary action was pressed. */
  readonly primary = output<void>();
  /** The id of the menu item chosen. */
  readonly menuAction = output<string>();
  protected readonly items = computed(() => tidy(this.menu()));
  /** True while the more-actions menu is open (the trigger is marked). */
  protected readonly menuOpen = signal(false);

  protected measure(): void {
    const el = this.content().nativeElement;
    this.scrolls.set(el.scrollHeight > el.clientHeight + 1);
    this.atEnd.set(el.scrollTop + el.clientHeight >= el.scrollHeight - 1);
  }

  constructor() {
    const destroy = inject(DestroyRef);
    afterNextRender(() => {
      const el = this.content().nativeElement;
      const check = (): void => this.measure();
      const resize = new ResizeObserver(check);
      resize.observe(el);
      const mutation = new MutationObserver(check);
      mutation.observe(el, { childList: true, subtree: true, characterData: true });
      destroy.onDestroy(() => {
        resize.disconnect();
        mutation.disconnect();
      });
      check();
    });
  }
}
